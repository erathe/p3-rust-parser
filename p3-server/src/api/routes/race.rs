use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::domain::race_event::{LoopConfig, RaceEvent, StagedRider, TrackConfig};

#[derive(Debug, Deserialize)]
pub struct StageRequest {
    pub moto_id: String,
    pub track_id: String,
}

#[derive(Debug, Serialize)]
pub struct RaceStateResponse {
    pub phase: String,
    pub snapshot: RaceEvent,
}

/// POST /api/race/stage — Load a moto onto the gate
pub async fn stage(
    State(state): State<AppState>,
    Json(req): Json<StageRequest>,
) -> Result<Json<RaceStateResponse>, ApiError> {
    // Load track config with loops from DB
    let track_row = sqlx::query_as::<_, crate::db::models::TrackRow>(
        "SELECT * FROM tracks WHERE id = ?",
    )
    .bind(&req.track_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Track {} not found", req.track_id)))?;

    let loop_rows = sqlx::query_as::<_, crate::db::models::TimingLoopRow>(
        "SELECT * FROM timing_loops WHERE track_id = ? ORDER BY position",
    )
    .bind(&req.track_id)
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
    let moto_row = sqlx::query_as::<_, crate::db::models::MotoRow>(
        "SELECT * FROM motos WHERE id = ?",
    )
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

    // Load entries with rider info using a join
    let entries = sqlx::query_as::<_, crate::db::models::MotoEntryRow>(
        "SELECT * FROM moto_entries WHERE moto_id = ? ORDER BY lane",
    )
    .bind(&req.moto_id)
    .fetch_all(&state.db)
    .await?;

    let mut staged_riders = Vec::new();
    for entry in &entries {
        let rider = sqlx::query_as::<_, crate::db::models::RiderRow>(
            "SELECT * FROM riders WHERE id = ?",
        )
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

    // Update moto status to staged
    sqlx::query("UPDATE motos SET status = 'staged' WHERE id = ?")
        .bind(&req.moto_id)
        .execute(&state.db)
        .await?;

    // Configure and stage the engine
    let mut engine = state.engine.lock().await;
    engine.set_track(track_config);
    engine.stage_moto(
        req.moto_id,
        class_row.name.clone(),
        moto_row.round_type.clone(),
        staged_riders,
    );

    let snapshot = engine.state_snapshot();
    let phase = engine.phase().name().to_string();

    Ok(Json(RaceStateResponse { phase, snapshot }))
}

/// POST /api/race/reset — Reset race to idle
pub async fn reset(State(state): State<AppState>) -> Json<RaceStateResponse> {
    let mut engine = state.engine.lock().await;
    engine.reset();
    let snapshot = engine.state_snapshot();
    let phase = engine.phase().name().to_string();
    Json(RaceStateResponse { phase, snapshot })
}

/// POST /api/race/force-finish — Force the current race to finish
pub async fn force_finish(
    State(state): State<AppState>,
) -> Result<Json<RaceStateResponse>, ApiError> {
    let mut engine = state.engine.lock().await;
    engine.force_finish();
    let snapshot = engine.state_snapshot();
    let phase = engine.phase().name().to_string();
    Ok(Json(RaceStateResponse { phase, snapshot }))
}

/// GET /api/race/state — Get current race state
pub async fn get_state(State(state): State<AppState>) -> Json<RaceStateResponse> {
    let engine = state.engine.lock().await;
    let snapshot = engine.state_snapshot();
    let phase = engine.phase().name().to_string();
    Json(RaceStateResponse { phase, snapshot })
}
