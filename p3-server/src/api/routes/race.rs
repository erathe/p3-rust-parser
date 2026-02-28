use axum::{
    Json,
    extract::{Query, State},
};
use p3_contracts::{
    LoopConfigV1, RACE_CONTROL_INTENT_ENVELOPE_CONTRACT_VERSION_V1, RaceControlIntentEnvelopeV1,
    RaceControlIntentV1, StagedRiderV1, TrackConfigV1,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::domain::race_event::{LoopConfig, RaceEvent, StagedRider, TrackConfig};

#[derive(Debug, Deserialize)]
pub struct StageRequest {
    pub moto_id: String,
    pub track_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TrackScopedRaceControlRequest {
    pub track_id: String,
}

#[derive(Debug, Serialize)]
pub struct RaceStateResponse {
    pub phase: String,
    pub snapshot: RaceEvent,
}

#[derive(Debug, Deserialize, Default)]
pub struct GetRaceStateQuery {
    pub track_id: Option<String>,
}

/// POST /api/race/stage — Load a moto onto the gate
pub async fn stage(
    State(state): State<AppState>,
    Json(req): Json<StageRequest>,
) -> Result<Json<RaceStateResponse>, ApiError> {
    let track_id = req.track_id.trim();
    if track_id.is_empty() {
        return Err(ApiError::BadRequest("track_id is required".to_string()));
    }

    if req.moto_id.trim().is_empty() {
        return Err(ApiError::BadRequest("moto_id is required".to_string()));
    }

    // Load track config with loops from DB
    let track_row =
        sqlx::query_as::<_, crate::db::models::TrackRow>("SELECT * FROM tracks WHERE id = ?")
            .bind(track_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Track {} not found", track_id)))?;

    let loop_rows = sqlx::query_as::<_, crate::db::models::TimingLoopRow>(
        "SELECT * FROM timing_loops WHERE track_id = ? ORDER BY position",
    )
    .bind(track_id)
    .fetch_all(&state.db)
    .await?;

    let track_config = TrackConfig {
        track_id: track_row.id.clone(),
        name: track_row.name.clone(),
        gate_beacon_id: track_row.gate_beacon_id as u32,
        loops: loop_rows
            .iter()
            .map(|l| LoopConfig {
                loop_id: l.id.clone(),
                name: l.name.clone(),
                decoder_id: l.decoder_id.clone(),
                position: l.position as u32,
                is_start: l.is_start,
                is_finish: l.is_finish,
            })
            .collect(),
    };

    // Load moto with class info and rider entries
    let moto_row =
        sqlx::query_as::<_, crate::db::models::MotoRow>("SELECT * FROM motos WHERE id = ?")
            .bind(&req.moto_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Moto {} not found", req.moto_id)))?;

    let class_row = sqlx::query_as::<_, crate::db::models::EventClassRow>(
        "SELECT * FROM event_classes WHERE id = ?",
    )
    .bind(&moto_row.class_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::Internal("Moto references missing class".into()))?;

    let moto_track_id = sqlx::query_scalar::<_, String>("SELECT track_id FROM events WHERE id = ?")
        .bind(&moto_row.event_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::Internal("Moto references missing event".to_string()))?;
    if moto_track_id != track_id {
        return Err(ApiError::BadRequest(
            "moto_id does not belong to provided track_id".to_string(),
        ));
    }

    // Load entries with rider info using a join
    let entries = sqlx::query_as::<_, crate::db::models::MotoEntryRow>(
        "SELECT * FROM moto_entries WHERE moto_id = ? ORDER BY lane",
    )
    .bind(&req.moto_id)
    .fetch_all(&state.db)
    .await?;

    let mut staged_riders = Vec::new();
    for entry in &entries {
        let rider =
            sqlx::query_as::<_, crate::db::models::RiderRow>("SELECT * FROM riders WHERE id = ?")
                .bind(&entry.rider_id)
                .fetch_optional(&state.db)
                .await?
                .ok_or_else(|| {
                    ApiError::Internal(format!("Rider {} not found for moto entry", entry.rider_id))
                })?;

        staged_riders.push(StagedRider {
            rider_id: rider.id.clone(),
            first_name: rider.first_name.clone(),
            last_name: rider.last_name.clone(),
            plate_number: rider.plate_number.clone(),
            transponder_id: rider.transponder_id as u32,
            lane: entry.lane as u32,
        });
    }

    let publisher = state
        .ingest_publisher
        .as_ref()
        .ok_or_else(|| ApiError::Internal("ingest publisher is not configured".to_string()))?;

    let stage_intent = RaceControlIntentV1::Stage {
        track_config: map_track_config_to_contract(&track_config),
        moto_id: req.moto_id.clone(),
        class_name: class_row.name.clone(),
        round_type: moto_row.round_type.clone(),
        riders: staged_riders
            .iter()
            .map(map_staged_rider_to_contract)
            .collect(),
    };
    let stage_envelope = build_control_intent_envelope(track_id.to_string(), stage_intent);

    publisher
        .publish_race_control_intent(&stage_envelope)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to publish stage intent: {e}")))?;

    // Update moto status to staged
    sqlx::query("UPDATE motos SET status = 'staged' WHERE id = ?")
        .bind(&req.moto_id)
        .execute(&state.db)
        .await?;

    let snapshot = RaceEvent::StateSnapshot {
        phase: "staged".to_string(),
        moto_id: Some(req.moto_id),
        class_name: Some(class_row.name),
        round_type: Some(moto_row.round_type),
        riders: staged_riders,
        positions: Vec::new(),
        gate_drop_time_us: None,
        finished_count: 0,
        total_riders: u32::try_from(entries.len()).unwrap_or(u32::MAX),
    };
    let phase = snapshot_phase(&snapshot).to_string();

    Ok(Json(RaceStateResponse { phase, snapshot }))
}

/// POST /api/race/reset — Reset race to idle
pub async fn reset(
    State(state): State<AppState>,
    Json(req): Json<TrackScopedRaceControlRequest>,
) -> Result<Json<RaceStateResponse>, ApiError> {
    let track_id = req.track_id.trim();
    if track_id.is_empty() {
        return Err(ApiError::BadRequest("track_id is required".to_string()));
    }

    let publisher = state
        .ingest_publisher
        .as_ref()
        .ok_or_else(|| ApiError::Internal("ingest publisher is not configured".to_string()))?;

    let envelope = build_control_intent_envelope(track_id.to_string(), RaceControlIntentV1::Reset);
    publisher
        .publish_race_control_intent(&envelope)
        .await
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to publish reset race control intent: {error}"
            ))
        })?;

    let snapshot = load_track_snapshot_or_idle(&state, track_id).await?;
    Ok(Json(RaceStateResponse {
        phase: snapshot_phase(&snapshot).to_string(),
        snapshot,
    }))
}

