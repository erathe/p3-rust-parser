use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use async_nats::HeaderMap;
use async_nats::error::Error as NatsError;
use async_nats::jetstream;
use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::AckPolicy;
use async_nats::jetstream::consumer::pull::MessagesErrorKind;
use futures_util::StreamExt;
use p3_contracts::{
    DLQ_ENVELOPE_CONTRACT_VERSION_V1, DlqEnvelopeV1, FinishResultV1, LoopConfigV1,
    RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1, RACE_SNAPSHOT_ENVELOPE_CONTRACT_VERSION_V1,
    RaceControlIntentEnvelopeV1, RaceControlIntentV1, RaceEventEnvelopeV1, RaceEventPayloadV1,
    RaceSnapshotEnvelopeV1, RawIngestEnvelopeV1, RiderPositionV1, StagedRiderV1, TrackConfigV1,
    build_idempotency_key, build_race_events_subject, build_race_snapshot_subject,
};
use p3_parser::Message;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::queries::race_worker_dedupe::{self, ClaimOutcome, DedupeSource};
use crate::domain::race_event::{
    FinishResult, LoopConfig, RaceEvent, RiderPosition, StagedRider, TrackConfig,
};
use crate::engine::{RaceEngine, RacePhase};
use crate::ingest::publisher::{
    RACE_CONTROL_STREAM_NAME, RACE_CONTROL_SUBJECT_PATTERN, RAW_INGEST_STREAM_NAME,
    RAW_INGEST_SUBJECT_PATTERN, connect_jetstream_and_provision_raw_race_events_and_race_control,
    publish_dlq_envelope,
};

const RACE_WORKER_RAW_CONSUMER: &str = "race_worker_raw_v1";
const RACE_WORKER_CONTROL_CONSUMER: &str = "race_worker_control_v1";
const CONSUMER_ACK_WAIT_SECS: u64 = 30;
const CONSUMER_MAX_DELIVER: i64 = 10;
const RETRY_DELAY_SECS: u64 = 2;
const DLQ_SOURCE_RAW: &str = "race_worker_raw";
const DLQ_SOURCE_CONTROL: &str = "race_worker_control";

enum TrackActorPayload {
    Raw(RawIngestEnvelopeV1),
    Control(RaceControlIntentEnvelopeV1),
}

struct TrackActorInput {
    payload: TrackActorPayload,
    result_tx: oneshot::Sender<anyhow::Result<()>>,
}

enum ActorDispatchOutcome {
    Processed,
    Failed(anyhow::Error),
}

enum DedupedDispatchOutcome {
    Duplicate,
    Processed,
    Failed(anyhow::Error),
}

