use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use async_nats::HeaderMap;
use async_nats::jetstream;
use async_nats::jetstream::stream::{Config, DiscardPolicy, RetentionPolicy, StorageType};
use p3_contracts::{
    DlqEnvelopeV1, RACE_CONTROL_SUBJECT_PATTERN_V1, RaceControlIntentEnvelopeV1, TrackIngestEvent,
    build_dlq_subject, build_idempotency_key, build_race_control_subject,
    build_raw_ingest_envelope_v1, build_raw_ingest_subject,
};

pub const RAW_INGEST_STREAM_NAME: &str = "timing_ingest_raw_v1";
pub const RAW_INGEST_SUBJECT_PATTERN: &str = "timing.ingest.raw.v1.*";
pub const RACE_EVENTS_STREAM_NAME: &str = "timing_race_events_v1";
pub const RACE_EVENTS_SUBJECT_PATTERN: &str = "timing.race.events.v1.*";
pub const RACE_SNAPSHOT_STREAM_NAME: &str = "timing_race_snapshot_v1";
pub const RACE_SNAPSHOT_SUBJECT_PATTERN: &str = "timing.race.snapshot.v1.*.*";
pub const RACE_CONTROL_STREAM_NAME: &str = "timing_race_control_v1";
pub const RACE_CONTROL_SUBJECT_PATTERN: &str = RACE_CONTROL_SUBJECT_PATTERN_V1;
pub const DLQ_STREAM_NAME: &str = "timing_dlq_v1";
pub const DLQ_SUBJECT_PATTERN: &str = "timing.dlq.v1.*";

const RAW_INGEST_MAX_AGE_SECS_DEFAULT: u64 = 7 * 24 * 60 * 60;
const RAW_INGEST_MAX_BYTES_DEFAULT: i64 = 107_374_182_400;
const RAW_INGEST_DUP_WINDOW_SECS_DEFAULT: u64 = 10 * 60;
const RACE_EVENTS_MAX_AGE_SECS_DEFAULT: u64 = 30 * 24 * 60 * 60;
const RACE_EVENTS_MAX_BYTES_DEFAULT: i64 = 53_687_091_200;
const RACE_EVENTS_DUP_WINDOW_SECS_DEFAULT: u64 = 10 * 60;
const RACE_SNAPSHOT_MAX_AGE_SECS_DEFAULT: u64 = 24 * 60 * 60;
const RACE_SNAPSHOT_DUP_WINDOW_SECS_DEFAULT: u64 = 10 * 60;
const RACE_CONTROL_MAX_AGE_SECS_DEFAULT: u64 = 30 * 24 * 60 * 60;
const RACE_CONTROL_MAX_BYTES_DEFAULT: i64 = 1_073_741_824;
const RACE_CONTROL_DUP_WINDOW_SECS_DEFAULT: u64 = 10 * 60;
const DLQ_MAX_AGE_SECS_DEFAULT: u64 = 14 * 24 * 60 * 60;
const DLQ_MAX_BYTES_DEFAULT: i64 = 10_737_418_240;
const DLQ_DUP_WINDOW_SECS_DEFAULT: u64 = 10 * 60;

static DLQ_PUBLISHED_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct IngestPublisher {
    jetstream: jetstream::Context,
}

pub struct PublishOutcome {
    pub duplicate: bool,
}

