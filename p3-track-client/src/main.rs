use clap::Parser as ClapParser;
use p3_parser::{Message, Parser};
use p3_protocol::{ESCAPE, SOR};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Duration, MissedTickBehavior, interval, sleep};
use tracing::{error, info, warn};

const CONTRACT_VERSION: &str = "track_ingest.v1";

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

    /// Dev/test session ID used for grouping and replay
    #[arg(long, default_value = "dev-default")]
    session_id: String,

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

    /// Max in-memory unsent events before oldest events are dropped
    #[arg(long, default_value = "5000")]
    max_buffer_events: usize,

    /// Reconnect delay to local decoder after disconnect/failure
    #[arg(long, default_value = "3")]
    reconnect_secs: u64,

    /// HTTP request timeout in seconds
    #[arg(long, default_value = "10")]
    http_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IngestEvent {
    seq: u64,
    captured_at_us: u64,
    message: Message,
}

#[derive(Debug, Clone, Serialize)]
struct IngestBatchRequest {
    contract_version: String,
    session_id: String,
    track_id: String,
    client_id: String,
    events: Vec<IngestEvent>,
}

#[derive(Debug, Deserialize)]
struct IngestBatchResponse {
    accepted: usize,
    duplicates: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let args = Args::parse();
    run(args).await
}

async fn run(args: Args) -> anyhow::Result<()> {
    let ingest_url = format!(
        "{}/api/dev/ingest/batch",
        args.central_base_url.trim_end_matches('/')
    );

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(args.http_timeout_secs))
        .build()?;

    let mut next_seq: u64 = 1;

    loop {
        info!(
            decoder_host = %args.decoder_host,
            decoder_port = args.decoder_port,
            track_id = %args.track_id,
            client_id = %args.client_id,
            "Connecting to local track decoder",
        );

        match TcpStream::connect((args.decoder_host.as_str(), args.decoder_port)).await {
            Ok(mut stream) => {
                info!("Connected to local decoder");

                let mut framer = MessageFramer::new();
                let mut pending: Vec<IngestEvent> = Vec::with_capacity(args.batch_size.max(8));
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
                                        pending.push(IngestEvent {
                                            seq: next_seq,
                                            captured_at_us: now_unix_micros(),
                                            message,
                                        });
                                        next_seq = next_seq.saturating_add(1);

                                        if pending.len() >= args.batch_size {
                                            flush_batch(&http, &ingest_url, &args, &mut pending).await?;
                                        }
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Skipping unparsable message from decoder");
                                    }
                                }
                            }
                        }
                        _ = flush_tick.tick() => {
                            if !pending.is_empty() {
                                flush_batch(&http, &ingest_url, &args, &mut pending).await?;
                            }
                        }
                    }

                    trim_pending_if_needed(&args, &mut pending);
                }

                if !pending.is_empty() {
                    flush_batch(&http, &ingest_url, &args, &mut pending).await?;
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to connect to local decoder");
            }
        }

        sleep(Duration::from_secs(args.reconnect_secs)).await;
    }
}

fn trim_pending_if_needed(args: &Args, pending: &mut Vec<IngestEvent>) {
    if pending.len() <= args.max_buffer_events {
        return;
    }

    let to_drop = pending.len() - args.max_buffer_events;
    pending.drain(..to_drop);
    warn!(
        dropped_events = to_drop,
        max_buffer_events = args.max_buffer_events,
        "Dropped oldest unsent events due to backpressure",
    );
}

async fn flush_batch(
    http: &reqwest::Client,
    ingest_url: &str,
    args: &Args,
    pending: &mut Vec<IngestEvent>,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let events = std::mem::take(pending);
    let event_count = events.len();
    let request = IngestBatchRequest {
        contract_version: CONTRACT_VERSION.to_string(),
        session_id: args.session_id.clone(),
        track_id: args.track_id.clone(),
        client_id: args.client_id.clone(),
        events,
    };

    let response = http.post(ingest_url).json(&request).send().await;
    match response {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.json::<IngestBatchResponse>().await;
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
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            *pending = request.events;
            error!(
                status = %status,
                body = %body,
                queued_events = pending.len(),
                "Central server rejected ingest batch",
            );
            Ok(())
        }
        Err(e) => {
            *pending = request.events;
            warn!(
                error = %e,
                queued_events = pending.len(),
                "Failed to send batch to central server",
            );
            Ok(())
        }
    }
}

fn now_unix_micros() -> u64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    dur.as_micros().min(u64::MAX as u128) as u64
}

/// Accumulates bytes from a decoder stream and yields complete parsed messages.
struct MessageFramer {
    buffer: Vec<u8>,
    parser: Parser,
}

impl MessageFramer {
    fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            parser: Parser::new(),
        }
    }

    fn feed(&mut self, data: &[u8]) -> Vec<FrameResult> {
        self.buffer.extend_from_slice(data);
        let mut results = Vec::new();

        while let Some(message_end) = find_complete_message(&self.buffer) {
            let message_data = &self.buffer[..message_end];
            results.push(self.parser.parse(message_data));
            self.buffer.drain(..message_end);
        }

        results
    }
}

type FrameResult = Result<Message, p3_parser::ParseError>;

fn calculate_escaped_message_end(
    buffer: &[u8],
    start_pos: usize,
    unescaped_length: usize,
) -> Option<usize> {
    let mut buffer_pos = start_pos;
    let mut unescaped_count = 0;

    while unescaped_count < unescaped_length {
        if buffer_pos >= buffer.len() {
            return None;
        }

        if buffer[buffer_pos] == ESCAPE {
            if buffer_pos + 1 >= buffer.len() {
                return None;
            }
            buffer_pos += 2;
            unescaped_count += 1;
        } else {
            buffer_pos += 1;
            unescaped_count += 1;
        }
    }

    Some(buffer_pos)
}

fn find_complete_message(buffer: &[u8]) -> Option<usize> {
    let sor_pos = buffer.iter().position(|&b| b == SOR)?;

    if buffer.len() < sor_pos + 4 {
        return None;
    }

    let len_bytes = [buffer[sor_pos + 2], buffer[sor_pos + 3]];
    let unescaped_length = u16::from_le_bytes(len_bytes) as usize;
    calculate_escaped_message_end(buffer, sor_pos, unescaped_length)
}
