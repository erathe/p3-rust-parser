use clap::{Parser, ValueEnum};
use p3_parser::Message;
use p3_server::api;
use p3_server::api::state::AppState;
use p3_server::db;
use p3_server::ingest::publisher::IngestPublisher;
use p3_server::workers::projection;
use p3_server::workers::race;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::info;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum RuntimeRole {
    Api,
    ProjectionWorker,
    RaceWorker,
}

#[derive(Parser)]
#[command(name = "p3-server")]
#[command(about = "BMX race timing server API and stream workers")]
struct Args {
    /// Runtime role for this process
    #[arg(long, value_enum, default_value_t = RuntimeRole::Api)]
    role: RuntimeRole,

    /// HTTP/WebSocket server port
    #[arg(long, default_value = "3001")]
    port: u16,

    /// SQLite database path
    #[arg(long, default_value = "bmx-timing.db")]
    db_path: String,

    /// NATS URL for ingest JetStream
    #[arg(long, default_value = "nats://127.0.0.1:4222")]
    nats_url: String,
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

    // NATS/JetStream ingest publisher
    let ingest_publisher = Arc::new(IngestPublisher::connect_and_provision(&args.nats_url).await?);
    info!(nats_url = %args.nats_url, "Connected to NATS and provisioned ingest stream");

    let state = AppState::new(
        broadcast_tx.clone(),
        pool.clone(),
        Some(ingest_publisher),
        args.nats_url.clone(),
    );

    info!("API role running in stream-only mode; ingest via /api/ingest/batch");

    // Start HTTP/WebSocket server
    let app = api::router(state);
    let listener = TcpListener::bind(("0.0.0.0", args.port)).await?;

    info!(port = %args.port, "Server listening");

    axum::serve(listener, app).await?;

    Ok(())
}
