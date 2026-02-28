use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use async_nats::error::Error as NatsError;
use async_nats::jetstream;
use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::AckPolicy;
use async_nats::jetstream::consumer::pull::MessagesErrorKind;
use futures_util::StreamExt;
use p3_contracts::{
    DLQ_ENVELOPE_CONTRACT_VERSION_V1, DlqEnvelopeV1, RaceEventEnvelopeV1, RawIngestEnvelopeV1,
    build_idempotency_key,
};
use p3_parser::Message;
use sqlx::SqlitePool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::queries::race_projection::{ProcessOutcome, project_race_event};
use crate::ingest::publisher::{
    RACE_EVENTS_STREAM_NAME, RACE_EVENTS_SUBJECT_PATTERN, RAW_INGEST_STREAM_NAME,
    RAW_INGEST_SUBJECT_PATTERN, connect_jetstream_and_provision_raw_and_race_events,
    publish_dlq_envelope,
};

const DECODER_STATUS_PROJECTION_CONSUMER: &str = "projection_decoder_status_v1";
const RACE_STATE_PROJECTION_CONSUMER: &str = "projection_race_state_v1";
const CONSUMER_ACK_WAIT_SECS: u64 = 30;
const CONSUMER_MAX_DELIVER: i64 = 10;
const RETRY_DELAY_SECS: u64 = 2;
const DLQ_SOURCE_RAW: &str = "projection_raw";
const DLQ_SOURCE_RACE: &str = "projection_race";

pub async fn run_projection_worker(nats_url: &str, pool: &SqlitePool) -> anyhow::Result<()> {
    let jetstream = connect_jetstream_and_provision_raw_and_race_events(nats_url).await?;
    let raw_stream = jetstream.get_stream(RAW_INGEST_STREAM_NAME).await?;
    let race_stream = jetstream.get_stream(RACE_EVENTS_STREAM_NAME).await?;

    let raw_consumer = get_or_create_consumer(
        &raw_stream,
        DECODER_STATUS_PROJECTION_CONSUMER,
        RAW_INGEST_SUBJECT_PATTERN,
    )
    .await?;
    let race_consumer = get_or_create_consumer(
        &race_stream,
        RACE_STATE_PROJECTION_CONSUMER,
        RACE_EVENTS_SUBJECT_PATTERN,
    )
    .await?;

    let mut raw_messages = raw_consumer.messages().await?;
    let mut race_messages = race_consumer.messages().await?;
    let mut raw_open = true;
    let mut race_open = true;

    info!(
        nats_url = %nats_url,
        raw_consumer = DECODER_STATUS_PROJECTION_CONSUMER,
        raw_subject = RAW_INGEST_SUBJECT_PATTERN,
        race_consumer = RACE_STATE_PROJECTION_CONSUMER,
        race_subject = RACE_EVENTS_SUBJECT_PATTERN,
        "Projection worker started"
    );

    while raw_open || race_open {
        tokio::select! {
            raw_message_result = raw_messages.next(), if raw_open => {
                match raw_message_result {
                    Some(message_result) => {
                        handle_raw_message(pool, &jetstream, message_result).await?;
                    }
                    None => {
                        raw_open = false;
                        warn!("Projection raw consumer stream closed");
                    }
                }
            }
            race_message_result = race_messages.next(), if race_open => {
                match race_message_result {
                    Some(message_result) => {
                        handle_race_message(pool, &jetstream, message_result).await?;
                    }
                    None => {
                        race_open = false;
                        warn!("Projection race-events consumer stream closed");
                    }
                }
            }
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
        ack_wait: Duration::from_secs(CONSUMER_ACK_WAIT_SECS),
        max_deliver: CONSUMER_MAX_DELIVER,
        ..Default::default()
    };

    let consumer = stream.create_consumer(config).await?;
    Ok(consumer)
}

async fn handle_raw_message(
    pool: &SqlitePool,
    jetstream: &jetstream::Context,
    message_result: Result<jetstream::Message, NatsError<MessagesErrorKind>>,
) -> anyhow::Result<()> {
    let message = match message_result {
        Ok(message) => message,
        Err(error) => {
            warn!(error = %error, "Projection worker failed to receive raw message");
            return Ok(());
        }
    };

    let envelope: RawIngestEnvelopeV1 = match serde_json::from_slice(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            publish_message_to_dlq(
                jetstream,
                &message,
                DLQ_SOURCE_RAW,
                None,
                None,
                format!("Failed to parse ingest envelope: {error}"),
            )
            .await?;
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack poison message: {error}"))?;
            return Ok(());
        }
    };

    let event_id_for_dlq = envelope.event_id.to_string();
    let track_id_for_dlq = envelope.track_id.clone();

    match process_raw_envelope(pool, &envelope).await {
        Ok(ProcessOutcome::Applied) => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack applied raw message: {error}"))?;
        }
        Ok(ProcessOutcome::Duplicate) => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack duplicate raw message: {error}"))?;
        }
        Err(error) => {
            handle_processing_failure(
                &message,
                jetstream,
                DLQ_SOURCE_RAW,
                Some(event_id_for_dlq),
                Some(track_id_for_dlq),
                error.to_string(),
            )
            .await?;
        }
    }

    Ok(())
}

async fn handle_race_message(
    pool: &SqlitePool,
    jetstream: &jetstream::Context,
    message_result: Result<jetstream::Message, NatsError<MessagesErrorKind>>,
) -> anyhow::Result<()> {
    let message = match message_result {
        Ok(message) => message,
        Err(error) => {
            warn!(error = %error, "Projection worker failed to receive race-event message");
            return Ok(());
        }
    };

    let envelope: RaceEventEnvelopeV1 = match serde_json::from_slice(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            publish_message_to_dlq(
                jetstream,
                &message,
                DLQ_SOURCE_RACE,
                None,
                None,
                format!("Failed to parse race-event envelope: {error}"),
            )
            .await?;
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack poison race-event message: {error}"))?;
            return Ok(());
        }
    };

    let event_id_for_dlq = envelope.event_id.to_string();
    let track_id_for_dlq = envelope.track_id.clone();

    match project_race_event(pool, &envelope).await {
        Ok(ProcessOutcome::Applied) => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack applied race-event message: {error}"))?;
        }
        Ok(ProcessOutcome::Duplicate) => {
            message
                .ack()
                .await
                .map_err(|error| anyhow!("Failed to ack duplicate race-event message: {error}"))?;
        }
        Err(error) => {
            handle_processing_failure(
                &message,
                jetstream,
                DLQ_SOURCE_RACE,
                Some(event_id_for_dlq),
                Some(track_id_for_dlq),
                error.to_string(),
            )
            .await?;
        }
    }

    Ok(())
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

async fn process_raw_envelope(
    pool: &SqlitePool,
    envelope: &RawIngestEnvelopeV1,
) -> anyhow::Result<ProcessOutcome> {
    let idempotency_key = build_idempotency_key(&envelope.track_id, &envelope.event_id_context);
    let dedupe_insert = sqlx::query(
        "INSERT INTO projection_dedupe (idempotency_key) VALUES (?) \
         ON CONFLICT(idempotency_key) DO NOTHING",
    )
    .bind(&idempotency_key)
    .execute(pool)
    .await?;

    if dedupe_insert.rows_affected() == 0 {
        return Ok(ProcessOutcome::Duplicate);
    }

    if let Message::Status(status) = &envelope.payload
        && let Some(decoder_id) = &status.decoder_id
    {
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
        .execute(pool)
        .await?;
    }

    Ok(ProcessOutcome::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
