use p3_parser::Message;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};

use crate::domain::race_event::RaceEvent;
use crate::engine::RaceEngine;
use crate::ingest::publisher::IngestPublisher;

/// Shared application state available to all Axum handlers.
#[derive(Clone)]
pub struct AppState {
    /// Broadcast channel for real-time P3 messages to WebSocket clients.
    pub message_tx: broadcast::Sender<Arc<Message>>,
    /// Broadcast channel for race events to WebSocket clients.
    pub race_event_tx: broadcast::Sender<Arc<RaceEvent>>,
    /// The race engine (mutable, behind a mutex for shared access).
    pub engine: Arc<Mutex<RaceEngine>>,
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
        race_event_tx: broadcast::Sender<Arc<RaceEvent>>,
        engine: Arc<Mutex<RaceEngine>>,
        db: SqlitePool,
        ingest_publisher: Option<Arc<IngestPublisher>>,
        nats_url: String,
    ) -> Self {
        Self {
            message_tx,
            race_event_tx,
            engine,
            db,
            ingest_publisher,
            nats_url,
        }
    }
}