pub async fn run_race_worker(nats_url: &str, pool: &SqlitePool) -> anyhow::Result<()> {
    let jetstream =
        connect_jetstream_and_provision_raw_race_events_and_race_control(nats_url).await?;
    let raw_stream = jetstream.get_stream(RAW_INGEST_STREAM_NAME).await?;
    let control_stream = jetstream.get_stream(RACE_CONTROL_STREAM_NAME).await?;
    let raw_consumer = get_or_create_consumer(
        &raw_stream,
        RACE_WORKER_RAW_CONSUMER,
        RAW_INGEST_SUBJECT_PATTERN,
    )
    .await?;
    let control_consumer = get_or_create_consumer(
        &control_stream,
        RACE_WORKER_CONTROL_CONSUMER,
        RACE_CONTROL_SUBJECT_PATTERN,
    )
    .await?;
    let mut raw_messages = raw_consumer.messages().await?;
    let mut control_messages = control_consumer.messages().await?;
    let mut track_actors: HashMap<String, mpsc::Sender<TrackActorInput>> = HashMap::new();
    let mut raw_open = true;
    let mut control_open = true;

    info!(
        nats_url = %nats_url,
        raw_consumer = RACE_WORKER_RAW_CONSUMER,
        raw_subject = RAW_INGEST_SUBJECT_PATTERN,
        control_consumer = RACE_WORKER_CONTROL_CONSUMER,
        control_subject = RACE_CONTROL_SUBJECT_PATTERN,
        "Race worker started"
    );

    while raw_open || control_open {
        tokio::select! {
            raw_message_result = raw_messages.next(), if raw_open => {
                match raw_message_result {
                    Some(message_result) => {
                        handle_raw_message(pool, &jetstream, &mut track_actors, message_result).await?;
                    }
                    None => {
                        raw_open = false;
                        warn!("Raw ingest consumer stream closed");
                    }
                }
            }
            control_message_result = control_messages.next(), if control_open => {
                match control_message_result {
                    Some(message_result) => {
                        handle_control_message(pool, &jetstream, &mut track_actors, message_result).await?;
                    }
                    None => {
                        control_open = false;
                        warn!("Race control consumer stream closed");
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_raw_message(
    pool: &SqlitePool,
    jetstream: &jetstream::Context,
    track_actors: &mut HashMap<String, mpsc::Sender<TrackActorInput>>,
    message_result: Result<jetstream::Message, NatsError<MessagesErrorKind>>,
) -> anyhow::Result<()> {
    let message = match message_result {
        Ok(message) => message,
        Err(error) => {
            warn!(error = %error, "Race worker failed to receive raw message");
            return Ok(());
        }
    };

    let envelope: RawIngestEnvelopeV1 = match serde_json::from_slice(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            let failure_reason = format!("Failed to parse raw ingest envelope: {error}");
            publish_message_to_dlq(
                jetstream,
                &message,
                DLQ_SOURCE_RAW,
                None,
                None,
                failure_reason,
            )
            .await?;
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack poison raw message: {error}"))?;
            return Ok(());
        }
    };

    let dedupe_key = format!(
        "raw:{}",
        build_idempotency_key(&envelope.track_id, &envelope.event_id_context)
    );
    let event_id_for_dlq = envelope.event_id.to_string();
    let track_id = envelope.track_id.clone();

    let dispatch = dispatch_with_dedupe(pool, &dedupe_key, &track_id, DedupeSource::Raw, || {
        dispatch_to_track_actor(
            track_actors,
            track_id.clone(),
            jetstream.clone(),
            TrackActorPayload::Raw(envelope),
        )
    })
    .await?;

    match dispatch {
        DedupedDispatchOutcome::Duplicate => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack duplicate raw message: {error}"))?;
        }
        DedupedDispatchOutcome::Processed => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack processed raw message: {error}"))?;
        }
        DedupedDispatchOutcome::Failed(error) => {
            handle_processing_failure(
                &message,
                jetstream,
                DLQ_SOURCE_RAW,
                Some(event_id_for_dlq),
                Some(track_id),
                error.to_string(),
            )
            .await?;
        }
    }

    Ok(())
}

async fn handle_control_message(
    pool: &SqlitePool,
    jetstream: &jetstream::Context,
    track_actors: &mut HashMap<String, mpsc::Sender<TrackActorInput>>,
    message_result: Result<jetstream::Message, NatsError<MessagesErrorKind>>,
) -> anyhow::Result<()> {
    let message = match message_result {
        Ok(message) => message,
        Err(error) => {
            warn!(error = %error, "Race worker failed to receive control message");
            return Ok(());
        }
    };

    let envelope: RaceControlIntentEnvelopeV1 = match serde_json::from_slice(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            let failure_reason = format!("Failed to parse race control envelope: {error}");
            publish_message_to_dlq(
                jetstream,
                &message,
                DLQ_SOURCE_CONTROL,
                None,
                None,
                failure_reason,
            )
            .await?;
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack poison control message: {error}"))?;
            return Ok(());
        }
    };

    let dedupe_key = format!("control:{}:{}", envelope.track_id, envelope.event_id);
    let event_id_for_dlq = envelope.event_id.to_string();
    let track_id = envelope.track_id.clone();

    let dispatch =
        dispatch_with_dedupe(pool, &dedupe_key, &track_id, DedupeSource::Control, || {
            dispatch_to_track_actor(
                track_actors,
                track_id.clone(),
                jetstream.clone(),
                TrackActorPayload::Control(envelope),
            )
        })
        .await?;

    match dispatch {
        DedupedDispatchOutcome::Duplicate => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack duplicate control message: {error}"))?;
        }
        DedupedDispatchOutcome::Processed => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack processed control message: {error}"))?;
        }
        DedupedDispatchOutcome::Failed(error) => {
            handle_processing_failure(
                &message,
                jetstream,
                DLQ_SOURCE_CONTROL,
                Some(event_id_for_dlq),
                Some(track_id),
                error.to_string(),
            )
            .await?;
        }
    }

    Ok(())
}

