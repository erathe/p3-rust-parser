use p3_parser::Message;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use crate::domain::race_event::RaceEvent;
use crate::engine::RaceEngine;

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
}

impl AppState {
    pub fn new(
        message_tx: broadcast::Sender<Arc<Message>>,
        race_event_tx: broadcast::Sender<Arc<RaceEvent>>,
        engine: Arc<Mutex<RaceEngine>>,
        db: SqlitePool,
    ) -> Self {
        Self {
            message_tx,
            race_event_tx,
            engine,
            db,
        }
    }
}
