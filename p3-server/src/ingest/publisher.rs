use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use async_nats::HeaderMap;
use async_nats::jetstream;
use async_nats::jetstream::stream::{Config, DiscardPolicy, RetentionPolicy, StorageType};
use p3_contracts::{
    RACE_CONTROL_SUBJECT_PATTERN_V1, RaceControlIntentEnvelopeV1, TrackIngestEvent,
    build_idempotency_key, build_race_control_subject, build_raw_ingest_envelope_v1,
    build_raw_ingest_subject,
};

pub const RAW_INGEST_STREAM_NAME: &str = "timing_ingest_raw_v1";
pub const RAW_INGEST_SUBJECT_PATTERN: &str = "timing.ingest.raw.v1.*";
pub const RACE_EVENTS_STREAM_NAME: &str = "timing_race_events_v1";
pub const RACE_EVENTS_SUBJECT_PATTERN: &str = "timing.race.events.v1.*";
pub const RACE_CONTROL_STREAM_NAME: &str = "timing_race_control_v1";
pub const RACE_CONTROL_SUBJECT_PATTERN: &str = RACE_CONTROL_SUBJECT_PATTERN_V1;

const RAW_INGEST_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;
const RAW_INGEST_MAX_BYTES: i64 = 1_073_741_824;
const RAW_INGEST_DUP_WINDOW_SECS: u64 = 10 * 60;
const RACE_EVENTS_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;
const RACE_EVENTS_MAX_BYTES: i64 = 53_687_091_200;
const RACE_EVENTS_DUP_WINDOW_SECS: u64 = 10 * 60;
const RACE_CONTROL_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;
const RACE_CONTROL_MAX_BYTES: i64 = 1_073_741_824;
const RACE_CONTROL_DUP_WINDOW_SECS: u64 = 10 * 60;

#[derive(Clone)]
pub struct IngestPublisher {
    jetstream: jetstream::Context,
}

pub struct PublishOutcome {
    pub duplicate: bool,
}

impl IngestPublisher {
    pub async fn connect_and_provision(nats_url: &str) -> anyhow::Result<Self> {
        let jetstream = connect_jetstream_and_provision_raw_race_events_and_race_control(nats_url).await?;

        Ok(Self { jetstream })
    }

    pub async fn publish_event(&self, event: &TrackIngestEvent) -> anyhow::Result<PublishOutcome> {
        let subject = build_raw_ingest_subject(&event.track_id);
        let msg_id = build_idempotency_key(&event.track_id, &event.event_id_context);
        let envelope = build_raw_ingest_envelope_v1(event, now_unix_micros()?);
        let payload = serde_json::to_vec(&envelope)?;

        let mut headers = HeaderMap::new();
        headers.insert("Nats-Msg-Id", msg_id);

        let ack = self
            .jetstream
            .publish_with_headers(subject, headers, payload.into())
            .await?
            .await?;

        Ok(PublishOutcome {
            duplicate: ack.duplicate,
        })
    }

    pub async fn publish_race_control_intent(
        &self,
        envelope: &RaceControlIntentEnvelopeV1,
    ) -> anyhow::Result<PublishOutcome> {
        let subject = build_race_control_subject(&envelope.track_id);
        let payload = serde_json::to_vec(envelope)?;

        let mut headers = HeaderMap::new();
        headers.insert("Nats-Msg-Id", envelope.event_id.to_string());

        let ack = self
            .jetstream
            .publish_with_headers(subject, headers, payload.into())
            .await?
            .await?;

        Ok(PublishOutcome {
            duplicate: ack.duplicate,
        })
    }
}

pub async fn connect_jetstream_and_provision_raw_ingest(
    nats_url: &str,
) -> anyhow::Result<jetstream::Context> {
    let client = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(client);
    ensure_raw_ingest_stream(&jetstream).await?;
    Ok(jetstream)
}

pub async fn connect_jetstream_and_provision_raw_and_race_events(
    nats_url: &str,
) -> anyhow::Result<jetstream::Context> {
    let client = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(client);
    ensure_raw_ingest_stream(&jetstream).await?;
    ensure_race_events_stream(&jetstream).await?;
    Ok(jetstream)
}

pub async fn connect_jetstream_and_provision_raw_race_events_and_race_control(
    nats_url: &str,
) -> anyhow::Result<jetstream::Context> {
    let client = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(client);
    ensure_raw_ingest_stream(&jetstream).await?;
    ensure_race_events_stream(&jetstream).await?;
    ensure_race_control_stream(&jetstream).await?;
    Ok(jetstream)
}

pub async fn ensure_raw_ingest_stream(jetstream: &jetstream::Context) -> anyhow::Result<()> {
    let stream_config = raw_ingest_stream_config();

    if jetstream.get_stream(RAW_INGEST_STREAM_NAME).await.is_ok() {
        jetstream.update_stream(stream_config).await?;
    } else {
        jetstream.create_stream(stream_config).await?;
    }

    Ok(())
}

pub async fn ensure_race_events_stream(jetstream: &jetstream::Context) -> anyhow::Result<()> {
    let stream_config = race_events_stream_config();

    if jetstream.get_stream(RACE_EVENTS_STREAM_NAME).await.is_ok() {
        jetstream.update_stream(stream_config).await?;
    } else {
        jetstream.create_stream(stream_config).await?;
    }

    Ok(())
}

pub async fn ensure_race_control_stream(jetstream: &jetstream::Context) -> anyhow::Result<()> {
    let stream_config = race_control_stream_config();

    if jetstream.get_stream(RACE_CONTROL_STREAM_NAME).await.is_ok() {
        jetstream.update_stream(stream_config).await?;
    } else {
        jetstream.create_stream(stream_config).await?;
    }

    Ok(())
}

fn now_unix_micros() -> anyhow::Result<u64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(duration.as_micros().try_into()?)
}

fn raw_ingest_stream_config() -> Config {
    Config {
        name: RAW_INGEST_STREAM_NAME.to_string(),
        subjects: vec![RAW_INGEST_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(RAW_INGEST_MAX_AGE_SECS),
        max_bytes: RAW_INGEST_MAX_BYTES,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(RAW_INGEST_DUP_WINDOW_SECS),
        storage: StorageType::File,
        ..Default::default()
    }
}

fn race_events_stream_config() -> Config {
    Config {
        name: RACE_EVENTS_STREAM_NAME.to_string(),
        subjects: vec![RACE_EVENTS_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(RACE_EVENTS_MAX_AGE_SECS),
        max_bytes: RACE_EVENTS_MAX_BYTES,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(RACE_EVENTS_DUP_WINDOW_SECS),
        storage: StorageType::File,
        ..Default::default()
    }
}

fn race_control_stream_config() -> Config {
    Config {
        name: RACE_CONTROL_STREAM_NAME.to_string(),
        subjects: vec![RACE_CONTROL_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(RACE_CONTROL_MAX_AGE_SECS),
        max_bytes: RACE_CONTROL_MAX_BYTES,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(RACE_CONTROL_DUP_WINDOW_SECS),
        storage: StorageType::File,
        ..Default::default()
    }
}
