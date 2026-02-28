use axum::{
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::StatusCode,
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use p3_contracts::{
    DecoderEventPayloadV1, DecoderSnapshotPayloadV1, DecoderStatusRowV1, EmptyPayloadV1,
    LiveChannelV1, LiveEnvelopeKindV1, LiveEnvelopeV1, LiveErrorPayloadV1, RaceEventEnvelopeV1,
    RaceEventPayloadV1, build_race_events_subject,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::select;
use tokio::time::{self, Duration};
use tracing::{info, warn};

use super::state::AppState;
use crate::db::queries::decoder_live::{
    DecoderSnapshotRow as DbDecoderSnapshotRow, list_decoder_snapshot_rows_for_track,
};

/// WebSocket upgrade handler — each connected client receives P3 messages and race events as JSON.
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

#[derive(Debug, Deserialize)]
pub struct LiveQuery {
    track_id: Option<String>,
    event_id: Option<String>,
    channels: Option<String>,
    from: Option<String>,
}

#[derive(Default)]
struct LiveSeq {
    next: u64,
}

impl LiveSeq {
    fn next(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

/// ADR-aligned live stream endpoint backed by NATS race event subjects.
pub async fn ws_live_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<LiveQuery>,
) -> Result<Response, (StatusCode, String)> {
    let LiveQuery {
        track_id,
        event_id,
        channels,
        from,
    } = query;

    let track_id = track_id.unwrap_or_default().trim().to_string();
    if track_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "track_id query parameter is required".to_string(),
        ));
    }

    if let Some(from) = from.as_deref() {
        if from != "now" {
            return Err((
                StatusCode::BAD_REQUEST,
                "from must be 'now' for /ws/v1/live".to_string(),
            ));
        }
    }

    let selection = classify_channels(channels.as_deref());

    Ok(ws.on_upgrade(move |socket| {
        handle_live_socket(
            socket,
            state,
            track_id,
            event_id,
            selection.supported,
            selection.issues,
        )
    }))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("WebSocket client connected");

    // Send current race state snapshot to newly connected client
    {
        let engine = state.engine.lock().await;
        let snapshot = engine.state_snapshot();
        if let Ok(json) = serde_json::to_string(&snapshot) {
            let _ = socket.send(WsMessage::text(json)).await;
        }
    }

    let mut p3_rx = state.message_tx.subscribe();
    let mut race_rx = state.race_event_tx.subscribe();

    loop {
        select! {
            result = p3_rx.recv() => {
                match result {
                    Ok(message) => {
                        let json = match serde_json::to_string(message.as_ref()) {
                            Ok(j) => j,
                            Err(e) => {
                                warn!(error = %e, "Failed to serialize P3 message");
                                continue;
                            }
                        };
                        if socket.send(WsMessage::text(json)).await.is_err() {
                            info!("WebSocket client disconnected");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WebSocket client lagging on P3 messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("P3 broadcast channel closed");
                        break;
                    }
                }
            }
            result = race_rx.recv() => {
                match result {
                    Ok(event) => {
                        let json = match serde_json::to_string(event.as_ref()) {
                            Ok(j) => j,
                            Err(e) => {
                                warn!(error = %e, "Failed to serialize race event");
                                continue;
                            }
                        };
                        if socket.send(WsMessage::text(json)).await.is_err() {
                            info!("WebSocket client disconnected");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WebSocket client lagging on race events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Race event broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_live_socket(
    socket: WebSocket,
    state: AppState,
    track_id: String,
    requested_event_id: Option<String>,
    channels: BTreeSet<LiveChannelV1>,
    channel_issues: Vec<ChannelIssue>,
) {
    info!(track_id = %track_id, "WebSocket /ws/v1/live client connected");

    let stream_decoder_channel = channels.contains(&LiveChannelV1::Decoder);

    let mut nats_sub = if stream_decoder_channel {
        let nats_client = match async_nats::connect(&state.nats_url).await {
            Ok(client) => client,
            Err(error) => {
                warn!(error = %error, "Failed to connect to NATS for live socket");
                return;
            }
        };

        let subject = build_race_events_subject(&track_id);
        match nats_client.subscribe(subject.clone()).await {
            Ok(sub) => Some(sub),
            Err(error) => {
                warn!(error = %error, subject = %subject, "Failed to subscribe to live race events");
                return;
            }
        }
    } else {
        None
    };

    let (mut sender, mut receiver) = socket.split();
    let mut seq = LiveSeq::default();
    let mut heartbeat = time::interval(Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    for channel in &channels {
        if *channel != LiveChannelV1::Decoder {
            continue;
        }

        let snapshot_rows = match list_decoder_snapshot_rows_for_track(&state.db, &track_id).await {
            Ok(rows) => rows,
            Err(error) => {
                warn!(error = %error, track_id = %track_id, "Failed to query decoder snapshot rows");
                let envelope = LiveEnvelopeV1 {
                    kind: LiveEnvelopeKindV1::Error,
                    channel: *channel,
                    track_id: track_id.clone(),
                    event_id: requested_event_id.clone(),
                    seq: seq.next(),
                    ts_us: now_unix_micros(),
                    payload: LiveErrorPayloadV1 {
                        code: "snapshot_query_failed".to_string(),
                        message: "Failed to load decoder snapshot".to_string(),
                        channel: Some("decoder".to_string()),
                    },
                };

                if send_live_envelope(&mut sender, &envelope).await.is_err() {
                    return;
                }
                continue;
            }
        };

        let envelope = LiveEnvelopeV1 {
            kind: LiveEnvelopeKindV1::Snapshot,
            channel: *channel,
            track_id: track_id.clone(),
            event_id: requested_event_id.clone(),
            seq: seq.next(),
            ts_us: now_unix_micros(),
            payload: map_decoder_snapshot_rows(snapshot_rows),
        };
        if send_live_envelope(&mut sender, &envelope).await.is_err() {
            return;
        }
    }

    for issue in channel_issues {
        let envelope = LiveEnvelopeV1 {
            kind: LiveEnvelopeKindV1::Error,
            channel: issue.envelope_channel,
            track_id: track_id.clone(),
            event_id: requested_event_id.clone(),
            seq: seq.next(),
            ts_us: now_unix_micros(),
            payload: LiveErrorPayloadV1 {
                code: issue.code.to_string(),
                message: issue.message,
                channel: Some(issue.requested_channel),
            },
        };
        if send_live_envelope(&mut sender, &envelope).await.is_err() {
            return;
        }
    }

    loop {
        select! {
            nats_message = async {
                if let Some(sub) = &mut nats_sub {
                    sub.next().await
                } else {
                    None
                }
            }, if stream_decoder_channel => {
                let Some(message) = nats_message else {
                    break;
                };

                let derived: RaceEventEnvelopeV1 = match serde_json::from_slice(&message.payload) {
                    Ok(derived) => derived,
                    Err(error) => {
                        warn!(error = %error, "Failed to parse race event envelope from NATS");
                        continue;
                    }
                };

                if let Some(payload) = map_decoder_event_payload(&derived) {
                    let envelope = LiveEnvelopeV1 {
                        kind: LiveEnvelopeKindV1::Event,
                        channel: LiveChannelV1::Decoder,
                        track_id: track_id.clone(),
                        event_id: Some(derived.event_id.to_string()),
                        seq: seq.next(),
                        ts_us: derived.ts_us,
                        payload,
                    };

                    if send_live_envelope(&mut sender, &envelope).await.is_err() {
                        break;
                    }
                }
            }
            _ = heartbeat.tick() => {
                for channel in &channels {
                    let envelope = LiveEnvelopeV1 {
                        kind: LiveEnvelopeKindV1::Heartbeat,
                        channel: *channel,
                        track_id: track_id.clone(),
                        event_id: requested_event_id.clone(),
                        seq: seq.next(),
                        ts_us: now_unix_micros(),
                        payload: EmptyPayloadV1 {},
                    };

                    if send_live_envelope(&mut sender, &envelope).await.is_err() {
                        return;
                    }
                }
            }
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        warn!(error = %error, "Live WebSocket receive error");
                        break;
                    }
                }
            }
        }
    }

    info!(track_id = %track_id, "WebSocket /ws/v1/live client disconnected");
}

#[derive(Debug)]
struct ChannelIssue {
    requested_channel: String,
    envelope_channel: LiveChannelV1,
    code: &'static str,
    message: String,
}

#[derive(Debug)]
struct ChannelSelection {
    supported: BTreeSet<LiveChannelV1>,
    issues: Vec<ChannelIssue>,
}

fn classify_channels(raw: Option<&str>) -> ChannelSelection {
    let mut supported = BTreeSet::new();
    let mut issues = Vec::new();

    let channels = raw.unwrap_or("decoder");
    let is_defaulted = raw.is_none() || channels.trim().is_empty();

    for candidate in channels
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        match candidate {
            "decoder" => {
                supported.insert(LiveChannelV1::Decoder);
            }
            "race" => issues.push(ChannelIssue {
                requested_channel: "race".to_string(),
                envelope_channel: LiveChannelV1::Race,
                code: "unimplemented_channel",
                message: "Channel 'race' is recognized but not implemented yet".to_string(),
            }),
            other => issues.push(ChannelIssue {
                requested_channel: other.to_string(),
                envelope_channel: LiveChannelV1::Unknown,
                code: "unsupported_channel",
                message: format!("Channel '{other}' is not supported"),
            }),
        }
    }

    if is_defaulted && supported.is_empty() {
        supported.insert(LiveChannelV1::Decoder);
    }

    ChannelSelection { supported, issues }
}

async fn send_live_envelope(
    sender: &mut futures_util::stream::SplitSink<WebSocket, WsMessage>,
    envelope: &impl serde::Serialize,
) -> Result<(), ()> {
    let json = match serde_json::to_string(envelope) {
        Ok(json) => json,
        Err(error) => {
            warn!(error = %error, "Failed to serialize live envelope");
            return Ok(());
        }
    };

    if sender.send(WsMessage::text(json)).await.is_err() {
        return Err(());
    }

    Ok(())
}

fn map_decoder_snapshot_rows(rows: Vec<DbDecoderSnapshotRow>) -> DecoderSnapshotPayloadV1 {
    DecoderSnapshotPayloadV1 {
        rows: rows
            .into_iter()
            .map(|row| DecoderStatusRowV1 {
                loop_id: row.loop_id,
                loop_name: row.loop_name,
                loop_position: row.loop_position,
                decoder_id: row.decoder_id,
                noise: row.noise,
                temperature: row.temperature,
                gps_status: row.gps_status,
                satellites: row.satellites,
                last_seen: row.last_seen,
            })
            .collect(),
    }
}

fn map_decoder_event_payload(derived: &RaceEventEnvelopeV1) -> Option<DecoderEventPayloadV1> {
    match &derived.payload {
        RaceEventPayloadV1::DecoderMessage { message } => Some(DecoderEventPayloadV1 {
            message: message.clone(),
            source_event_id: derived.source_event_id,
        }),
        _ => None,
    }
}

fn now_unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p3_parser::{Message, StatusMessage};
    use uuid::Uuid;

    #[test]
    fn classify_channels_defaults_to_decoder() {
        let parsed = classify_channels(None);
        assert_eq!(parsed.supported, BTreeSet::from([LiveChannelV1::Decoder]));
        assert!(parsed.issues.is_empty());

        let parsed_empty = classify_channels(Some("   "));
        assert_eq!(
            parsed_empty.supported,
            BTreeSet::from([LiveChannelV1::Decoder])
        );
        assert!(parsed_empty.issues.is_empty());
    }

    #[test]
    fn classify_channels_tracks_unsupported_and_unimplemented() {
        let parsed = classify_channels(Some("decoder,race,invalid"));
        assert_eq!(parsed.supported, BTreeSet::from([LiveChannelV1::Decoder]));
        assert_eq!(parsed.issues.len(), 2);

        assert_eq!(parsed.issues[0].requested_channel, "race");
        assert_eq!(parsed.issues[0].envelope_channel, LiveChannelV1::Race);
        assert_eq!(parsed.issues[0].code, "unimplemented_channel");

        assert_eq!(parsed.issues[1].requested_channel, "invalid");
        assert_eq!(parsed.issues[1].envelope_channel, LiveChannelV1::Unknown);
        assert_eq!(parsed.issues[1].code, "unsupported_channel");
    }

    #[test]
    fn live_seq_is_monotonic() {
        let mut seq = LiveSeq::default();
        assert_eq!(seq.next(), 1);
        assert_eq!(seq.next(), 2);
        assert_eq!(seq.next(), 3);
    }

    #[test]
    fn map_decoder_snapshot_rows_preserves_order_and_fields() {
        let payload = map_decoder_snapshot_rows(vec![
            DbDecoderSnapshotRow {
                loop_id: "loop-1".to_string(),
                loop_name: "Start".to_string(),
                loop_position: 1,
                decoder_id: "D1000C00".to_string(),
                noise: Some(20),
                temperature: Some(170),
                gps_status: Some(1),
                satellites: Some(9),
                last_seen: Some("2026-01-01T00:00:00".to_string()),
            },
            DbDecoderSnapshotRow {
                loop_id: "loop-2".to_string(),
                loop_name: "Finish".to_string(),
                loop_position: 2,
                decoder_id: "D2000C00".to_string(),
                noise: None,
                temperature: None,
                gps_status: None,
                satellites: None,
                last_seen: None,
            },
        ]);

        assert_eq!(payload.rows.len(), 2);
        assert_eq!(payload.rows[0].loop_id, "loop-1");
        assert_eq!(payload.rows[0].decoder_id, "D1000C00");
        assert_eq!(payload.rows[1].loop_id, "loop-2");
        assert!(payload.rows[1].noise.is_none());
    }

    #[test]
    fn map_decoder_event_payload_maps_decoder_message() {
        let source_event_id = Uuid::new_v4();
        let message = Message::Status(StatusMessage {
            noise: 55,
            gps_status: 1,
            temperature: 180,
            satellites: 10,
            decoder_id: Some("D1000C00".to_string()),
        });

        let derived = RaceEventEnvelopeV1 {
            event_id: Uuid::new_v4(),
            contract_version: "race_events_envelope.v1".to_string(),
            track_id: "track-1".to_string(),
            source_event_id,
            ts_us: 123,
            payload: RaceEventPayloadV1::DecoderMessage {
                message: message.clone(),
            },
        };

        let mapped = map_decoder_event_payload(&derived).expect("expected decoder payload");
        assert_eq!(mapped.message, message);
        assert_eq!(mapped.source_event_id, source_event_id);
    }
}