/// POST /api/race/force-finish — Force the current race to finish
pub async fn force_finish(
    State(state): State<AppState>,
    Json(req): Json<TrackScopedRaceControlRequest>,
) -> Result<Json<RaceStateResponse>, ApiError> {
    let track_id = req.track_id.trim();
    if track_id.is_empty() {
        return Err(ApiError::BadRequest("track_id is required".to_string()));
    }

    let publisher = state
        .ingest_publisher
        .as_ref()
        .ok_or_else(|| ApiError::Internal("ingest publisher is not configured".to_string()))?;

    let envelope =
        build_control_intent_envelope(track_id.to_string(), RaceControlIntentV1::ForceFinish);
    publisher
        .publish_race_control_intent(&envelope)
        .await
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to publish force-finish race control intent: {error}"
            ))
        })?;

    let snapshot = load_track_snapshot_or_idle(&state, track_id).await?;
    Ok(Json(RaceStateResponse {
        phase: snapshot_phase(&snapshot).to_string(),
        snapshot,
    }))
}

/// GET /api/race/state — Get current race state
pub async fn get_state(
    State(state): State<AppState>,
    Query(query): Query<GetRaceStateQuery>,
) -> Result<Json<RaceStateResponse>, ApiError> {
    if let Some(track_id) = query
        .track_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let snapshot = load_track_snapshot_or_idle(&state, track_id).await?;

        return Ok(Json(RaceStateResponse {
            phase: snapshot_phase(&snapshot).to_string(),
            snapshot,
        }));
    }

    let snapshot = idle_state_snapshot();
    Ok(Json(RaceStateResponse {
        phase: snapshot_phase(&snapshot).to_string(),
        snapshot,
    }))
}

