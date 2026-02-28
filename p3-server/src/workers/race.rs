use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use async_nats::error::Error as NatsError;
use async_nats::HeaderMap;
use async_nats::jetstream;
use async_nats::jetstream::consumer::pull::MessagesErrorKind;
use async_nats::jetstream::consumer::AckPolicy;
use futures_util::StreamExt;
use p3_contracts::{
    FinishResultV1, LoopConfigV1, RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1,
    RaceControlIntentEnvelopeV1, RaceControlIntentV1, RaceEventEnvelopeV1, RaceEventPayloadV1,
    RawIngestEnvelopeV1, RiderPositionV1, StagedRiderV1, TrackConfigV1, build_race_events_subject,
};
use p3_parser::Message;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::race_event::{FinishResult, LoopConfig, RaceEvent, RiderPosition, StagedRider, TrackConfig};
use crate::engine::{RaceEngine, RacePhase};
use crate::ingest::publisher::{
    RACE_CONTROL_STREAM_NAME, RACE_CONTROL_SUBJECT_PATTERN, RAW_INGEST_STREAM_NAME,
    RAW_INGEST_SUBJECT_PATTERN, connect_jetstream_and_provision_raw_race_events_and_race_control,
};

const RACE_WORKER_RAW_CONSUMER: &str = "race_worker_raw_v1";
const RACE_WORKER_CONTROL_CONSUMER: &str = "race_worker_control_v1";

enum TrackActorPayload {
    Raw(RawIngestEnvelopeV1),
    Control(RaceControlIntentEnvelopeV1),
}

struct TrackActorInput {
    payload: TrackActorPayload,
    result_tx: oneshot::Sender<anyhow::Result<()>>,
}

pub async fn run_race_worker(nats_url: &str) -> anyhow::Result<()> {
    let jetstream = connect_jetstream_and_provision_raw_race_events_and_race_control(nats_url).await?;
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
                        handle_raw_message(&jetstream, &mut track_actors, message_result).await?;
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
                        handle_control_message(&jetstream, &mut track_actors, message_result).await?;
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
            warn!(error = %error, "Failed to parse raw ingest envelope, acking poison message");
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack poison raw message: {error}"))?;
            return Ok(());
        }
    };

    dispatch_to_track_actor(
        track_actors,
        envelope.track_id.clone(),
        jetstream.clone(),
        TrackActorPayload::Raw(envelope),
        message,
    )
    .await
}

async fn handle_control_message(
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
            warn!(error = %error, "Failed to parse race control envelope, acking poison message");
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack poison control message: {error}"))?;
            return Ok(());
        }
    };

    dispatch_to_track_actor(
        track_actors,
        envelope.track_id.clone(),
        jetstream.clone(),
        TrackActorPayload::Control(envelope),
        message,
    )
    .await
}

async fn dispatch_to_track_actor(
    track_actors: &mut HashMap<String, mpsc::Sender<TrackActorInput>>,
    track_id: String,
    jetstream: jetstream::Context,
    payload: TrackActorPayload,
    message: jetstream::Message,
) -> anyhow::Result<()> {
    let actor = track_actors
        .entry(track_id.clone())
        .or_insert_with(|| spawn_track_actor(track_id, jetstream))
        .clone();

    let (result_tx, result_rx) = oneshot::channel();
    if actor.send(TrackActorInput { payload, result_tx }).await.is_err() {
        warn!("Race track actor unavailable, leaving message unacked");
        return Ok(());
    }

    match result_rx.await {
        Ok(Ok(())) => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack processed message: {error}"))?;
        }
        Ok(Err(error)) => {
            warn!(error = %error, "Race actor processing failed, leaving message unacked");
        }
        Err(error) => {
            warn!(error = %error, "Race actor dropped response, leaving message unacked");
        }
    }

    Ok(())
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
                        format!("{track_id}:{}:control:{index}:race_staged", control.event_id),
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
                    format!("{track_id}:{}:control:{index}:race_finished", control.event_id),
                )
                .await?;
                index += 1;
            }
        }
    }

    if let Some(snapshot_payload) = map_domain_event_to_payload(engine.state_snapshot()) {
        publish_event_payload(
            jetstream,
            track_id,
            control.event_id,
            control.ts_us,
            snapshot_payload,
            format!("{track_id}:{}:control:{index}:state_snapshot", control.event_id),
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
            riders: riders.into_iter().map(map_staged_rider_from_domain).collect(),
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
                positions: positions.into_iter().map(map_position_from_domain).collect(),
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
            riders: riders.into_iter().map(map_staged_rider_from_domain).collect(),
            positions: positions.into_iter().map(map_position_from_domain).collect(),
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
