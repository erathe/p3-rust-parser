use anyhow::anyhow;
use async_nats::jetstream;
use async_nats::jetstream::consumer::AckPolicy;
use futures_util::StreamExt;
use p3_contracts::{RawIngestEnvelopeV1, build_idempotency_key};
use p3_parser::Message;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::ingest::publisher::{
    RAW_INGEST_STREAM_NAME, RAW_INGEST_SUBJECT_PATTERN, connect_jetstream_and_provision_raw_ingest,
};

const DECODER_STATUS_PROJECTION_CONSUMER: &str = "projection_decoder_status_v1";

pub async fn run_projection_worker(nats_url: &str, pool: &SqlitePool) -> anyhow::Result<()> {
    let jetstream = connect_jetstream_and_provision_raw_ingest(nats_url).await?;
    let stream = jetstream.get_stream(RAW_INGEST_STREAM_NAME).await?;
    let consumer = get_or_create_consumer(&stream).await?;
    let mut messages = consumer.messages().await?;

    info!(
        nats_url = %nats_url,
        consumer = DECODER_STATUS_PROJECTION_CONSUMER,
        subject = RAW_INGEST_SUBJECT_PATTERN,
        "Projection worker started"
    );

    while let Some(message_result) = messages.next().await {
        let message = match message_result {
            Ok(message) => message,
            Err(error) => {
                warn!(error = %error, "Projection worker failed to receive message");
                continue;
            }
        };

        let envelope: RawIngestEnvelopeV1 = match serde_json::from_slice(&message.payload) {
            Ok(envelope) => envelope,
            Err(error) => {
                warn!(error = %error, "Failed to parse ingest envelope, acking poison message");
                message
                    .ack()
                    .await
                    .map_err(|error| anyhow!("Failed to ack poison message: {error}"))?;
                continue;
            }
        };

        match process_envelope(pool, &envelope).await {
            Ok(ProcessOutcome::Applied) => {
                message
                    .ack()
                    .await
                    .map_err(|error| anyhow!("Failed to ack applied message: {error}"))?;
            }
            Ok(ProcessOutcome::Duplicate) => {
                message
                    .ack()
                    .await
                    .map_err(|error| anyhow!("Failed to ack duplicate message: {error}"))?;
            }
            Err(error) => {
                warn!(error = %error, "Projection processing failed, leaving message unacked");
            }
        }
    }

    Ok(())
}

async fn get_or_create_consumer(
    stream: &jetstream::stream::Stream,
) -> anyhow::Result<jetstream::consumer::Consumer<jetstream::consumer::pull::Config>> {
    if let Ok(consumer) = stream
        .get_consumer::<jetstream::consumer::pull::Config>(DECODER_STATUS_PROJECTION_CONSUMER)
        .await
    {
        return Ok(consumer);
    }

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(DECODER_STATUS_PROJECTION_CONSUMER.to_string()),
        filter_subject: RAW_INGEST_SUBJECT_PATTERN.to_string(),
        ack_policy: AckPolicy::Explicit,
        ..Default::default()
    };

    let consumer = stream.create_consumer(config).await?;
    Ok(consumer)
}

enum ProcessOutcome {
    Applied,
    Duplicate,
}

async fn process_envelope(
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
