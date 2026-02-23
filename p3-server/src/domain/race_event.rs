use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Events broadcast to WebSocket clients during a race.
/// These are separate from raw P3 messages — they represent interpreted race state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum RaceEvent {
    /// A moto has been loaded and is ready for gate drop
    #[serde(rename = "race_staged")]
    RaceStaged {
        moto_id: String,
        class_name: String,
        round_type: String,
        riders: Vec<StagedRider>,
    },

    /// Gate has dropped — race clock starts
    #[serde(rename = "gate_drop")]
    GateDrop {
        moto_id: String,
        timestamp_us: u64,
    },

    /// A rider crossed a timing loop (split or finish)
    #[serde(rename = "split_time")]
    SplitTime {
        moto_id: String,
        rider_id: String,
        loop_name: String,
        is_finish: bool,
        elapsed_us: u64,
        position: u32,
        gap_to_leader_us: Option<u64>,
    },

    /// Current positions updated (sent after each split/finish)
    #[serde(rename = "positions_update")]
    PositionsUpdate {
        moto_id: String,
        positions: Vec<RiderPosition>,
    },

    /// A rider has crossed the finish line
    #[serde(rename = "rider_finished")]
    RiderFinished {
        moto_id: String,
        rider_id: String,
        finish_position: u32,
        elapsed_us: u64,
        gap_to_leader_us: Option<u64>,
    },

    /// All riders finished (or timeout/force-finish)
    #[serde(rename = "race_finished")]
    RaceFinished {
        moto_id: String,
        results: Vec<FinishResult>,
    },

    /// Race has been reset back to idle
    #[serde(rename = "race_reset")]
    RaceReset,

    /// Current race state snapshot (sent to newly connected clients)
    #[serde(rename = "state_snapshot")]
    StateSnapshot {
        phase: String,
        moto_id: Option<String>,
        class_name: Option<String>,
        round_type: Option<String>,
        riders: Vec<StagedRider>,
        positions: Vec<RiderPosition>,
        gate_drop_time_us: Option<u64>,
        finished_count: u32,
        total_riders: u32,
    },
}

/// A rider in a staged moto, before the race starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedRider {
    pub rider_id: String,
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub transponder_id: u32,
    pub lane: u32,
}

/// A rider's current position in the race.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiderPosition {
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

/// Final result for a rider in a finished moto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishResult {
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

/// Internal rider state tracked by the engine during a race.
#[derive(Debug, Clone)]
pub struct RiderState {
    pub rider_id: String,
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub transponder_id: u32,
    pub lane: u32,
    /// Split times keyed by loop_id → elapsed_us from gate drop
    pub splits: HashMap<String, u64>,
    /// The furthest loop (by position) the rider has been seen at
    pub last_loop_position: Option<u32>,
    pub last_loop_name: Option<String>,
    /// Elapsed time at the last seen loop
    pub last_elapsed_us: Option<u64>,
    /// Finish result
    pub finish_elapsed_us: Option<u64>,
    pub finish_position: Option<u32>,
    pub finished: bool,
    pub dnf: bool,
}

impl RiderState {
    pub fn new(
        rider_id: String,
        first_name: String,
        last_name: String,
        plate_number: String,
        transponder_id: u32,
        lane: u32,
    ) -> Self {
        Self {
            rider_id,
            first_name,
            last_name,
            plate_number,
            transponder_id,
            lane,
            splits: HashMap::new(),
            last_loop_position: None,
            last_loop_name: None,
            last_elapsed_us: None,
            finish_elapsed_us: None,
            finish_position: None,
            finished: false,
            dnf: false,
        }
    }

    pub fn to_position(&self, position: u32, gap_to_leader_us: Option<u64>) -> RiderPosition {
        RiderPosition {
            rider_id: self.rider_id.clone(),
            plate_number: self.plate_number.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            lane: self.lane,
            position,
            last_loop: self.last_loop_name.clone(),
            elapsed_us: self.last_elapsed_us,
            gap_to_leader_us,
            finished: self.finished,
            dnf: self.dnf,
        }
    }
}

/// Track configuration loaded for the engine.
#[derive(Debug, Clone)]
pub struct TrackConfig {
    pub track_id: String,
    pub name: String,
    pub gate_beacon_id: u32,
    /// Timing loops ordered by position (0=first, n=last)
    pub loops: Vec<LoopConfig>,
}

/// A single timing loop on the track.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub loop_id: String,
    pub name: String,
    pub decoder_id: String,
    pub position: u32,
    pub is_start: bool,
    pub is_finish: bool,
}