async fn dispatch_with_dedupe<F, Fut>(
    pool: &SqlitePool,
    dedupe_key: &str,
    track_id: &str,
    source: DedupeSource,
    dispatch: F,
) -> anyhow::Result<DedupedDispatchOutcome>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ActorDispatchOutcome>,
{
    match race_worker_dedupe::claim(pool, dedupe_key, track_id, source)
        .await
        .map_err(|error| anyhow!("Failed to claim race-worker dedupe key: {error}"))?
    {
        ClaimOutcome::Duplicate => return Ok(DedupedDispatchOutcome::Duplicate),
        ClaimOutcome::Claimed => {}
    }

    let outcome = dispatch().await;
    match outcome {
        ActorDispatchOutcome::Processed => Ok(DedupedDispatchOutcome::Processed),
        ActorDispatchOutcome::Failed(error) => {
            if let Err(error) = race_worker_dedupe::release(pool, dedupe_key).await {
                warn!(error = %error, key = %dedupe_key, "Failed to release race-worker dedupe claim after processing failure");
            }
            Ok(DedupedDispatchOutcome::Failed(error))
        }
    }
}

async fn dispatch_to_track_actor(
    track_actors: &mut HashMap<String, mpsc::Sender<TrackActorInput>>,
    track_id: String,
    jetstream: jetstream::Context,
    payload: TrackActorPayload,
) -> ActorDispatchOutcome {
    let actor = track_actors
        .entry(track_id.clone())
        .or_insert_with(|| spawn_track_actor(track_id, jetstream))
        .clone();

    let (result_tx, result_rx) = oneshot::channel();
    if actor
        .send(TrackActorInput { payload, result_tx })
        .await
        .is_err()
    {
        return ActorDispatchOutcome::Failed(anyhow!("Race track actor unavailable"));
    }

    match result_rx.await {
        Ok(Ok(())) => ActorDispatchOutcome::Processed,
        Ok(Err(error)) => {
            ActorDispatchOutcome::Failed(anyhow!("Race actor processing failed: {error}"))
        }
        Err(error) => ActorDispatchOutcome::Failed(anyhow!("Race actor dropped response: {error}")),
    }
}

async fn get_or_create_consumer(
    stream: &jetstream::stream::Stream,
    durable_name: &str,
    filter_subject: &str,
) -> anyhow::Result<jetstream::consumer::Consumer<jetstream::consumer::pull::Config>> {
    if let Ok(consumer) = stream
        .get_consumer::<jetstream::consumer::pull::Config>(durable_name)
        .await
    {
        return Ok(consumer);
    }

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(durable_name.to_string()),
        filter_subject: filter_subject.to_string(),
        ack_policy: AckPolicy::Explicit,
        ack_wait: Duration::from_secs(CONSUMER_ACK_WAIT_SECS),
        max_deliver: CONSUMER_MAX_DELIVER,
        ..Default::default()
    };

    let consumer = stream.create_consumer(config).await?;
    Ok(consumer)
}

fn spawn_track_actor(
    track_id: String,
    jetstream: jetstream::Context,
) -> mpsc::Sender<TrackActorInput> {
    let (tx, mut rx) = mpsc::channel::<TrackActorInput>(256);

    tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel::<Arc<RaceEvent>>(64);
        let mut engine = RaceEngine::new(event_tx);

        while let Some(input) = rx.recv().await {
            let result = match input.payload {
                TrackActorPayload::Raw(envelope) => {
                    process_raw_envelope(&jetstream, &track_id, &mut engine, &envelope).await
                }
                TrackActorPayload::Control(envelope) => {
                    process_control_envelope(&jetstream, &track_id, &mut engine, &envelope).await
                }
            };
            let _ = input.result_tx.send(result);
        }
    });

    tx
}

