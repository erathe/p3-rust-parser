mod spool;

use anyhow::anyhow;
use clap::Parser as ClapParser;
use p3_contracts::{
    EventIdContext, TRACK_INGEST_CONTRACT_VERSION_V2, TrackIngestBatchRequest,
    TrackIngestBatchResponse, TrackIngestEvent, message_type_from_message,
};
use p3_parser::stream::MessageFramer;
use spool::SpoolStore;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Duration, MissedTickBehavior, interval, sleep};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(ClapParser, Debug)]
#[command(
    name = "p3-track-client",
    about = "Track-side client: reads local P3 decoder TCP, decodes messages, forwards JSON to central server"
)]
struct Args {
    /// Unique ID of this track-side client instance
    #[arg(long)]
    client_id: String,

    /// Track ID this client belongs to
    #[arg(long)]
    track_id: String,

    /// Local decoder hostname/IP (physically at the track)
    #[arg(long, default_value = "localhost")]
    decoder_host: String,

    /// Local decoder TCP port
    #[arg(long, default_value = "5403")]
    decoder_port: u16,

    /// Central server base URL (remote location)
    #[arg(long, default_value = "http://localhost:3001")]
    central_base_url: String,

    /// Max events per ingest POST
    #[arg(long, default_value = "50")]
    batch_size: usize,

    /// Flush interval in milliseconds if batch is not full
    #[arg(long, default_value = "1000")]
    flush_interval_ms: u64,

    /// Max locally spooled unsent events before oldest events are dropped
    #[arg(long, default_value = "5000")]
    max_buffer_events: usize,

    /// Path to the local SQLite spool database
    #[arg(long)]
    spool_db_path: Option<String>,

    /// Reconnect delay to local decoder after disconnect/failure
    #[arg(long, default_value = "3")]
    reconnect_secs: u64,

    /// HTTP request timeout in seconds
    #[arg(long, default_value = "10")]
    http_timeout_secs: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let args = Args::parse();
    run(args).await
}

async fn run(args: Args) -> anyhow::Result<()> {
    if args.batch_size == 0 {
        return Err(anyhow!("batch_size must be greater than 0"));
    }
    if args.flush_interval_ms == 0 {
        return Err(anyhow!("flush_interval_ms must be greater than 0"));
    }

    let ingest_url = format!(
        "{}/api/ingest/batch",
        args.central_base_url.trim_end_matches('/')
    );

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(args.http_timeout_secs))
        .build()?;

    let spool_path = resolve_spool_db_path(&args);
    let spool = SpoolStore::open(&spool_path).await?;
    let mut queued_events = spool.len().await?;

    info!(
        spool_db_path = %spool_path.display(),
        queued_events,
        "Initialized local ingest spool",
    );

    let boot_id = Uuid::new_v4().to_string();
    let mut next_seq: u64 = 1;

    loop {
        if queued_events > 0 {
            flush_spool_batch(&http, &ingest_url, &args, &spool, &mut queued_events).await?;
        }

        info!(
            decoder_host = %args.decoder_host,
            decoder_port = args.decoder_port,
            track_id = %args.track_id,
            client_id = %args.client_id,
            queued_events,
            "Connecting to local track decoder",
        );

        match TcpStream::connect((args.decoder_host.as_str(), args.decoder_port)).await {
            Ok(mut stream) => {
                info!("Connected to local decoder");

                let mut framer = MessageFramer::new();
                let mut flush_tick = interval(Duration::from_millis(args.flush_interval_ms));
                flush_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    let mut chunk = [0u8; 4096];
                    tokio::select! {
                        read_res = stream.read(&mut chunk) => {
                            let n = match read_res {
                                Ok(n) => n,
                                Err(e) => {
                                    warn!(error = %e, "Decoder socket read error");
                                    break;
                                }
                            };

                            if n == 0 {
                                warn!("Decoder connection closed");
                                break;
                            }

                            for framed in framer.feed(&chunk[..n]) {
                                match framed {
                                    Ok(message) => {
                                        let event = TrackIngestEvent {
                                            event_id: Uuid::new_v4(),
                                            track_id: args.track_id.clone(),
                                            event_id_context: EventIdContext {
                                                client_id: args.client_id.clone(),
                                                boot_id: boot_id.clone(),
                                                seq: next_seq,
                                            },
                                            captured_at_us: now_unix_micros(),
                                            message_type: message_type_from_message(&message)
                                                .to_string(),
                                            payload: message,
                                        };
                                        next_seq = next_seq.saturating_add(1);

                                        spool.enqueue(&event).await?;
                                        queued_events = queued_events.saturating_add(1);
                                        trim_spool_if_needed(&args, &spool, &mut queued_events).await?;

                                        if queued_events >= args.batch_size {
                                            flush_spool_batch(&http, &ingest_url, &args, &spool, &mut queued_events).await?;
                                        }
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Skipping unparsable message from decoder");
                                    }
                                }
                            }
                        }
                        _ = flush_tick.tick() => {
                            if queued_events > 0 {
                                flush_spool_batch(&http, &ingest_url, &args, &spool, &mut queued_events).await?;
                            }
                        }
                    }
                }

                flush_spool_batch(&http, &ingest_url, &args, &spool, &mut queued_events).await?;
            }
            Err(e) => {
                warn!(error = %e, "Failed to connect to local decoder");
                if queued_events > 0 {
                    flush_spool_batch(&http, &ingest_url, &args, &spool, &mut queued_events)
                        .await?;
                }
            }
        }

        sleep(Duration::from_secs(args.reconnect_secs)).await;
    }
}

