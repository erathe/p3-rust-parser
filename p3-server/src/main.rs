use clap::{Parser, ValueEnum};
use p3_parser::Message;
use p3_server::api;
use p3_server::api::auth::TrackAuthConfig;
use p3_server::api::metrics::AppMetrics;
use p3_server::api::state::AppState;
use p3_server::db;
use p3_server::ingest::publisher::IngestPublisher;
use p3_server::workers::projection;
use p3_server::workers::race;
use std::collections::HashMap;
use std::str::FromStr;
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

    /// Enable legacy development ingest/replay API routes under /api/dev/ingest/*
    #[arg(long)]
    enable_dev_ingest: bool,

    /// Enforce track-scoped token auth on ingest and live websocket endpoints
    #[arg(long, default_value_t = false)]
    enforce_track_auth: bool,

    /// Track token mapping in the form track_id=token (repeatable)
    #[arg(long = "track-token")]
    track_tokens: Vec<TrackTokenArg>,
}

#[derive(Clone, Debug)]
struct TrackTokenArg {
    track_id: String,
    token: String,
}

impl FromStr for TrackTokenArg {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (track_id, token) = value
            .split_once('=')
            .ok_or_else(|| "expected <track_id=token>".to_string())?;

        let track_id = track_id.trim();
        let token = token.trim();

        if track_id.is_empty() {
            return Err("track_id cannot be empty".to_string());
        }
        if token.is_empty() {
            return Err("token cannot be empty".to_string());
        }

        Ok(Self {
            track_id: track_id.to_string(),
            token: token.to_string(),
        })
    }
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
        RuntimeRole::RaceWorker => {
            let pool = db::create_pool(&args.db_path).await?;
            db::run_migrations(&pool).await?;
            race::run_race_worker(&args.nats_url, &pool).await?
        }
    }

    Ok(())
}

async fn run_api_role(args: &Args, pool: sqlx::SqlitePool) -> anyhow::Result<()> {
    // Broadcast channels
    let (broadcast_tx, _) = broadcast::channel::<Arc<Message>>(256);

    // NATS/JetStream ingest publisher
    let ingest_publisher = Arc::new(IngestPublisher::connect_and_provision(&args.nats_url).await?);
    info!(nats_url = %args.nats_url, "Connected to NATS and provisioned ingest stream");

    let mut track_tokens = HashMap::new();
    for entry in &args.track_tokens {
        if track_tokens
            .insert(entry.track_id.clone(), entry.token.clone())
            .is_some()
        {
            return Err(anyhow::anyhow!(
                "duplicate --track-token provided for track_id '{}'",
                entry.track_id
            ));
        }
    }
    let track_auth = TrackAuthConfig::new(args.enforce_track_auth, track_tokens);

    let state = AppState::new(
        broadcast_tx.clone(),
        pool.clone(),
        Some(ingest_publisher),
        args.nats_url.clone(),
        args.enable_dev_ingest,
        track_auth.clone(),
        Arc::new(AppMetrics::new()),
    );

    info!("API role running in stream-only mode; ingest via /api/ingest/batch");
    info!(
        enable_dev_ingest = args.enable_dev_ingest,
        "Legacy dev ingest routes runtime gate"
    );
    info!(
        enforce_track_auth = track_auth.enforce_track_auth(),
        configured_track_tokens = track_auth.token_count(),
        "Track auth runtime configuration"
    );

    // Start HTTP/WebSocket server
    let app = api::router(state);
    let listener = TcpListener::bind(("0.0.0.0", args.port)).await?;

    info!(port = %args.port, "Server listening");

    axum::serve(listener, app).await?;

    Ok(())
}
