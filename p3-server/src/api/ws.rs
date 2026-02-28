use ::time::OffsetDateTime;
use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, DeliverPolicy};
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
    RaceEventPayloadV1, RaceSnapshotEnvelopeV1, build_race_events_subject,
};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::select;
use tokio::time::{self, Duration};
use tracing::{info, warn};

use super::metrics::AppMetrics;
use super::state::AppState;
use crate::api::auth::TrackAuthError;
use crate::db::queries::decoder_live::{
    DecoderSnapshotRow as DbDecoderSnapshotRow, list_decoder_snapshot_rows_for_track,
};
use crate::db::queries::race_projection::{
    ProjectedRaceState as DbProjectedRaceState, get_race_state_projection,
};
use crate::ingest::publisher::{RACE_EVENTS_STREAM_NAME, RACE_SNAPSHOT_STREAM_NAME};

#[derive(Debug, Deserialize)]
pub struct LiveQuery {
    track_id: Option<String>,
    event_id: Option<String>,
    channels: Option<String>,
    from: Option<String>,
    auth_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveFromMarker {
    Now,
    BySequence(u64),
    ByTimestampUs(u64),
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
        auth_token,
    } = query;

    let track_id = track_id.unwrap_or_default().trim().to_string();
    if track_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "track_id query parameter is required".to_string(),
        ));
    }

    let from_marker = parse_live_from_marker(from.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;

    authorize_live_request(&state, &track_id, auth_token.as_deref())?;

    let selection = classify_channels(channels.as_deref());

    Ok(ws.on_upgrade(move |socket| {
        handle_live_socket(
            socket,
            state,
            track_id,
            event_id,
            from_marker,
            selection.supported,
            selection.issues,
        )
    }))
}

fn authorize_live_request(
    state: &AppState,
    track_id: &str,
    auth_token: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    state
        .track_auth
        .authorize_track_token(track_id, auth_token)
        .map_err(|error| match error {
            TrackAuthError::MissingToken => (
                StatusCode::UNAUTHORIZED,
                "auth_token query parameter is required when track auth is enabled".to_string(),
            ),
            TrackAuthError::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "invalid track auth token".to_string(),
            ),
            TrackAuthError::ForbiddenTrack => (
                StatusCode::FORBIDDEN,
                "auth token is not authorized for this track_id".to_string(),
            ),
        })
}

