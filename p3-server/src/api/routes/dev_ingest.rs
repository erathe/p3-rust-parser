use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use p3_parser::Message;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::queries::dev_ingest::{self, IngestMessageRow, InsertSummary, PreparedIngestEvent};

#[derive(Debug, Clone, Deserialize)]
pub struct IngestBatchRequest {
    pub contract_version: String,
    pub session_id: String,
    pub track_id: String,
    pub client_id: String,
    pub events: Vec<IngestEvent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IngestEvent {
    pub seq: u64,
    pub captured_at_us: u64,
    pub message: Message,
}

#[derive(Debug, Serialize)]
pub struct IngestBatchResponse {
    pub accepted: usize,
    pub duplicates: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub session_id: String,
    pub track_id: Option<String>,
    pub client_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct IngestMessage {
    pub id: String,
    pub session_id: String,
    pub track_id: String,
    pub client_id: String,
    pub seq: u64,
    pub captured_at_us: u64,
    pub message_type: String,
    pub message: Message,
    pub received_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ReplayRequest {
    pub session_id: String,
    pub track_id: Option<String>,
    pub client_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ReplayResponse {
    pub replayed: usize,
}

/// POST /api/dev/ingest/batch
///
/// Entry point for remote track clients. Accepts decoded P3 JSON payloads
/// and persists them for diagnostics/replay.
pub async fn ingest_batch(
    State(state): State<AppState>,
    Json(req): Json<IngestBatchRequest>,
) -> Result<Json<IngestBatchResponse>, ApiError> {
    if req.contract_version != "track_ingest.v1" {
        return Err(ApiError::BadRequest(format!(
            "Unsupported contract_version: {}",
            req.contract_version
        )));
    }
    if req.session_id.trim().is_empty()
        || req.track_id.trim().is_empty()
        || req.client_id.trim().is_empty()
    {
        return Err(ApiError::BadRequest(
            "session_id, track_id, and client_id are required".to_string(),
        ));
    }
    if req.events.is_empty() {
        return Ok(Json(IngestBatchResponse {
            accepted: 0,
            duplicates: 0,
        }));
    }

    let mut prepared = Vec::with_capacity(req.events.len());
    for event in &req.events {
        let seq = i64::try_from(event.seq)
            .map_err(|_| ApiError::BadRequest("seq is too large".to_string()))?;
        let captured_at_us = i64::try_from(event.captured_at_us)
            .map_err(|_| ApiError::BadRequest("captured_at_us is too large".to_string()))?;
        let payload_json = serde_json::to_string(&event.message)
            .map_err(|e| ApiError::Internal(format!("Failed to serialize message: {e}")))?;

        prepared.push(PreparedIngestEvent {
            seq,
            captured_at_us,
            message_type: message_type_name(&event.message).to_string(),
            payload_json,
        });
    }

    let summary: InsertSummary = dev_ingest::insert_batch(
        &state.db,
        &req.session_id,
        &req.track_id,
        &req.client_id,
        &prepared,
    )
    .await?;

    for event in &req.events {
        if let Message::Status(status) = &event.message {
            if let Some(decoder_id) = &status.decoder_id {
                sqlx::query(
                    "INSERT INTO decoder_status (decoder_id, noise, temperature, gps_status, satellites, last_seen) \
                     VALUES (?, ?, ?, ?, ?, datetime('now')) \
                     ON CONFLICT(decoder_id) DO UPDATE SET \
                       noise = excluded.noise, \
                       temperature = excluded.temperature, \
                       gps_status = excluded.gps_status, \
                       satellites = excluded.satellites, \
                       last_seen = datetime('now')",
                )
                .bind(decoder_id)
                .bind(status.noise as i64)
                .bind(status.temperature as i64)
                .bind(status.gps_status as i64)
                .bind(status.satellites as i64)
                .execute(&state.db)
                .await?;
            }
        }

        let _ = state.message_tx.send(Arc::new(event.message.clone()));
    }

    Ok(Json(IngestBatchResponse {
        accepted: summary.accepted,
        duplicates: summary.duplicates,
    }))
}

/// GET /api/dev/ingest/messages
///
/// Returns persisted ingest messages for diagnostics and replay.
pub async fn list_messages(
    State(state): State<AppState>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<IngestMessage>>, ApiError> {
    if query.session_id.trim().is_empty() {
        return Err(ApiError::BadRequest("session_id is required".to_string()));
    }

    let limit = i64::from(query.limit.unwrap_or(1000).min(10_000));
    let rows = dev_ingest::list_messages(
        &state.db,
        &query.session_id,
        query.track_id.as_deref(),
        query.client_id.as_deref(),
        limit,
    )
    .await?;

    let mapped = rows_to_messages(rows)?;
    Ok(Json(mapped))
}

/// POST /api/dev/ingest/replay
///
/// Replays stored ingest messages back onto the WebSocket message channel.
pub async fn replay(
    State(state): State<AppState>,
    Json(req): Json<ReplayRequest>,
) -> Result<Json<ReplayResponse>, ApiError> {
    if req.session_id.trim().is_empty() {
        return Err(ApiError::BadRequest("session_id is required".to_string()));
    }

    let limit = i64::from(req.limit.unwrap_or(1000).min(10_000));
    let rows = dev_ingest::list_messages(
        &state.db,
        &req.session_id,
        req.track_id.as_deref(),
        req.client_id.as_deref(),
        limit,
    )
    .await?;

    let messages = rows_to_messages(rows)?;
    let replayed = messages.len();
    for message in messages {
        let _ = state.message_tx.send(Arc::new(message.message));
    }

    Ok(Json(ReplayResponse { replayed }))
}

fn rows_to_messages(rows: Vec<IngestMessageRow>) -> Result<Vec<IngestMessage>, ApiError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let message: Message = serde_json::from_str(&row.payload_json).map_err(|e| {
            ApiError::Internal(format!(
                "Failed to parse stored payload_json for row {}: {e}",
                row.id
            ))
        })?;

        out.push(IngestMessage {
            id: row.id,
            session_id: row.session_id,
            track_id: row.track_id,
            client_id: row.client_id,
            seq: row.seq as u64,
            captured_at_us: row.captured_at_us as u64,
            message_type: row.message_type,
            message,
            received_at: row.received_at,
        });
    }
    Ok(out)
}

fn message_type_name(message: &Message) -> &'static str {
    match message {
        Message::Passing(_) => "PASSING",
        Message::Status(_) => "STATUS",
        Message::Version(_) => "VERSION",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::race_event::RaceEvent;
    use crate::engine::RaceEngine;
    use p3_parser::{PassingMessage, StatusMessage};
    use std::sync::Arc;
    use tokio::sync::{Mutex, broadcast};

    async fn test_state() -> AppState {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::run_migrations(&db).await.unwrap();

        let (message_tx, _) = broadcast::channel(32);
        let (race_event_tx, _) = broadcast::channel::<Arc<RaceEvent>>(32);
        let engine = Arc::new(Mutex::new(RaceEngine::new(race_event_tx.clone())));
        AppState::new(message_tx, race_event_tx, engine, db)
    }

    #[tokio::test]
    async fn ingest_batch_persists_and_deduplicates() {
        let state = test_state().await;
        let request = IngestBatchRequest {
            contract_version: "track_ingest.v1".to_string(),
            session_id: "session-a".to_string(),
            track_id: "track-1".to_string(),
            client_id: "client-1".to_string(),
            events: vec![
                IngestEvent {
                    seq: 1,
                    captured_at_us: 10,
                    message: Message::Status(StatusMessage {
                        noise: 50,
                        gps_status: 1,
                        temperature: 215,
                        satellites: 7,
                        decoder_id: Some("D1000C00".to_string()),
                    }),
                },
                IngestEvent {
                    seq: 2,
                    captured_at_us: 11,
                    message: Message::Passing(PassingMessage {
                        passing_number: 1,
                        transponder_id: 1001,
                        rtc_time_us: 123_456,
                        utc_time_us: None,
                        strength: Some(100),
                        hits: Some(20),
                        transponder_string: Some("FL-01001".to_string()),
                        flags: 0,
                        decoder_id: Some("D1000C00".to_string()),
                    }),
                },
            ],
        };

        let first = ingest_batch(State(state.clone()), Json(request.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(first.accepted, 2);
        assert_eq!(first.duplicates, 0);

        let second = ingest_batch(State(state.clone()), Json(request))
            .await
            .unwrap()
            .0;
        assert_eq!(second.accepted, 0);
        assert_eq!(second.duplicates, 2);

        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ingest_messages")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn ingest_batch_allows_same_seq_for_different_sessions() {
        let state = test_state().await;

        let first = IngestBatchRequest {
            contract_version: "track_ingest.v1".to_string(),
            session_id: "session-1".to_string(),
            track_id: "track-1".to_string(),
            client_id: "client-1".to_string(),
            events: vec![IngestEvent {
                seq: 1,
                captured_at_us: 100,
                message: Message::Status(StatusMessage {
                    noise: 45,
                    gps_status: 1,
                    temperature: 210,
                    satellites: 8,
                    decoder_id: Some("D1000C00".to_string()),
                }),
            }],
        };

        let second = IngestBatchRequest {
            contract_version: "track_ingest.v1".to_string(),
            session_id: "session-2".to_string(),
            track_id: "track-1".to_string(),
            client_id: "client-1".to_string(),
            events: vec![IngestEvent {
                seq: 1,
                captured_at_us: 101,
                message: Message::Status(StatusMessage {
                    noise: 46,
                    gps_status: 1,
                    temperature: 211,
                    satellites: 7,
                    decoder_id: Some("D1000C00".to_string()),
                }),
            }],
        };

        let first_result = ingest_batch(State(state.clone()), Json(first))
            .await
            .unwrap()
            .0;
        assert_eq!(first_result.accepted, 1);
        assert_eq!(first_result.duplicates, 0);

        let second_result = ingest_batch(State(state.clone()), Json(second))
            .await
            .unwrap()
            .0;
        assert_eq!(second_result.accepted, 1);
        assert_eq!(second_result.duplicates, 0);

        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM ingest_messages WHERE client_id = 'client-1' AND seq = 1",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn replay_rebroadcasts_stored_messages() {
        let state = test_state().await;
        let request = IngestBatchRequest {
            contract_version: "track_ingest.v1".to_string(),
            session_id: "session-b".to_string(),
            track_id: "track-2".to_string(),
            client_id: "client-2".to_string(),
            events: vec![IngestEvent {
                seq: 1,
                captured_at_us: 55,
                message: Message::Status(StatusMessage {
                    noise: 48,
                    gps_status: 1,
                    temperature: 220,
                    satellites: 8,
                    decoder_id: Some("D2000C00".to_string()),
                }),
            }],
        };

        let _ = ingest_batch(State(state.clone()), Json(request))
            .await
            .unwrap();

        // Subscribe after ingest so we only observe replay output.
        let mut rx = state.message_tx.subscribe();
        let replayed = replay(
            State(state.clone()),
            Json(ReplayRequest {
                session_id: "session-b".to_string(),
                track_id: Some("track-2".to_string()),
                client_id: Some("client-2".to_string()),
                limit: Some(100),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(replayed.replayed, 1);

        let msg = rx.recv().await.unwrap();
        match msg.as_ref() {
            Message::Status(status) => {
                assert_eq!(status.decoder_id.as_deref(), Some("D2000C00"));
            }
            other => panic!("Expected status message, got {other:?}"),
        }
    }
}