async fn load_track_snapshot_or_idle(
    state: &AppState,
    track_id: &str,
) -> Result<RaceEvent, ApiError> {
    let maybe_projection =
        crate::db::queries::race_projection::get_race_state_projection(&state.db, track_id)
            .await
            .map_err(|error| {
                ApiError::Internal(format!(
                    "Failed to load race state projection for track {track_id}: {error}"
                ))
            })?;

    Ok(maybe_projection
        .map(map_projected_state_to_snapshot)
        .unwrap_or_else(idle_state_snapshot))
}

fn map_projected_state_to_snapshot(
    projected: crate::db::queries::race_projection::ProjectedRaceState,
) -> RaceEvent {
    RaceEvent::StateSnapshot {
        phase: projected.phase,
        moto_id: projected.moto_id,
        class_name: projected.class_name,
        round_type: projected.round_type,
        riders: projected
            .riders
            .into_iter()
            .map(map_projected_rider)
            .collect(),
        positions: projected
            .positions
            .into_iter()
            .map(map_projected_position)
            .collect(),
        gate_drop_time_us: projected.gate_drop_time_us,
        finished_count: projected.finished_count,
        total_riders: projected.total_riders,
    }
}

fn map_projected_rider(rider: p3_contracts::StagedRiderV1) -> StagedRider {
    StagedRider {
        rider_id: rider.rider_id,
        first_name: rider.first_name,
        last_name: rider.last_name,
        plate_number: rider.plate_number,
        transponder_id: rider.transponder_id,
        lane: rider.lane,
    }
}

fn map_projected_position(
    position: p3_contracts::RiderPositionV1,
) -> crate::domain::race_event::RiderPosition {
    crate::domain::race_event::RiderPosition {
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

fn idle_state_snapshot() -> RaceEvent {
    RaceEvent::StateSnapshot {
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

fn snapshot_phase(snapshot: &RaceEvent) -> &str {
    match snapshot {
        RaceEvent::StateSnapshot { phase, .. } => phase.as_str(),
        _ => "idle",
    }
}

fn map_track_config_to_contract(track_config: &TrackConfig) -> TrackConfigV1 {
    TrackConfigV1 {
        track_id: track_config.track_id.clone(),
        name: track_config.name.clone(),
        gate_beacon_id: track_config.gate_beacon_id,
        loops: track_config
            .loops
            .iter()
            .map(|loop_config| LoopConfigV1 {
                loop_id: loop_config.loop_id.clone(),
                name: loop_config.name.clone(),
                decoder_id: loop_config.decoder_id.clone(),
                position: loop_config.position,
                is_start: loop_config.is_start,
                is_finish: loop_config.is_finish,
            })
            .collect(),
    }
}

fn map_staged_rider_to_contract(rider: &StagedRider) -> StagedRiderV1 {
    StagedRiderV1 {
        rider_id: rider.rider_id.clone(),
        first_name: rider.first_name.clone(),
        last_name: rider.last_name.clone(),
        plate_number: rider.plate_number.clone(),
        transponder_id: rider.transponder_id,
        lane: rider.lane,
    }
}

fn build_control_intent_envelope(
    track_id: String,
    intent: RaceControlIntentV1,
) -> RaceControlIntentEnvelopeV1 {
    RaceControlIntentEnvelopeV1 {
        event_id: Uuid::new_v4(),
        contract_version: RACE_CONTROL_INTENT_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id,
        ts_us: now_unix_micros(),
        intent,
    }
}

fn now_unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}