async fn process_raw_envelope(
    jetstream: &jetstream::Context,
    track_id: &str,
    engine: &mut RaceEngine,
    raw: &RawIngestEnvelopeV1,
) -> anyhow::Result<()> {
    publish_event_payload(
        jetstream,
        track_id,
        raw.event_id,
        raw.captured_at_us,
        RaceEventPayloadV1::DecoderMessage {
            message: raw.payload.clone(),
        },
        format!("{track_id}:{}:decoder_message", raw.event_id),
    )
    .await?;

    if let Message::Passing(passing) = &raw.payload {
        let events = engine.process_passing(passing);

        for (index, event) in events.into_iter().enumerate() {
            let Some(payload) = map_domain_event_to_payload(event) else {
                continue;
            };

            let msg_id = format!("{track_id}:{}:passing:{}", raw.event_id, index);
            publish_event_payload(
                jetstream,
                track_id,
                raw.event_id,
                raw.captured_at_us,
                payload,
                msg_id,
            )
            .await?;
        }

        if let Some(snapshot_payload) = map_domain_event_to_payload(engine.state_snapshot()) {
            publish_snapshot_payload(
                jetstream,
                track_id,
                raw.event_id,
                raw.captured_at_us,
                snapshot_payload,
                format!("{track_id}:{}:snapshot", raw.event_id),
            )
            .await?;
        }
    }

    Ok(())
}

async fn process_control_envelope(
    jetstream: &jetstream::Context,
    track_id: &str,
    engine: &mut RaceEngine,
    control: &RaceControlIntentEnvelopeV1,
) -> anyhow::Result<()> {
    let mut index = 0usize;

    match &control.intent {
        RaceControlIntentV1::Stage {
            track_config,
            moto_id,
            class_name,
            round_type,
            riders,
        } => {
            engine.set_track(map_track_config(track_config));
            engine.stage_moto(
                moto_id.clone(),
                class_name.clone(),
                round_type.clone(),
                riders.iter().cloned().map(map_staged_rider).collect(),
            );

            if let RacePhase::Staged {
                moto_id: active_moto,
                ..
            } = engine.phase()
            {
                if active_moto == moto_id {
                    publish_event_payload(
                        jetstream,
                        track_id,
                        control.event_id,
                        control.ts_us,
                        RaceEventPayloadV1::RaceStaged {
                            moto_id: moto_id.clone(),
                            class_name: class_name.clone(),
                            round_type: round_type.clone(),
                            riders: riders.clone(),
                        },
                        format!(
                            "{track_id}:{}:control:{index}:race_staged",
                            control.event_id
                        ),
                    )
                    .await?;
                    index += 1;
                } else {
                    warn!(
                        track_id = %track_id,
                        requested_moto = %moto_id,
                        active_moto = %active_moto,
                        "Stage intent did not become active stage"
                    );
                }
            } else {
                warn!(track_id = %track_id, "Stage intent was rejected by race engine");
            }
        }
        RaceControlIntentV1::Reset => {
            engine.reset();

            publish_event_payload(
                jetstream,
                track_id,
                control.event_id,
                control.ts_us,
                RaceEventPayloadV1::RaceReset,
                format!("{track_id}:{}:control:{index}:race_reset", control.event_id),
            )
            .await?;
            index += 1;
        }
        RaceControlIntentV1::ForceFinish => {
            if let Some(event) = engine.force_finish()
                && let Some(payload) = map_domain_event_to_payload(event)
            {
                publish_event_payload(
                    jetstream,
                    track_id,
                    control.event_id,
                    control.ts_us,
                    payload,
                    format!(
                        "{track_id}:{}:control:{index}:race_finished",
                        control.event_id
                    ),
                )
                .await?;
                index += 1;
            }
        }
    }

    if let Some(snapshot_payload) = map_domain_event_to_payload(engine.state_snapshot()) {
        publish_snapshot_payload(
            jetstream,
            track_id,
            control.event_id,
            control.ts_us,
            snapshot_payload.clone(),
            format!("{track_id}:{}:control:snapshot", control.event_id),
        )
        .await?;

        publish_event_payload(
            jetstream,
            track_id,
            control.event_id,
            control.ts_us,
            snapshot_payload,
            format!(
                "{track_id}:{}:control:{index}:state_snapshot",
                control.event_id
            ),
        )
        .await?;
    }

    Ok(())
}

