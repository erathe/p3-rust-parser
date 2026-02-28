use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use p3_parser::Message;
use p3_protocol::fields::reserved_ids;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::models::TimingLoopRow;

#[derive(Debug, Deserialize)]
pub struct DiscoveryQuery {
    pub window_seconds: Option<u32>,
    pub max_messages: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct TrackOnboardingDiscoveryResponse {
    pub track_id: String,
    pub window_seconds: u32,
    pub sampled_messages: usize,
    pub decoders: Vec<DiscoveredDecoder>,
    pub gate_beacons: Vec<ObservedGateBeacon>,
    pub generated_at: String,
}

#[derive(Debug, Serialize)]
pub struct DiscoveredDecoder {
    pub decoder_id: String,
    pub passing_count: u64,
    pub status_count: u64,
    pub version_count: u64,
    pub gate_hits: u64,
    pub last_seen: String,
    pub mapped_loop_id: Option<String>,
    pub mapped_loop_name: Option<String>,
    pub mapped_role: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ObservedGateBeacon {
    pub transponder_id: u32,
    pub hits: u64,
}

#[derive(Debug, sqlx::FromRow)]
struct RecentIngestRow {
    payload_json: String,
    received_at: String,
}

#[derive(Debug, Default)]
struct DecoderAggregate {
    passing_count: u64,
    status_count: u64,
    version_count: u64,
    gate_hits: u64,
    last_seen: String,
}

/// GET /api/tracks/{track_id}/onboarding/discovery
///
/// Track-scoped live decoder discovery based on recent ingest messages.
pub async fn discovery(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Query(query): Query<DiscoveryQuery>,
) -> Result<Json<TrackOnboardingDiscoveryResponse>, ApiError> {
    // Ensure track exists
    let track_exists: Option<String> = sqlx::query_scalar("SELECT id FROM tracks WHERE id = ?")
        .bind(&track_id)
        .fetch_optional(&state.db)
        .await?;
    if track_exists.is_none() {
        return Err(ApiError::NotFound(format!("Track {} not found", track_id)));
    }

    let window_seconds = query.window_seconds.unwrap_or(180).clamp(30, 3600);
    let max_messages = i64::from(query.max_messages.unwrap_or(5_000).clamp(100, 20_000));
    let window_expr = format!("-{} seconds", window_seconds);

    let rows = sqlx::query_as::<_, RecentIngestRow>(
        "SELECT payload_json, received_at \
         FROM ingest_messages \
         WHERE track_id = ? \
           AND received_at >= datetime('now', ?) \
         ORDER BY received_at DESC \
         LIMIT ?",
    )
    .bind(&track_id)
    .bind(&window_expr)
    .bind(max_messages)
    .fetch_all(&state.db)
    .await?;

    let loops = sqlx::query_as::<_, TimingLoopRow>(
        "SELECT * FROM timing_loops WHERE track_id = ? ORDER BY position",
    )
    .bind(&track_id)
    .fetch_all(&state.db)
    .await?;

    let loop_by_decoder: HashMap<&str, &TimingLoopRow> =
        loops.iter().map(|l| (l.decoder_id.as_str(), l)).collect();

    let mut aggregates: HashMap<String, DecoderAggregate> = HashMap::new();
    let mut gate_beacon_hits: HashMap<u32, u64> = HashMap::new();

    for row in &rows {
        let message = match serde_json::from_str::<Message>(&row.payload_json) {
            Ok(msg) => msg,
            Err(_) => continue,
        };

        let (decoder_id, message_kind, gate_beacon_id) = match message {
            Message::Passing(passing) => (
                passing.decoder_id,
                "passing",
                reserved_ids::is_reserved(passing.transponder_id).then_some(passing.transponder_id),
            ),
            Message::Status(status) => (status.decoder_id, "status", None),
            Message::Version(version) => (Some(version.decoder_id), "version", None),
        };

        let Some(decoder_id) = decoder_id else {
            continue;
        };

        let aggregate = aggregates
            .entry(decoder_id)
            .or_insert_with(DecoderAggregate::default);

        match message_kind {
            "passing" => aggregate.passing_count += 1,
            "status" => aggregate.status_count += 1,
            "version" => aggregate.version_count += 1,
            _ => {}
        }

        if let Some(gate_beacon_id) = gate_beacon_id {
            aggregate.gate_hits += 1;
            *gate_beacon_hits.entry(gate_beacon_id).or_insert(0) += 1;
        }

        if aggregate.last_seen.is_empty() || row.received_at > aggregate.last_seen {
            aggregate.last_seen = row.received_at.clone();
        }
    }

    let mut decoders: Vec<DiscoveredDecoder> = aggregates
        .into_iter()
        .map(|(decoder_id, agg)| {
            let mapped = loop_by_decoder.get(decoder_id.as_str()).copied();
            let mapped_role = mapped.map(|loop_row| {
                if loop_row.is_start {
                    "start".to_string()
                } else if loop_row.is_finish {
                    "finish".to_string()
                } else {
                    "split".to_string()
                }
            });

            DiscoveredDecoder {
                decoder_id,
                passing_count: agg.passing_count,
                status_count: agg.status_count,
                version_count: agg.version_count,
                gate_hits: agg.gate_hits,
                last_seen: agg.last_seen,
                mapped_loop_id: mapped.map(|l| l.id.clone()),
                mapped_loop_name: mapped.map(|l| l.name.clone()),
                mapped_role,
            }
        })
        .collect();

    decoders.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

    let mut gate_beacons: Vec<ObservedGateBeacon> = gate_beacon_hits
        .into_iter()
        .map(|(transponder_id, hits)| ObservedGateBeacon {
            transponder_id,
            hits,
        })
        .collect();
    gate_beacons.sort_by(|a, b| b.hits.cmp(&a.hits));

    Ok(Json(TrackOnboardingDiscoveryResponse {
        track_id,
        window_seconds,
        sampled_messages: rows.len(),
        decoders,
        gate_beacons,
        generated_at: chrono::Utc::now().to_rfc3339(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::routes::dev_ingest::{IngestBatchRequest, IngestEvent, ingest_batch};
    use crate::domain::race_event::RaceEvent;
    use crate::engine::RaceEngine;
    use p3_parser::{PassingMessage, StatusMessage};
    use std::sync::Arc;
    use tokio::sync::{Mutex, broadcast};

    async fn test_state() -> AppState {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::run_migrations(&db).await.unwrap();

        sqlx::query(
            "INSERT INTO tracks (id, name, hill_type, gate_beacon_id) VALUES ('track-a', 'Track A', '8m', 9992)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tracks (id, name, hill_type, gate_beacon_id) VALUES ('track-b', 'Track B', '8m', 9992)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO timing_loops (id, track_id, name, decoder_id, position, is_start, is_finish) \
             VALUES ('loop-a', 'track-a', 'Start Hill', 'D1000C00', 0, 1, 0)",
        )
        .execute(&db)
        .await
        .unwrap();

        let (message_tx, _) = broadcast::channel(32);
        let (race_event_tx, _) = broadcast::channel::<Arc<RaceEvent>>(32);
        let engine = Arc::new(Mutex::new(RaceEngine::new(race_event_tx.clone())));
        AppState::new(
            message_tx,
            race_event_tx,
            engine,
            db,
            None,
            "nats://127.0.0.1:4222".to_string(),
        )
    }

    #[tokio::test]
    async fn discovery_is_track_scoped_and_marks_mapped_loops() {
        let state = test_state().await;

        let track_a_batch = IngestBatchRequest {
            contract_version: "track_ingest.v1".to_string(),
            session_id: "sess-a".to_string(),
            track_id: "track-a".to_string(),
            client_id: "client-a".to_string(),
            events: vec![
                IngestEvent {
                    seq: 1,
                    captured_at_us: 100,
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
                    captured_at_us: 101,
                    message: Message::Passing(PassingMessage {
                        passing_number: 1,
                        transponder_id: 9992,
                        rtc_time_us: 10_000,
                        utc_time_us: None,
                        strength: Some(120),
                        hits: Some(20),
                        transponder_string: None,
                        flags: 0,
                        decoder_id: Some("D1000C00".to_string()),
                    }),
                },
            ],
        };

        let track_b_batch = IngestBatchRequest {
            contract_version: "track_ingest.v1".to_string(),
            session_id: "sess-b".to_string(),
            track_id: "track-b".to_string(),
            client_id: "client-b".to_string(),
            events: vec![IngestEvent {
                seq: 1,
                captured_at_us: 102,
                message: Message::Status(StatusMessage {
                    noise: 51,
                    gps_status: 1,
                    temperature: 216,
                    satellites: 8,
                    decoder_id: Some("D2000C00".to_string()),
                }),
            }],
        };

        let _ = ingest_batch(State(state.clone()), Json(track_a_batch))
            .await
            .unwrap();
        let _ = ingest_batch(State(state.clone()), Json(track_b_batch))
            .await
            .unwrap();

        let response = discovery(
            State(state.clone()),
            Path("track-a".to_string()),
            Query(DiscoveryQuery {
                window_seconds: Some(300),
                max_messages: Some(1000),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.track_id, "track-a");
        assert_eq!(response.decoders.len(), 1);
        assert_eq!(response.decoders[0].decoder_id, "D1000C00");
        assert_eq!(response.decoders[0].status_count, 1);
        assert_eq!(response.decoders[0].passing_count, 1);
        assert_eq!(response.decoders[0].gate_hits, 1);
        assert_eq!(response.decoders[0].mapped_role.as_deref(), Some("start"));
        assert_eq!(response.gate_beacons.len(), 1);
        assert_eq!(response.gate_beacons[0].transponder_id, 9992);
    }
}