async fn trim_spool_if_needed(
    args: &Args,
    spool: &SpoolStore,
    queued_events: &mut usize,
) -> anyhow::Result<()> {
    if *queued_events <= args.max_buffer_events {
        return Ok(());
    }

    let to_drop = *queued_events - args.max_buffer_events;
    let dropped = spool.drop_oldest(to_drop).await?;
    *queued_events = queued_events.saturating_sub(dropped);

    warn!(
        dropped_events = dropped,
        max_buffer_events = args.max_buffer_events,
        queued_events = *queued_events,
        "Dropped oldest spooled events due to backpressure",
    );

    Ok(())
}

async fn flush_spool_batch(
    http: &reqwest::Client,
    ingest_url: &str,
    args: &Args,
    spool: &SpoolStore,
    queued_events: &mut usize,
) -> anyhow::Result<()> {
    if *queued_events == 0 {
        return Ok(());
    }

    let loaded = spool.load_batch(args.batch_size).await?;
    if loaded.dropped_invalid > 0 {
        *queued_events = queued_events.saturating_sub(loaded.dropped_invalid);
        warn!(
            dropped_invalid = loaded.dropped_invalid,
            queued_events = *queued_events,
            "Removed invalid rows from local ingest spool",
        );
    }
    if loaded.rows.is_empty() {
        return Ok(());
    }

    let ids: Vec<i64> = loaded.rows.iter().map(|row| row.id).collect();
    let events: Vec<TrackIngestEvent> = loaded.rows.into_iter().map(|row| row.event).collect();
    let event_count = events.len();

    let request = TrackIngestBatchRequest {
        contract_version: TRACK_INGEST_CONTRACT_VERSION_V2.to_string(),
        track_id: args.track_id.clone(),
        events,
    };

    let response = http.post(ingest_url).json(&request).send().await;
    match response {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.json::<TrackIngestBatchResponse>().await;
            match body {
                Ok(summary) => {
                    info!(
                        sent = event_count,
                        accepted = summary.accepted,
                        duplicates = summary.duplicates,
                        "Delivered batch to central server",
                    );
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        sent = event_count,
                        "Batch accepted but response body could not be parsed",
                    );
                }
            }

            let deleted = spool.ack_batch(&ids).await?;
            *queued_events = queued_events.saturating_sub(deleted);
            if deleted != event_count {
                warn!(
                    expected = event_count,
                    deleted, "Spool ack removed fewer rows than expected; resyncing queue depth",
                );
                *queued_events = spool.len().await?;
            }

            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(
                status = %status,
                body = %body,
                queued_events = *queued_events,
                "Central server rejected ingest batch",
            );
            Ok(())
        }
        Err(e) => {
            warn!(
                error = %e,
                queued_events = *queued_events,
                "Failed to send batch to central server",
            );
            Ok(())
        }
    }
}

fn resolve_spool_db_path(args: &Args) -> PathBuf {
    if let Some(path) = &args.spool_db_path {
        return PathBuf::from(path);
    }

    PathBuf::from(format!(
        "track-ingest-spool-{}-{}.db",
        sanitize_for_filename(&args.track_id),
        sanitize_for_filename(&args.client_id)
    ))
}

fn sanitize_for_filename(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn now_unix_micros() -> u64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    dur.as_micros().min(u64::MAX as u128) as u64
}
