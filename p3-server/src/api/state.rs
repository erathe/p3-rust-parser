use crate::api::auth::TrackAuthConfig;
use crate::api::metrics::AppMetrics;
use crate::ingest::publisher::IngestPublisher;
use p3_parser::Message;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Shared application state available to all Axum handlers.
#[derive(Clone)]
pub struct AppState {
    /// Broadcast channel for real-time P3 messages to WebSocket clients.
    pub message_tx: broadcast::Sender<Arc<Message>>,
    /// SQLite connection pool.
    pub db: SqlitePool,
    /// Track ingest publisher (JetStream).
    pub ingest_publisher: Option<Arc<IngestPublisher>>,
    /// NATS URL used by the API server.
    pub nats_url: String,
    /// Enables legacy dev ingest API routes.
    pub enable_dev_ingest: bool,
    /// Track-scoped ingest/live auth config.
    pub track_auth: TrackAuthConfig,
    /// In-process API metrics registry.
    pub metrics: Arc<AppMetrics>,
}

impl AppState {
    pub fn new(
        message_tx: broadcast::Sender<Arc<Message>>,
        db: SqlitePool,
        ingest_publisher: Option<Arc<IngestPublisher>>,
        nats_url: String,
        enable_dev_ingest: bool,
        track_auth: TrackAuthConfig,
        metrics: Arc<AppMetrics>,
    ) -> Self {
        Self {
            message_tx,
            db,
            ingest_publisher,
            nats_url,
            enable_dev_ingest,
            track_auth,
            metrics,
        }
    }
}
