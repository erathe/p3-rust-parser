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
}

impl AppState {
    pub fn new(
        message_tx: broadcast::Sender<Arc<Message>>,
        db: SqlitePool,
        ingest_publisher: Option<Arc<IngestPublisher>>,
        nats_url: String,
    ) -> Self {
        Self {
            message_tx,
            db,
            ingest_publisher,
            nats_url,
        }
    }
}