async fn publish_event_payload(
    jetstream: &jetstream::Context,
    track_id: &str,
    source_event_id: Uuid,
    ts_us: u64,
    payload: RaceEventPayloadV1,
    msg_id: String,
) -> anyhow::Result<()> {
    let subject = build_race_events_subject(track_id);
    let envelope = RaceEventEnvelopeV1 {
        event_id: Uuid::new_v4(),
        contract_version: RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id: track_id.to_string(),
        source_event_id,
        ts_us,
        payload,
    };
    let body = serde_json::to_vec(&envelope)?;

    let mut headers = HeaderMap::new();
    headers.insert("Nats-Msg-Id", msg_id);

    jetstream
        .publish_with_headers(subject, headers, body.into())
        .await?
        .await?;

    Ok(())
}

async fn publish_snapshot_payload(
    jetstream: &jetstream::Context,
    track_id: &str,
    source_event_id: Uuid,
    ts_us: u64,
    payload: RaceEventPayloadV1,
    msg_id: String,
) -> anyhow::Result<()> {
    let event_scope_id = snapshot_scope_event_id(&payload);
    let subject = build_race_snapshot_subject(track_id, &event_scope_id);
    let envelope = RaceSnapshotEnvelopeV1 {
        event_id: Uuid::new_v4(),
        contract_version: RACE_SNAPSHOT_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id: track_id.to_string(),
        event_scope_id,
        source_event_id,
        ts_us,
        payload,
    };
    let body = serde_json::to_vec(&envelope)?;

    let mut headers = HeaderMap::new();
    headers.insert("Nats-Msg-Id", msg_id);

    jetstream
        .publish_with_headers(subject, headers, body.into())
        .await?
        .await?;

    Ok(())
}

fn snapshot_scope_event_id(payload: &RaceEventPayloadV1) -> String {
    match payload {
        RaceEventPayloadV1::StateSnapshot {
            moto_id: Some(moto_id),
            ..
        } => moto_id.clone(),
        _ => "none".to_string(),
    }
}

async fn handle_processing_failure(
    message: &jetstream::Message,
    jetstream: &jetstream::Context,
    source: &str,
    event_id: Option<String>,
    track_id: Option<String>,
    failure_reason: String,
) -> anyhow::Result<()> {
    let delivered = message.info().ok().map(|info| info.delivered);
    if should_route_to_dlq(delivered, CONSUMER_MAX_DELIVER) {
        publish_message_to_dlq(
            jetstream,
            message,
            source,
            event_id,
            track_id,
            format!(
                "{failure_reason}; max deliveries reached (delivered={})",
                delivered.unwrap_or_default()
            ),
        )
        .await?;

        message
            .ack()
            .await
            .map_err(|error| anyhow!("Failed to ack terminal {source} message: {error}"))?;
    } else {
        message
            .ack_with(AckKind::Nak(Some(Duration::from_secs(RETRY_DELAY_SECS))))
            .await
            .map_err(|error| anyhow!("Failed to nak {source} message: {error}"))?;
    }

    Ok(())
}

fn should_route_to_dlq(delivered: Option<i64>, max_deliver: i64) -> bool {
    delivered.is_some_and(|attempt| attempt >= max_deliver)
}