async fn handle_live_socket(
    socket: WebSocket,
    state: AppState,
    track_id: String,
    requested_event_id: Option<String>,
    from_marker: LiveFromMarker,
    channels: BTreeSet<LiveChannelV1>,
    channel_issues: Vec<ChannelIssue>,
) {
    state.metrics.inc_ws_connections();
    info!(track_id = %track_id, "WebSocket /ws/v1/live client connected");

    let stream_decoder_channel = channels.contains(&LiveChannelV1::Decoder);
    let stream_race_channel = channels.contains(&LiveChannelV1::Race);

    let (mut sender, mut receiver) = socket.split();
    let mut seq = LiveSeq::default();
    let mut heartbeat = time::interval(Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let mut race_messages = if stream_decoder_channel || stream_race_channel {
        let nats_client = match async_nats::connect(&state.nats_url).await {
            Ok(client) => client,
            Err(error) => {
                warn!(error = %error, "Failed to connect to NATS for live socket");
                let _ = send_live_error(
                    &mut sender,
                    &state.metrics,
                    &track_id,
                    requested_event_id.clone(),
                    LiveChannelV1::Race,
                    "race_events_unavailable",
                    "Failed to connect to event stream",
                    Some("race"),
                    &mut seq,
                )
                .await;
                return;
            }
        };

        let jetstream = jetstream::new(nats_client);
        let consumer = match create_live_race_consumer(&jetstream, &track_id, from_marker).await {
            Ok(consumer) => consumer,
            Err(error) => {
                warn!(error = %error, track_id = %track_id, "Failed to create live race consumer");
                let _ = send_live_error(
                    &mut sender,
                    &state.metrics,
                    &track_id,
                    requested_event_id.clone(),
                    LiveChannelV1::Race,
                    "race_events_unavailable",
                    "Failed to create event stream consumer",
                    Some("race"),
                    &mut seq,
                )
                .await;
                return;
            }
        };

        if stream_race_channel {
            let snapshot_payload = load_race_snapshot_bootstrap(
                &state,
                &jetstream,
                &track_id,
                requested_event_id.as_deref(),
            )
            .await;

            let envelope = LiveEnvelopeV1 {
                kind: LiveEnvelopeKindV1::Snapshot,
                channel: LiveChannelV1::Race,
                track_id: track_id.clone(),
                event_id: requested_event_id.clone(),
                seq: seq.next(),
                ts_us: now_unix_micros(),
                payload: snapshot_payload,
            };
            if send_live_envelope(&mut sender, &state.metrics, &envelope)
                .await
                .is_err()
            {
                return;
            }
        }

        match consumer.messages().await {
            Ok(messages) => Some(messages),
            Err(error) => {
                warn!(error = %error, track_id = %track_id, "Failed to open live race consumer messages");
                let _ = send_live_error(
                    &mut sender,
                    &state.metrics,
                    &track_id,
                    requested_event_id.clone(),
                    LiveChannelV1::Race,
                    "race_events_unavailable",
                    "Failed to consume event stream",
                    Some("race"),
                    &mut seq,
                )
                .await;
                return;
            }
        }
    } else {
        None
    };

    let mut moto_event_cache: HashMap<String, Option<String>> = HashMap::new();

    for channel in &channels {
        match *channel {
            LiveChannelV1::Decoder => {
                let snapshot_rows =
                    match list_decoder_snapshot_rows_for_track(&state.db, &track_id).await {
                        Ok(rows) => rows,
                        Err(error) => {
                            warn!(
                                error = %error,
                                track_id = %track_id,
                                "Failed to query decoder snapshot rows"
                            );
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

                            if send_live_envelope(&mut sender, &state.metrics, &envelope)
                                .await
                                .is_err()
                            {
                                return;
                            }
                            state.metrics.inc_ws_errors();
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
                if send_live_envelope(&mut sender, &state.metrics, &envelope)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            LiveChannelV1::Race => {
                // Race snapshot already sent during stream bootstrap.
            }
            LiveChannelV1::Unknown => {}
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
        if send_live_envelope(&mut sender, &state.metrics, &envelope)
            .await
            .is_err()
        {
            return;
        }
        state.metrics.inc_ws_errors();
    }

    loop {
        select! {
            nats_message = async {
                if let Some(messages) = &mut race_messages {
                    messages.next().await
                } else {
                    None
                }
            }, if stream_decoder_channel || stream_race_channel => {
                let Some(message) = nats_message else {
                    break;
                };

                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        warn!(error = %error, "Failed to receive race event from consumer");
                        continue;
                    }
                };

                let derived: RaceEventEnvelopeV1 = match serde_json::from_slice(&message.payload) {
                    Ok(derived) => derived,
                    Err(error) => {
                        warn!(error = %error, "Failed to parse race event envelope from NATS");
                        continue;
                    }
                };

                if stream_decoder_channel && let Some(payload) = map_decoder_event_payload(&derived) {
                    let envelope = LiveEnvelopeV1 {
                        kind: LiveEnvelopeKindV1::Event,
                        channel: LiveChannelV1::Decoder,
                        track_id: track_id.clone(),
                        event_id: Some(derived.event_id.to_string()),
                        seq: seq.next(),
                        ts_us: derived.ts_us,
                        payload,
                    };

                    if send_live_envelope(&mut sender, &state.metrics, &envelope)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }

                if stream_race_channel && let Some(payload) = map_race_event_payload(&derived) {
                    if !should_forward_race_payload(
                        &state,
                        &payload,
                        requested_event_id.as_deref(),
                        &mut moto_event_cache,
                    )
                    .await
                    {
                        continue;
                    }

                    let envelope = LiveEnvelopeV1 {
                        kind: LiveEnvelopeKindV1::Event,
                        channel: LiveChannelV1::Race,
                        track_id: track_id.clone(),
                        event_id: Some(derived.event_id.to_string()),
                        seq: seq.next(),
                        ts_us: derived.ts_us,
                        payload,
                    };

                    if send_live_envelope(&mut sender, &state.metrics, &envelope)
                        .await
                        .is_err()
                    {
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

                    if send_live_envelope(&mut sender, &state.metrics, &envelope)
                        .await
                        .is_err()
                    {
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

fn parse_live_from_marker(raw: Option<&str>) -> Result<LiveFromMarker, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(LiveFromMarker::Now);
    };

    if raw == "now" {
        return Ok(LiveFromMarker::Now);
    }

    if let Some(seq) = raw.strip_prefix("seq:") {
        let sequence = seq
            .parse::<u64>()
            .map_err(|_| "from must be now, seq:<u64>, or ts_us:<u64>".to_string())?;
        if sequence == 0 {
            return Err("from seq must be >= 1".to_string());
        }
        return Ok(LiveFromMarker::BySequence(sequence));
    }

    if let Some(ts_us) = raw.strip_prefix("ts_us:") {
        let timestamp_us = ts_us
            .parse::<u64>()
            .map_err(|_| "from must be now, seq:<u64>, or ts_us:<u64>".to_string())?;
        if unix_micros_to_offset_datetime(timestamp_us).is_none() {
            return Err("from ts_us is out of range".to_string());
        }
        return Ok(LiveFromMarker::ByTimestampUs(timestamp_us));
    }

    Err("from must be now, seq:<u64>, or ts_us:<u64>".to_string())
}

async fn create_live_race_consumer(
    jetstream: &jetstream::Context,
    track_id: &str,
    from_marker: LiveFromMarker,
) -> anyhow::Result<jetstream::consumer::Consumer<jetstream::consumer::pull::Config>> {
    let stream = jetstream.get_stream(RACE_EVENTS_STREAM_NAME).await?;
    let filter_subject = build_race_events_subject(track_id);
    let deliver_policy = deliver_policy_from_marker(from_marker)?;
    let config = jetstream::consumer::pull::Config {
        filter_subject,
        ack_policy: AckPolicy::None,
        deliver_policy,
        ..Default::default()
    };

    let consumer = stream.create_consumer(config).await?;
    Ok(consumer)
}

fn deliver_policy_from_marker(from_marker: LiveFromMarker) -> anyhow::Result<DeliverPolicy> {
    match from_marker {
        LiveFromMarker::Now => Ok(DeliverPolicy::New),
        LiveFromMarker::BySequence(start_sequence) => {
            Ok(DeliverPolicy::ByStartSequence { start_sequence })
        }
        LiveFromMarker::ByTimestampUs(timestamp_us) => {
            let start_time = unix_micros_to_offset_datetime(timestamp_us)
                .ok_or_else(|| anyhow::anyhow!("from ts_us is out of range"))?;
            Ok(DeliverPolicy::ByStartTime { start_time })
        }
    }
}

async fn load_race_snapshot_bootstrap(
    state: &AppState,
    jetstream: &jetstream::Context,
    track_id: &str,
    requested_event_id: Option<&str>,
) -> RaceEventPayloadV1 {
    if let Some(snapshot_payload) =
        try_load_snapshot_payload_from_stream(state, jetstream, track_id, requested_event_id).await
    {
        return snapshot_payload;
    }

    match get_race_state_projection(&state.db, track_id).await {
        Ok(Some(projected)) => map_race_snapshot_payload(projected),
        Ok(None) => idle_race_snapshot_payload(),
        Err(error) => {
            warn!(
                error = %error,
                track_id = %track_id,
                "Failed to query race snapshot projection"
            );
            idle_race_snapshot_payload()
        }
    }
}

async fn try_load_snapshot_payload_from_stream(
    state: &AppState,
    jetstream: &jetstream::Context,
    track_id: &str,
    requested_event_id: Option<&str>,
) -> Option<RaceEventPayloadV1> {
    let stream = match jetstream.get_stream(RACE_SNAPSHOT_STREAM_NAME).await {
        Ok(stream) => stream,
        Err(error) => {
            warn!(
                error = %error,
                track_id = %track_id,
                "Snapshot stream unavailable, falling back to projection"
            );
            return None;
        }
    };

    if let Some(event_id) = requested_event_id
        && let Ok(Some(moto_id)) = resolve_latest_moto_id_for_event(&state.db, track_id, event_id).await
    {
        let subject = format!("timing.race.snapshot.v1.{track_id}.{moto_id}");
        if let Some(snapshot) = fetch_snapshot_by_subject(&stream, &subject).await {
            return Some(snapshot);
        }
    }

    let latest = fetch_latest_track_snapshot(&stream, track_id).await?;

    if let Some(event_id) = requested_event_id {
        let moto_id = race_payload_moto_id(&latest)?;
        let resolved_event_id = resolve_moto_event_id(&state.db, moto_id).await.ok().flatten()?;
        if resolved_event_id != event_id {
            return None;
        }
    }

    Some(latest)
}

async fn fetch_snapshot_by_subject(
    stream: &jetstream::stream::Stream,
    subject: &str,
) -> Option<RaceEventPayloadV1> {
    let config = jetstream::consumer::pull::Config {
        filter_subject: subject.to_string(),
        ack_policy: AckPolicy::None,
        deliver_policy: DeliverPolicy::Last,
        ..Default::default()
    };

    let consumer = match stream.create_consumer(config).await {
        Ok(consumer) => consumer,
        Err(error) => {
            warn!(error = %error, subject = %subject, "Failed to create snapshot lookup consumer");
            return None;
        }
    };

    let mut messages = match consumer.messages().await {
        Ok(messages) => messages,
        Err(error) => {
            warn!(error = %error, subject = %subject, "Failed to read snapshot lookup messages");
            return None;
        }
    };

    let next = match time::timeout(Duration::from_millis(250), messages.next()).await {
        Ok(next) => next,
        Err(_) => return None,
    };

    let message = match next {
        Some(Ok(message)) => message,
        Some(Err(error)) => {
            warn!(error = %error, subject = %subject, "Snapshot lookup receive error");
            return None;
        }
        None => return None,
    };

    parse_snapshot_payload(&message.payload)
}

async fn fetch_latest_track_snapshot(
    stream: &jetstream::stream::Stream,
    track_id: &str,
) -> Option<RaceEventPayloadV1> {
    let subject = format!("timing.race.snapshot.v1.{track_id}.*");
    let config = jetstream::consumer::pull::Config {
        filter_subject: subject,
        ack_policy: AckPolicy::None,
        deliver_policy: DeliverPolicy::LastPerSubject,
        ..Default::default()
    };

    let consumer = match stream.create_consumer(config).await {
        Ok(consumer) => consumer,
        Err(error) => {
            warn!(error = %error, track_id = %track_id, "Failed to create latest snapshot consumer");
            return None;
        }
    };

    let mut messages = match consumer.messages().await {
        Ok(messages) => messages,
        Err(error) => {
            warn!(error = %error, track_id = %track_id, "Failed to read latest snapshot messages");
            return None;
        }
    };

    let mut latest: Option<RaceSnapshotEnvelopeV1> = None;
    loop {
        let next = match time::timeout(Duration::from_millis(100), messages.next()).await {
            Ok(next) => next,
            Err(_) => break,
        };

        let Some(next) = next else {
            break;
        };

        let message = match next {
            Ok(message) => message,
            Err(error) => {
                warn!(error = %error, track_id = %track_id, "Snapshot receive error");
                break;
            }
        };

        let envelope = match serde_json::from_slice::<RaceSnapshotEnvelopeV1>(&message.payload) {
            Ok(envelope) => envelope,
            Err(error) => {
                warn!(error = %error, track_id = %track_id, "Failed to decode race snapshot envelope");
                continue;
            }
        };

        let replace = latest
            .as_ref()
            .is_none_or(|current| envelope.ts_us >= current.ts_us);
        if replace {
            latest = Some(envelope);
        }
    }

    latest.map(|envelope| envelope.payload)
}

fn parse_snapshot_payload(raw: &[u8]) -> Option<RaceEventPayloadV1> {
    let envelope = serde_json::from_slice::<RaceSnapshotEnvelopeV1>(raw).ok()?;
    Some(envelope.payload)
}

async fn send_live_error(
    sender: &mut futures_util::stream::SplitSink<WebSocket, WsMessage>,
    metrics: &std::sync::Arc<AppMetrics>,
    track_id: &str,
    requested_event_id: Option<String>,
    channel: LiveChannelV1,
    code: &str,
    message: &str,
    channel_name: Option<&str>,
    seq: &mut LiveSeq,
) -> Result<(), ()> {
    let envelope = LiveEnvelopeV1 {
        kind: LiveEnvelopeKindV1::Error,
        channel,
        track_id: track_id.to_string(),
        event_id: requested_event_id,
        seq: seq.next(),
        ts_us: now_unix_micros(),
        payload: LiveErrorPayloadV1 {
            code: code.to_string(),
            message: message.to_string(),
            channel: channel_name.map(ToString::to_string),
        },
    };

    let sent = send_live_envelope(sender, metrics, &envelope).await;
    if sent.is_ok() {
        metrics.inc_ws_errors();
    }
    sent
}

async fn should_forward_race_payload(
    state: &AppState,
    payload: &RaceEventPayloadV1,
    requested_event_id: Option<&str>,
    moto_event_cache: &mut HashMap<String, Option<String>>,
) -> bool {
    let Some(requested_event_id) = requested_event_id else {
        return true;
    };

    let payload_moto_id = race_payload_moto_id(payload);
    let Some(payload_moto_id) = payload_moto_id else {
        return false;
    };

    let moto_event_id = if let Some(cached) = moto_event_cache.get(payload_moto_id) {
        cached.clone()
    } else {
        let resolved = match resolve_moto_event_id(&state.db, payload_moto_id).await {
            Ok(event_id) => event_id,
            Err(error) => {
                warn!(
                    error = %error,
                    moto_id = %payload_moto_id,
                    "Failed to resolve moto scope for race payload"
                );
                return false;
            }
        };
        moto_event_cache.insert(payload_moto_id.to_string(), resolved.clone());
        resolved
    };

    should_forward_scoped_payload(
        Some(requested_event_id),
        Some(payload_moto_id),
        moto_event_id.as_deref(),
    )
}

fn should_forward_scoped_payload(
    requested_event_id: Option<&str>,
    payload_moto_id: Option<&str>,
    moto_event_id: Option<&str>,
) -> bool {
    match requested_event_id {
        None => true,
        Some(requested_event_id) => {
            if payload_moto_id.is_none() {
                return false;
            }
            moto_event_id == Some(requested_event_id)
        }
    }
}

fn race_payload_moto_id(payload: &RaceEventPayloadV1) -> Option<&str> {
    match payload {
        RaceEventPayloadV1::RaceStaged { moto_id, .. }
        | RaceEventPayloadV1::GateDrop { moto_id, .. }
        | RaceEventPayloadV1::SplitTime { moto_id, .. }
        | RaceEventPayloadV1::PositionsUpdate { moto_id, .. }
        | RaceEventPayloadV1::RiderFinished { moto_id, .. }
        | RaceEventPayloadV1::RaceFinished { moto_id, .. } => Some(moto_id),
        RaceEventPayloadV1::StateSnapshot {
            moto_id: Some(moto_id),
            ..
        } => Some(moto_id),
        RaceEventPayloadV1::DecoderMessage { .. }
        | RaceEventPayloadV1::RaceReset
        | RaceEventPayloadV1::StateSnapshot { moto_id: None, .. } => None,
    }
}

async fn resolve_moto_event_id(
    db: &sqlx::SqlitePool,
    moto_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT event_id FROM motos WHERE id = ?")
        .bind(moto_id)
        .fetch_optional(db)
        .await
}

async fn resolve_latest_moto_id_for_event(
    db: &sqlx::SqlitePool,
    track_id: &str,
    event_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT motos.id \
         FROM motos \
         JOIN events ON events.id = motos.event_id \
         WHERE motos.event_id = ? AND events.track_id = ? \
         ORDER BY motos.sequence DESC, motos.created_at DESC \
         LIMIT 1",
    )
    .bind(event_id)
    .bind(track_id)
    .fetch_optional(db)
    .await
}

fn unix_micros_to_offset_datetime(timestamp_us: u64) -> Option<OffsetDateTime> {
    let nanos = i128::from(timestamp_us).checked_mul(1_000)?;
    OffsetDateTime::from_unix_timestamp_nanos(nanos).ok()
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

    let channels = raw.unwrap_or("race");
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
            "race" => {
                supported.insert(LiveChannelV1::Race);
            }
            other => issues.push(ChannelIssue {
                requested_channel: other.to_string(),
                envelope_channel: LiveChannelV1::Unknown,
                code: "unsupported_channel",
                message: format!("Channel '{other}' is not supported"),
            }),
        }
    }

    if is_defaulted && supported.is_empty() {
        supported.insert(LiveChannelV1::Race);
    }

    ChannelSelection { supported, issues }
}

async fn send_live_envelope(
    sender: &mut futures_util::stream::SplitSink<WebSocket, WsMessage>,
    metrics: &std::sync::Arc<AppMetrics>,
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

    metrics.inc_ws_messages_sent();

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

fn map_race_event_payload(derived: &RaceEventEnvelopeV1) -> Option<RaceEventPayloadV1> {
    match &derived.payload {
        RaceEventPayloadV1::DecoderMessage { .. } => None,
        payload => Some(payload.clone()),
    }
}

fn map_race_snapshot_payload(projected: DbProjectedRaceState) -> RaceEventPayloadV1 {
    RaceEventPayloadV1::StateSnapshot {
        phase: projected.phase,
        moto_id: projected.moto_id,
        class_name: projected.class_name,
        round_type: projected.round_type,
        riders: projected.riders,
        positions: projected.positions,
        gate_drop_time_us: projected.gate_drop_time_us,
        finished_count: projected.finished_count,
        total_riders: projected.total_riders,
    }
}

fn idle_race_snapshot_payload() -> RaceEventPayloadV1 {
    RaceEventPayloadV1::StateSnapshot {
        phase: "idle".to_string(),
        moto_id: None,
        class_name: None,
        round_type: None,
        riders: Vec::new(),
        positions: Vec::new(),
        gate_drop_time_us: None,
        finished_count: 0,
        total_riders: 0,
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
    use crate::api::auth::TrackAuthConfig;
    use crate::api::metrics::AppMetrics;
    use p3_parser::{Message, StatusMessage};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::broadcast;
    use uuid::Uuid;

    async fn test_state(auth: TrackAuthConfig) -> AppState {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::run_migrations(&db).await.unwrap();
        let (message_tx, _) = broadcast::channel::<Arc<Message>>(16);

        AppState::new(
            message_tx,
            db,
            None,
            "nats://127.0.0.1:4222".to_string(),
            false,
            auth,
            Arc::new(AppMetrics::new()),
        )
    }

    fn auth_config_enabled() -> TrackAuthConfig {
        let mut tokens = HashMap::new();
        tokens.insert("track-a".to_string(), "token-a".to_string());
        tokens.insert("track-b".to_string(), "token-b".to_string());
        TrackAuthConfig::new(true, tokens)
    }

    #[test]
    fn classify_channels_defaults_to_race() {
        let parsed = classify_channels(None);
        assert_eq!(parsed.supported, BTreeSet::from([LiveChannelV1::Race]));
        assert!(parsed.issues.is_empty());

        let parsed_empty = classify_channels(Some("   "));
        assert_eq!(
            parsed_empty.supported,
            BTreeSet::from([LiveChannelV1::Race])
        );
        assert!(parsed_empty.issues.is_empty());
    }

    #[test]
    fn classify_channels_tracks_supported_and_unsupported() {
        let parsed = classify_channels(Some("decoder,race,invalid"));
        assert_eq!(
            parsed.supported,
            BTreeSet::from([LiveChannelV1::Decoder, LiveChannelV1::Race])
        );
        assert_eq!(parsed.issues.len(), 1);

        assert_eq!(parsed.issues[0].requested_channel, "invalid");
        assert_eq!(parsed.issues[0].envelope_channel, LiveChannelV1::Unknown);
        assert_eq!(parsed.issues[0].code, "unsupported_channel");
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

    #[test]
    fn parse_live_from_marker_defaults_to_now() {
        assert_eq!(parse_live_from_marker(None).unwrap(), LiveFromMarker::Now);
        assert_eq!(
            parse_live_from_marker(Some("   ")).unwrap(),
            LiveFromMarker::Now
        );
    }

    #[test]
    fn parse_live_from_marker_accepts_seq_and_timestamp() {
        assert_eq!(
            parse_live_from_marker(Some("seq:42")).unwrap(),
            LiveFromMarker::BySequence(42)
        );
        assert_eq!(
            parse_live_from_marker(Some("ts_us:123")).unwrap(),
            LiveFromMarker::ByTimestampUs(123)
        );
    }

    #[test]
    fn parse_live_from_marker_rejects_invalid_values() {
        assert!(parse_live_from_marker(Some("later")).is_err());
        assert!(parse_live_from_marker(Some("seq:0")).is_err());
        assert!(parse_live_from_marker(Some("seq:abc")).is_err());
        assert!(parse_live_from_marker(Some("ts_us:abc")).is_err());
    }

    #[test]
    fn race_payload_moto_id_extracts_expected_scope() {
        let staged = RaceEventPayloadV1::RaceStaged {
            moto_id: "moto-1".to_string(),
            class_name: "Expert".to_string(),
            round_type: "main".to_string(),
            riders: Vec::new(),
        };
        assert_eq!(race_payload_moto_id(&staged), Some("moto-1"));

        let snapshot = RaceEventPayloadV1::StateSnapshot {
            phase: "staged".to_string(),
            moto_id: Some("moto-2".to_string()),
            class_name: None,
            round_type: None,
            riders: Vec::new(),
            positions: Vec::new(),
            gate_drop_time_us: None,
            finished_count: 0,
            total_riders: 0,
        };
        assert_eq!(race_payload_moto_id(&snapshot), Some("moto-2"));

        let idle_snapshot = RaceEventPayloadV1::StateSnapshot {
            phase: "idle".to_string(),
            moto_id: None,
            class_name: None,
            round_type: None,
            riders: Vec::new(),
            positions: Vec::new(),
            gate_drop_time_us: None,
            finished_count: 0,
            total_riders: 0,
        };
        assert_eq!(race_payload_moto_id(&idle_snapshot), None);
    }

    #[test]
    fn should_forward_scoped_payload_applies_event_scope() {
        assert!(should_forward_scoped_payload(None, None, None));
        assert!(should_forward_scoped_payload(
            Some("event-a"),
            Some("moto-1"),
            Some("event-a")
        ));
        assert!(!should_forward_scoped_payload(
            Some("event-a"),
            Some("moto-1"),
            Some("event-b")
        ));
        assert!(!should_forward_scoped_payload(
            Some("event-a"),
            None,
            Some("event-a")
        ));
    }

    #[tokio::test]
    async fn authorize_live_request_requires_token_when_enabled() {
        let state = test_state(auth_config_enabled()).await;
        let err = authorize_live_request(&state, "track-a", None).expect_err("should fail");
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authorize_live_request_rejects_token_for_other_track() {
        let state = test_state(auth_config_enabled()).await;
        let err =
            authorize_live_request(&state, "track-a", Some("token-b")).expect_err("should fail");
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn authorize_live_request_accepts_matching_token() {
        let state = test_state(auth_config_enabled()).await;
        authorize_live_request(&state, "track-a", Some("token-a")).expect("should pass");
    }
}