impl IngestPublisher {
    pub async fn connect_and_provision(nats_url: &str) -> anyhow::Result<Self> {
        let jetstream =
            connect_jetstream_and_provision_raw_race_events_and_race_control(nats_url).await?;

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

pub async fn publish_dlq_envelope(
    jetstream: &jetstream::Context,
    envelope: &DlqEnvelopeV1,
) -> anyhow::Result<()> {
    let subject = build_dlq_subject(&envelope.source);
    let payload = serde_json::to_vec(envelope)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "Nats-Msg-Id",
        format!(
            "{}:{}:{}",
            envelope.source, envelope.event_id, envelope.failed_at_us
        ),
    );

    jetstream
        .publish_with_headers(subject, headers, payload.into())
        .await?
        .await?;

    DLQ_PUBLISHED_TOTAL.fetch_add(1, Ordering::Relaxed);

    Ok(())
}

pub fn dlq_published_total() -> u64 {
    DLQ_PUBLISHED_TOTAL.load(Ordering::Relaxed)
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
    ensure_race_snapshot_stream(&jetstream).await?;
    ensure_dlq_stream(&jetstream).await?;
    Ok(jetstream)
}

pub async fn connect_jetstream_and_provision_raw_race_events_and_race_control(
    nats_url: &str,
) -> anyhow::Result<jetstream::Context> {
    let client = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(client);
    ensure_raw_ingest_stream(&jetstream).await?;
    ensure_race_events_stream(&jetstream).await?;
    ensure_race_snapshot_stream(&jetstream).await?;
    ensure_race_control_stream(&jetstream).await?;
    ensure_dlq_stream(&jetstream).await?;
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

pub async fn ensure_race_snapshot_stream(jetstream: &jetstream::Context) -> anyhow::Result<()> {
    let stream_config = race_snapshot_stream_config();

    if jetstream
        .get_stream(RACE_SNAPSHOT_STREAM_NAME)
        .await
        .is_ok()
    {
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

pub async fn ensure_dlq_stream(jetstream: &jetstream::Context) -> anyhow::Result<()> {
    let stream_config = dlq_stream_config();

    if jetstream.get_stream(DLQ_STREAM_NAME).await.is_ok() {
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
    let settings = stream_settings_from_env(env_value);

    Config {
        name: RAW_INGEST_STREAM_NAME.to_string(),
        subjects: vec![RAW_INGEST_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(settings.raw_max_age_secs),
        max_bytes: settings.raw_max_bytes,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(settings.raw_duplicate_window_secs),
        storage: StorageType::File,
        ..Default::default()
    }
}

fn race_events_stream_config() -> Config {
    let settings = stream_settings_from_env(env_value);

    Config {
        name: RACE_EVENTS_STREAM_NAME.to_string(),
        subjects: vec![RACE_EVENTS_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(settings.race_events_max_age_secs),
        max_bytes: settings.race_events_max_bytes,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(settings.race_events_duplicate_window_secs),
        storage: StorageType::File,
        ..Default::default()
    }
}

fn race_snapshot_stream_config() -> Config {
    let settings = stream_settings_from_env(env_value);

    Config {
        name: RACE_SNAPSHOT_STREAM_NAME.to_string(),
        subjects: vec![RACE_SNAPSHOT_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(settings.race_snapshot_max_age_secs),
        max_messages_per_subject: 1,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(settings.race_snapshot_duplicate_window_secs),
        storage: StorageType::File,
        ..Default::default()
    }
}

fn race_control_stream_config() -> Config {
    let settings = stream_settings_from_env(env_value);

    Config {
        name: RACE_CONTROL_STREAM_NAME.to_string(),
        subjects: vec![RACE_CONTROL_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(settings.race_control_max_age_secs),
        max_bytes: settings.race_control_max_bytes,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(settings.race_control_duplicate_window_secs),
        storage: StorageType::File,
        ..Default::default()
    }
}

fn dlq_stream_config() -> Config {
    let settings = stream_settings_from_env(env_value);

    Config {
        name: DLQ_STREAM_NAME.to_string(),
        subjects: vec![DLQ_SUBJECT_PATTERN.to_string()],
        retention: RetentionPolicy::Limits,
        max_age: Duration::from_secs(settings.dlq_max_age_secs),
        max_bytes: settings.dlq_max_bytes,
        discard: DiscardPolicy::Old,
        duplicate_window: Duration::from_secs(settings.dlq_duplicate_window_secs),
        storage: StorageType::File,
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy)]
struct StreamSettings {
    raw_max_age_secs: u64,
    raw_max_bytes: i64,
    raw_duplicate_window_secs: u64,
    race_events_max_age_secs: u64,
    race_events_max_bytes: i64,
    race_events_duplicate_window_secs: u64,
    race_snapshot_max_age_secs: u64,
    race_snapshot_duplicate_window_secs: u64,
    race_control_max_age_secs: u64,
    race_control_max_bytes: i64,
    race_control_duplicate_window_secs: u64,
    dlq_max_age_secs: u64,
    dlq_max_bytes: i64,
    dlq_duplicate_window_secs: u64,
}

fn stream_settings_from_env<F>(env_getter: F) -> StreamSettings
where
    F: Fn(&str) -> Option<String>,
{
    StreamSettings {
        raw_max_age_secs: parse_env_u64(
            &env_getter,
            "P3_RAW_MAX_AGE_SECS",
            RAW_INGEST_MAX_AGE_SECS_DEFAULT,
        ),
        raw_max_bytes: parse_env_i64(
            &env_getter,
            "P3_RAW_MAX_BYTES",
            RAW_INGEST_MAX_BYTES_DEFAULT,
        ),
        raw_duplicate_window_secs: parse_env_u64(
            &env_getter,
            "P3_RAW_DUPLICATE_WINDOW_SECS",
            RAW_INGEST_DUP_WINDOW_SECS_DEFAULT,
        ),
        race_events_max_age_secs: parse_env_u64(
            &env_getter,
            "P3_RACE_EVENTS_MAX_AGE_SECS",
            RACE_EVENTS_MAX_AGE_SECS_DEFAULT,
        ),
        race_events_max_bytes: parse_env_i64(
            &env_getter,
            "P3_RACE_EVENTS_MAX_BYTES",
            RACE_EVENTS_MAX_BYTES_DEFAULT,
        ),
        race_events_duplicate_window_secs: parse_env_u64(
            &env_getter,
            "P3_RACE_EVENTS_DUPLICATE_WINDOW_SECS",
            RACE_EVENTS_DUP_WINDOW_SECS_DEFAULT,
        ),
        race_snapshot_max_age_secs: parse_env_u64(
            &env_getter,
            "P3_RACE_SNAPSHOT_MAX_AGE_SECS",
            RACE_SNAPSHOT_MAX_AGE_SECS_DEFAULT,
        ),
        race_snapshot_duplicate_window_secs: parse_env_u64(
            &env_getter,
            "P3_RACE_SNAPSHOT_DUPLICATE_WINDOW_SECS",
            RACE_SNAPSHOT_DUP_WINDOW_SECS_DEFAULT,
        ),
        race_control_max_age_secs: parse_env_u64(
            &env_getter,
            "P3_RACE_CONTROL_MAX_AGE_SECS",
            RACE_CONTROL_MAX_AGE_SECS_DEFAULT,
        ),
        race_control_max_bytes: parse_env_i64(
            &env_getter,
            "P3_RACE_CONTROL_MAX_BYTES",
            RACE_CONTROL_MAX_BYTES_DEFAULT,
        ),
        race_control_duplicate_window_secs: parse_env_u64(
            &env_getter,
            "P3_RACE_CONTROL_DUPLICATE_WINDOW_SECS",
            RACE_CONTROL_DUP_WINDOW_SECS_DEFAULT,
        ),
        dlq_max_age_secs: parse_env_u64(
            &env_getter,
            "P3_DLQ_MAX_AGE_SECS",
            DLQ_MAX_AGE_SECS_DEFAULT,
        ),
        dlq_max_bytes: parse_env_i64(&env_getter, "P3_DLQ_MAX_BYTES", DLQ_MAX_BYTES_DEFAULT),
        dlq_duplicate_window_secs: parse_env_u64(
            &env_getter,
            "P3_DLQ_DUPLICATE_WINDOW_SECS",
            DLQ_DUP_WINDOW_SECS_DEFAULT,
        ),
    }
}

fn parse_env_u64<F>(env_getter: &F, key: &str, default: u64) -> u64
where
    F: Fn(&str) -> Option<String>,
{
    env_getter(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_env_i64<F>(env_getter: &F, key: &str, default: i64) -> i64
where
    F: Fn(&str) -> Option<String>,
{
    env_getter(key)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn env_value(key: &str) -> Option<String> {
    env::var(key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_defaults_match_adr_sizing() {
        let settings = stream_settings_from_env(|_| None);

        assert_eq!(settings.raw_max_bytes, RAW_INGEST_MAX_BYTES_DEFAULT);
        assert_eq!(
            settings.race_events_max_bytes,
            RACE_EVENTS_MAX_BYTES_DEFAULT
        );
        assert_eq!(
            settings.race_snapshot_max_age_secs,
            RACE_SNAPSHOT_MAX_AGE_SECS_DEFAULT
        );
        assert_eq!(settings.dlq_max_age_secs, DLQ_MAX_AGE_SECS_DEFAULT);
        assert_eq!(settings.dlq_max_bytes, DLQ_MAX_BYTES_DEFAULT);
    }

    #[test]
    fn stream_configs_use_default_key_fields() {
        let raw = raw_ingest_stream_config();
        let race = race_events_stream_config();
        let snapshot = race_snapshot_stream_config();
        let dlq = dlq_stream_config();

        assert_eq!(raw.max_bytes, RAW_INGEST_MAX_BYTES_DEFAULT);
        assert_eq!(race.max_bytes, RACE_EVENTS_MAX_BYTES_DEFAULT);
        assert_eq!(snapshot.max_messages_per_subject, 1);
        assert_eq!(
            snapshot.max_age,
            Duration::from_secs(RACE_SNAPSHOT_MAX_AGE_SECS_DEFAULT)
        );
        assert_eq!(dlq.max_age, Duration::from_secs(DLQ_MAX_AGE_SECS_DEFAULT));
        assert_eq!(dlq.max_bytes, DLQ_MAX_BYTES_DEFAULT);
    }
}
