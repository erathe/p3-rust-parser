use clap::{Parser, ValueEnum};
use p3_parser::Message;
use p3_server::api;
use p3_server::api::state::AppState;
use p3_server::db;
use p3_server::decoder::DecoderConnection;
use p3_server::domain::race_event::RaceEvent;
use p3_server::engine::RaceEngine;
use p3_server::ingest::publisher::IngestPublisher;
use p3_server::workers::projection;
use p3_server::workers::race;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{info, warn};

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum RuntimeRole {
    Api,
    ProjectionWorker,
    RaceWorker,
}

#[derive(Parser)]
#[command(name = "p3-server")]
#[command(about = "BMX race timing server - bridges P3 decoders to WebSocket clients")]
struct Args {
    /// Runtime role for this process
    #[arg(long, value_enum, default_value_t = RuntimeRole::Api)]
    role: RuntimeRole,

    /// Decoder hostname
    #[arg(long, default_value = "localhost")]
    decoder_host: String,

    /// Decoder TCP port
    #[arg(long, default_value = "5403")]
    decoder_port: u16,

    /// HTTP/WebSocket server port
    #[arg(long, default_value = "3001")]
    port: u16,

    /// SQLite database path
    #[arg(long, default_value = "bmx-timing.db")]
    db_path: String,

    /// NATS URL for ingest JetStream
    #[arg(long, default_value = "nats://127.0.0.1:4222")]
    nats_url: String,

    /// Run without connecting to a decoder (UI-only mode)
    #[arg(long)]
    no_decoder: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    match args.role {
        RuntimeRole::Api => {
            let pool = db::create_pool(&args.db_path).await?;
            db::run_migrations(&pool).await?;
            run_api_role(&args, pool).await?
        }
        RuntimeRole::ProjectionWorker => {
            let pool = db::create_pool(&args.db_path).await?;
            db::run_migrations(&pool).await?;
            projection::run_projection_worker(&args.nats_url, &pool).await?
        }
        RuntimeRole::RaceWorker => race::run_race_worker(&args.nats_url).await?,
    }

    Ok(())
}

async fn run_api_role(args: &Args, pool: sqlx::SqlitePool) -> anyhow::Result<()> {
    // Broadcast channels
    let (broadcast_tx, _) = broadcast::channel::<Arc<Message>>(256);
    let (race_event_tx, _) = broadcast::channel::<Arc<RaceEvent>>(256);

    // Race engine
    let engine = Arc::new(Mutex::new(RaceEngine::new(race_event_tx.clone())));

    // NATS/JetStream ingest publisher
    let ingest_publisher = Arc::new(IngestPublisher::connect_and_provision(&args.nats_url).await?);
    info!(nats_url = %args.nats_url, "Connected to NATS and provisioned ingest stream");

    let state = AppState::new(
        broadcast_tx.clone(),
        race_event_tx.clone(),
        engine.clone(),
        pool.clone(),
        Some(ingest_publisher),
        args.nats_url.clone(),
    );

    // Task: persist race results when a race finishes
    {
        let mut results_rx = race_event_tx.subscribe();
        let results_pool = pool.clone();
        tokio::spawn(async move {
            loop {
                match results_rx.recv().await {
                    Ok(event) => {
                        if let RaceEvent::RaceFinished {
                            ref moto_id,
                            ref results,
                        } = *event
                        {
                            info!(moto_id = %moto_id, results = results.len(), "Persisting race results");
                            if let Err(e) = p3_server::db::queries::results::persist_results(
                                &results_pool,
                                moto_id,
                                results,
                            )
                            .await
                            {
                                warn!(error = %e, "Failed to persist race results");
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Result persistence task lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });
    }

    // Spawn decoder connection unless --no-decoder
    if !args.no_decoder {
        let (msg_tx, mut msg_rx) = mpsc::channel::<Message>(256);
        let decoder = DecoderConnection::new(args.decoder_host.clone(), args.decoder_port);

        // Task: read from decoder TCP → mpsc channel
        tokio::spawn(async move {
            decoder.run(msg_tx).await;
        });

        // Task: relay from mpsc → broadcast + feed race engine
        let relay_tx = broadcast_tx.clone();
        let relay_engine = engine.clone();
        tokio::spawn(async move {
            while let Some(message) = msg_rx.recv().await {
                // Feed passing messages to the race engine
                if let Message::Passing(ref passing) = message {
                    let mut eng = relay_engine.lock().await;
                    eng.process_passing(passing);
                }

                // Broadcast raw P3 message to all WebSocket clients
                if relay_tx.send(Arc::new(message)).is_err() {
                    // No active subscribers, that's fine
                }
            }
            warn!("Decoder message relay ended");
        });

        info!(
            host = %args.decoder_host,
            port = %args.decoder_port,
            "Decoder connection enabled"
        );
    } else {
        info!("Running in no-decoder mode (UI only)");
    }

    // Start HTTP/WebSocket server
    let app = api::router(state);
    let listener = TcpListener::bind(("0.0.0.0", args.port)).await?;

    info!(port = %args.port, "Server listening");

    axum::serve(listener, app).await?;

    Ok(())
}
