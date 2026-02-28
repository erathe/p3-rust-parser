use p3_parser::Message;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const TRACK_INGEST_CONTRACT_VERSION_V2: &str = "track_ingest.v2";
pub const RAW_INGEST_ENVELOPE_CONTRACT_VERSION_V1: &str = "raw_ingest_envelope.v1";
pub const RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1: &str = "race_events_envelope.v1";
pub const RACE_CONTROL_INTENT_ENVELOPE_CONTRACT_VERSION_V1: &str =
    "race_control_intent_envelope.v1";
pub const RACE_CONTROL_SUBJECT_PATTERN_V1: &str = "timing.race.control.v1.*";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventIdContext {
    pub client_id: String,
    pub boot_id: String,
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackIngestEvent {
    pub event_id: Uuid,
    pub track_id: String,
    pub event_id_context: EventIdContext,
    pub captured_at_us: u64,
    pub message_type: String,
    pub payload: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawIngestEnvelopeV1 {
    pub event_id: Uuid,
    pub contract_version: String,
    pub track_id: String,
    pub event_id_context: EventIdContext,
    pub captured_at_us: u64,
    pub ingested_at_us: u64,
    pub message_type: String,
    pub payload: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RaceEventPayloadV1 {
    DecoderMessage {
        message: Message,
    },
    RaceStaged {
        moto_id: String,
        class_name: String,
        round_type: String,
        riders: Vec<StagedRiderV1>,
    },
    GateDrop {
        moto_id: String,
        timestamp_us: u64,
    },
    SplitTime {
        moto_id: String,
        rider_id: String,
        loop_name: String,
        is_finish: bool,
        elapsed_us: u64,
        position: u32,
        gap_to_leader_us: Option<u64>,
    },
    PositionsUpdate {
        moto_id: String,
        positions: Vec<RiderPositionV1>,
    },
    RiderFinished {
        moto_id: String,
        rider_id: String,
        finish_position: u32,
        elapsed_us: u64,
        gap_to_leader_us: Option<u64>,
    },
    RaceFinished {
        moto_id: String,
        results: Vec<FinishResultV1>,
    },
    RaceReset,
    StateSnapshot {
        phase: String,
        moto_id: Option<String>,
        class_name: Option<String>,
        round_type: Option<String>,
        riders: Vec<StagedRiderV1>,
        positions: Vec<RiderPositionV1>,
        gate_drop_time_us: Option<u64>,
        finished_count: u32,
        total_riders: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedRiderV1 {
    pub rider_id: String,
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub transponder_id: u32,
    pub lane: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiderPositionV1 {
    pub rider_id: String,
    pub plate_number: String,
    pub first_name: String,
    pub last_name: String,
    pub lane: u32,
    pub position: u32,
    pub last_loop: Option<String>,
    pub elapsed_us: Option<u64>,
    pub gap_to_leader_us: Option<u64>,
    pub finished: bool,
    pub dnf: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishResultV1 {
    pub rider_id: String,
    pub plate_number: String,
    pub first_name: String,
    pub last_name: String,
    pub position: u32,
    pub elapsed_us: Option<u64>,
    pub gap_to_leader_us: Option<u64>,
    pub dnf: bool,
    pub dns: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackConfigV1 {
    pub track_id: String,
    pub name: String,
    pub gate_beacon_id: u32,
    pub loops: Vec<LoopConfigV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfigV1 {
    pub loop_id: String,
    pub name: String,
    pub decoder_id: String,
    pub position: u32,
    pub is_start: bool,
    pub is_finish: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RaceControlIntentV1 {
    Stage {
        track_config: TrackConfigV1,
        moto_id: String,
        class_name: String,
        round_type: String,
        riders: Vec<StagedRiderV1>,
    },
    Reset,
    ForceFinish,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceControlIntentEnvelopeV1 {
    pub event_id: Uuid,
    pub contract_version: String,
    pub track_id: String,
    pub ts_us: u64,
    pub intent: RaceControlIntentV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceEventEnvelopeV1 {
    pub event_id: Uuid,
    pub contract_version: String,
    pub track_id: String,
    pub source_event_id: Uuid,
    pub ts_us: u64,
    pub payload: RaceEventPayloadV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveEnvelopeKindV1 {
    Snapshot,
    Event,
    Heartbeat,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveChannelV1 {
    Decoder,
    Race,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveEnvelopeV1<T> {
    pub kind: LiveEnvelopeKindV1,
    pub channel: LiveChannelV1,
    pub track_id: String,
    pub event_id: Option<String>,
    pub seq: u64,
    pub ts_us: u64,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderSnapshotPayloadV1 {
    pub rows: Vec<DecoderStatusRowV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderStatusRowV1 {
    pub loop_id: String,
    pub loop_name: String,
    pub loop_position: i64,
    pub decoder_id: String,
    pub noise: Option<i64>,
    pub temperature: Option<i64>,
    pub gps_status: Option<i64>,
    pub satellites: Option<i64>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderEventPayloadV1 {
    pub message: Message,
    pub source_event_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveErrorPayloadV1 {
    pub code: String,
    pub message: String,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct EmptyPayloadV1 {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackIngestBatchRequest {
    pub contract_version: String,
    pub track_id: String,
    pub events: Vec<TrackIngestEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackIngestBatchResponse {
    pub accepted: usize,
    pub duplicates: usize,
}

pub fn message_type_from_message(message: &Message) -> &'static str {
    match message {
        Message::Passing(_) => "PASSING",
        Message::Status(_) => "STATUS",
        Message::Version(_) => "VERSION",
    }
}

pub fn build_idempotency_key(track_id: &str, context: &EventIdContext) -> String {
    format!(
        "{}:{}:{}:{}",
        track_id, context.client_id, context.boot_id, context.seq
    )
}

pub fn build_raw_ingest_subject(track_id: &str) -> String {
    format!("timing.ingest.raw.v1.{}", track_id)
}

pub fn build_race_events_subject(track_id: &str) -> String {
    format!("timing.race.events.v1.{}", track_id)
}

pub fn build_race_control_subject(track_id: &str) -> String {
    format!("timing.race.control.v1.{}", track_id)
}

pub fn build_raw_ingest_envelope_v1(
    event: &TrackIngestEvent,
    ingested_at_us: u64,
) -> RawIngestEnvelopeV1 {
    RawIngestEnvelopeV1 {
        event_id: event.event_id,
        contract_version: RAW_INGEST_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id: event.track_id.clone(),
        event_id_context: event.event_id_context.clone(),
        captured_at_us: event.captured_at_us,
        ingested_at_us,
        message_type: event.message_type.clone(),
        payload: event.payload.clone(),
    }
}

pub fn build_race_event_envelope_v1_from_raw(raw: &RawIngestEnvelopeV1) -> RaceEventEnvelopeV1 {
    RaceEventEnvelopeV1 {
        event_id: Uuid::new_v4(),
        contract_version: RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id: raw.track_id.clone(),
        source_event_id: raw.event_id,
        ts_us: raw.captured_at_us,
        payload: RaceEventPayloadV1::DecoderMessage {
            message: raw.payload.clone(),
        },
    }
}
