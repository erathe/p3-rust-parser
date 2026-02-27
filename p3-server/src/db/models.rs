use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TrackRow {
    pub id: String,
    pub name: String,
    pub hill_type: String,
    pub gate_beacon_id: i64,
    pub location_label: Option<String>,
    pub timezone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TimingLoopRow {
    pub id: String,
    pub track_id: String,
    pub name: String,
    pub decoder_id: String,
    pub position: i64,
    pub is_finish: bool,
    pub is_start: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TrackSectionRow {
    pub id: String,
    pub track_id: String,
    pub name: String,
    pub section_type: String,
    pub length_m: f64,
    pub position: i64,
    pub loop_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RiderRow {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub transponder_id: i64,
    pub transponder_string: Option<String>,
    pub age_group: Option<String>,
    pub skill_level: Option<String>,
    pub gender: Option<String>,
    pub equipment: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventRow {
    pub id: String,
    pub name: String,
    pub date: String,
    pub track_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventClassRow {
    pub id: String,
    pub event_id: String,
    pub name: String,
    pub age_group: Option<String>,
    pub skill_level: Option<String>,
    pub gender: Option<String>,
    pub equipment: Option<String>,
    pub race_format: String,
    pub scoring: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MotoRow {
    pub id: String,
    pub event_id: String,
    pub class_id: String,
    pub round_type: String,
    pub round_number: Option<i64>,
    pub sequence: i64,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MotoEntryRow {
    pub id: String,
    pub moto_id: String,
    pub rider_id: String,
    pub lane: i64,
    pub finish_position: Option<i64>,
    pub elapsed_us: Option<i64>,
    pub points: Option<i64>,
    pub dnf: bool,
    pub dns: bool,
    pub created_at: String,
}