async fn publish_message_to_dlq(
    jetstream: &jetstream::Context,
    message: &jetstream::Message,
    source: &str,
    event_id: Option<String>,
    track_id: Option<String>,
    failure_reason: String,
) -> anyhow::Result<()> {
    let info = message.info().ok();
    let envelope = DlqEnvelopeV1 {
        event_id: event_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        contract_version: DLQ_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        source: source.to_string(),
        track_id,
        stream: info.as_ref().map(|info| info.stream.to_string()),
        consumer: info.as_ref().map(|info| info.consumer.to_string()),
        subject: Some(message.subject.to_string()),
        delivered: info.as_ref().map(|info| info.delivered),
        failure_reason,
        original_payload: String::from_utf8_lossy(&message.payload).to_string(),
        failed_at_us: now_unix_micros()?,
    };
    publish_dlq_envelope(jetstream, &envelope).await
}

fn now_unix_micros() -> anyhow::Result<u64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(duration.as_micros().try_into()?)
}

fn map_domain_event_to_payload(event: RaceEvent) -> Option<RaceEventPayloadV1> {
    match event {
        RaceEvent::RaceStaged {
            moto_id,
            class_name,
            round_type,
            riders,
        } => Some(RaceEventPayloadV1::RaceStaged {
            moto_id,
            class_name,
            round_type,
            riders: riders
                .into_iter()
                .map(map_staged_rider_from_domain)
                .collect(),
        }),
        RaceEvent::GateDrop {
            moto_id,
            timestamp_us,
        } => Some(RaceEventPayloadV1::GateDrop {
            moto_id,
            timestamp_us,
        }),
        RaceEvent::SplitTime {
            moto_id,
            rider_id,
            loop_name,
            is_finish,
            elapsed_us,
            position,
            gap_to_leader_us,
        } => Some(RaceEventPayloadV1::SplitTime {
            moto_id,
            rider_id,
            loop_name,
            is_finish,
            elapsed_us,
            position,
            gap_to_leader_us,
        }),
        RaceEvent::PositionsUpdate { moto_id, positions } => {
            Some(RaceEventPayloadV1::PositionsUpdate {
                moto_id,
                positions: positions
                    .into_iter()
                    .map(map_position_from_domain)
                    .collect(),
            })
        }
        RaceEvent::RiderFinished {
            moto_id,
            rider_id,
            finish_position,
            elapsed_us,
            gap_to_leader_us,
        } => Some(RaceEventPayloadV1::RiderFinished {
            moto_id,
            rider_id,
            finish_position,
            elapsed_us,
            gap_to_leader_us,
        }),
        RaceEvent::RaceFinished { moto_id, results } => Some(RaceEventPayloadV1::RaceFinished {
            moto_id,
            results: results.into_iter().map(map_result_from_domain).collect(),
        }),
        RaceEvent::RaceReset => Some(RaceEventPayloadV1::RaceReset),
        RaceEvent::StateSnapshot {
            phase,
            moto_id,
            class_name,
            round_type,
            riders,
            positions,
            gate_drop_time_us,
            finished_count,
            total_riders,
        } => Some(RaceEventPayloadV1::StateSnapshot {
            phase,
            moto_id,
            class_name,
            round_type,
            riders: riders
                .into_iter()
                .map(map_staged_rider_from_domain)
                .collect(),
            positions: positions
                .into_iter()
                .map(map_position_from_domain)
                .collect(),
            gate_drop_time_us,
            finished_count,
            total_riders,
        }),
    }
}

fn map_track_config(track_config: &TrackConfigV1) -> TrackConfig {
    TrackConfig {
        track_id: track_config.track_id.clone(),
        name: track_config.name.clone(),
        gate_beacon_id: track_config.gate_beacon_id,
        loops: track_config.loops.iter().map(map_loop_config).collect(),
    }
}

fn map_loop_config(loop_config: &LoopConfigV1) -> LoopConfig {
    LoopConfig {
        loop_id: loop_config.loop_id.clone(),
        name: loop_config.name.clone(),
        decoder_id: loop_config.decoder_id.clone(),
        position: loop_config.position,
        is_start: loop_config.is_start,
        is_finish: loop_config.is_finish,
    }
}

fn map_staged_rider(rider: StagedRiderV1) -> StagedRider {
    StagedRider {
        rider_id: rider.rider_id,
        first_name: rider.first_name,
        last_name: rider.last_name,
        plate_number: rider.plate_number,
        transponder_id: rider.transponder_id,
        lane: rider.lane,
    }
}

fn map_staged_rider_from_domain(rider: StagedRider) -> StagedRiderV1 {
    StagedRiderV1 {
        rider_id: rider.rider_id,
        first_name: rider.first_name,
        last_name: rider.last_name,
        plate_number: rider.plate_number,
        transponder_id: rider.transponder_id,
        lane: rider.lane,
    }
}

fn map_position_from_domain(position: RiderPosition) -> RiderPositionV1 {
    RiderPositionV1 {
        rider_id: position.rider_id,
        plate_number: position.plate_number,
        first_name: position.first_name,
        last_name: position.last_name,
        lane: position.lane,
        position: position.position,
        last_loop: position.last_loop,
        elapsed_us: position.elapsed_us,
        gap_to_leader_us: position.gap_to_leader_us,
        finished: position.finished,
        dnf: position.dnf,
    }
}

fn map_result_from_domain(result: FinishResult) -> FinishResultV1 {
    FinishResultV1 {
        rider_id: result.rider_id,
        plate_number: result.plate_number,
        first_name: result.first_name,
        last_name: result.last_name,
        position: result.position,
        elapsed_us: result.elapsed_us,
        gap_to_leader_us: result.gap_to_leader_us,
        dnf: result.dnf,
        dns: result.dns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use p3_contracts::EventIdContext;
    use p3_parser::PassingMessage;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn redelivered_raw_message_is_skipped_and_state_is_unchanged() {
        let pool = test_pool().await;
        let mut engine = test_engine();

        engine.set_track(test_track_config());
        engine.stage_moto(
            "moto-1".to_string(),
            "Open".to_string(),
            "main".to_string(),
            vec![test_staged_rider()],
        );

        let gate_drop = test_passing(1, 9992, 1_000_000, Some("START"));
        let _ = engine.process_passing(&gate_drop);

        let split = test_passing(2, 1001, 1_250_000, Some("SPLIT"));
        let event_id_context = EventIdContext {
            client_id: "client-a".to_string(),
            boot_id: "boot-a".to_string(),
            seq: 42,
        };
        let dedupe_key = format!(
            "raw:{}",
            build_idempotency_key("track-a", &event_id_context)
        );

        let first =
            dispatch_with_dedupe(&pool, &dedupe_key, "track-a", DedupeSource::Raw, || async {
                let events = engine.process_passing(&split);
                assert_eq!(events.len(), 2);
                ActorDispatchOutcome::Processed
            })
            .await
            .unwrap();
        let snapshot_after_first = serde_json::to_value(engine.state_snapshot()).unwrap();

        let mut duplicate_was_processed = false;
        let second =
            dispatch_with_dedupe(&pool, &dedupe_key, "track-a", DedupeSource::Raw, || async {
                duplicate_was_processed = true;
                let _ = engine.process_passing(&split);
                ActorDispatchOutcome::Processed
            })
            .await
            .unwrap();
        let snapshot_after_second = serde_json::to_value(engine.state_snapshot()).unwrap();

        assert!(matches!(first, DedupedDispatchOutcome::Processed));
        assert!(matches!(second, DedupedDispatchOutcome::Duplicate));
        assert!(!duplicate_was_processed);
        assert_eq!(snapshot_after_first, snapshot_after_second);
    }

    #[tokio::test]
    async fn redelivered_control_intent_is_skipped_and_state_is_unchanged() {
        let pool = test_pool().await;
        let mut engine = test_engine();

        engine.set_track(test_track_config());
        engine.stage_moto(
            "moto-2".to_string(),
            "Open".to_string(),
            "main".to_string(),
            vec![test_staged_rider()],
        );

        let dedupe_key = "control:track-a:event-abc";
        let first = dispatch_with_dedupe(
            &pool,
            dedupe_key,
            "track-a",
            DedupeSource::Control,
            || async {
                engine.reset();
                ActorDispatchOutcome::Processed
            },
        )
        .await
        .unwrap();
        let snapshot_after_first = serde_json::to_value(engine.state_snapshot()).unwrap();

        let mut duplicate_was_processed = false;
        let second = dispatch_with_dedupe(
            &pool,
            dedupe_key,
            "track-a",
            DedupeSource::Control,
            || async {
                duplicate_was_processed = true;
                engine.reset();
                ActorDispatchOutcome::Processed
            },
        )
        .await
        .unwrap();
        let snapshot_after_second = serde_json::to_value(engine.state_snapshot()).unwrap();

        assert!(matches!(first, DedupedDispatchOutcome::Processed));
        assert!(matches!(second, DedupedDispatchOutcome::Duplicate));
        assert!(!duplicate_was_processed);
        assert_eq!(snapshot_after_first, snapshot_after_second);
    }

    #[tokio::test]
    async fn failed_dispatch_releases_claim_for_retry() {
        let pool = test_pool().await;
        let key = "raw:track-a:client-a:boot-a:7";

        let first = dispatch_with_dedupe(&pool, key, "track-a", DedupeSource::Raw, || async {
            ActorDispatchOutcome::Failed(anyhow!("boom"))
        })
        .await
        .unwrap();

        let second = dispatch_with_dedupe(&pool, key, "track-a", DedupeSource::Raw, || async {
            ActorDispatchOutcome::Processed
        })
        .await
        .unwrap();

        assert!(matches!(first, DedupedDispatchOutcome::Failed(_)));
        assert!(matches!(second, DedupedDispatchOutcome::Processed));
    }

    #[test]
    fn dlq_routing_only_on_terminal_attempt() {
        assert!(!should_route_to_dlq(None, CONSUMER_MAX_DELIVER));
        assert!(!should_route_to_dlq(
            Some(CONSUMER_MAX_DELIVER - 1),
            CONSUMER_MAX_DELIVER
        ));
        assert!(should_route_to_dlq(
            Some(CONSUMER_MAX_DELIVER),
            CONSUMER_MAX_DELIVER
        ));
    }

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db::run_migrations(&pool).await.unwrap();
        pool
    }

    fn test_engine() -> RaceEngine {
        let (tx, _) = tokio::sync::broadcast::channel::<Arc<RaceEvent>>(64);
        RaceEngine::new(tx)
    }

    fn test_track_config() -> TrackConfig {
        TrackConfig {
            track_id: "track-a".to_string(),
            name: "Track A".to_string(),
            gate_beacon_id: 9992,
            loops: vec![
                LoopConfig {
                    loop_id: "start".to_string(),
                    name: "Start".to_string(),
                    decoder_id: "START".to_string(),
                    position: 0,
                    is_start: true,
                    is_finish: false,
                },
                LoopConfig {
                    loop_id: "split-1".to_string(),
                    name: "Split 1".to_string(),
                    decoder_id: "SPLIT".to_string(),
                    position: 1,
                    is_start: false,
                    is_finish: false,
                },
                LoopConfig {
                    loop_id: "finish".to_string(),
                    name: "Finish".to_string(),
                    decoder_id: "FINISH".to_string(),
                    position: 2,
                    is_start: false,
                    is_finish: true,
                },
            ],
        }
    }

    fn test_staged_rider() -> StagedRider {
        StagedRider {
            rider_id: "rider-1".to_string(),
            first_name: "Riley".to_string(),
            last_name: "Hart".to_string(),
            plate_number: "11".to_string(),
            transponder_id: 1001,
            lane: 1,
        }
    }

    fn test_passing(
        passing_number: u32,
        transponder_id: u32,
        rtc_time_us: u64,
        decoder_id: Option<&str>,
    ) -> PassingMessage {
        PassingMessage {
            passing_number,
            transponder_id,
            rtc_time_us,
            utc_time_us: None,
            strength: Some(120),
            hits: Some(18),
            transponder_string: Some("FL-01001".to_string()),
            flags: 0,
            decoder_id: decoder_id.map(ToString::to_string),
        }
    }
}
