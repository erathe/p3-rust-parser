use axum::{Json, extract::State};
use p3_contracts::{
    LoopConfigV1, RACE_CONTROL_INTENT_ENVELOPE_CONTRACT_VERSION_V1, RaceControlIntentEnvelopeV1,
    RaceControlIntentV1, StagedRiderV1, TrackConfigV1,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;
use uuid::Uuid;

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
    let track_row =
        sqlx::query_as::<_, crate::db::models::TrackRow>("SELECT * FROM tracks WHERE id = ?")
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
    let stage_envelope = build_control_intent_envelope(req.track_id.clone(), stage_intent);

    publisher
        .publish_race_control_intent(&stage_envelope)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to publish stage intent: {e}")))?;

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
    if let Some(track_id) = resolve_track_id_for_active_moto(&state).await {
        if let Some(publisher) = &state.ingest_publisher {
            let envelope =
                build_control_intent_envelope(track_id, RaceControlIntentV1::Reset);
            if let Err(error) = publisher.publish_race_control_intent(&envelope).await {
                warn!(error = %error, "Failed to publish reset race control intent");
            }
        } else {
            warn!("Skipping reset race control intent publish: ingest publisher unavailable");
        }
    }

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
    if let Some(track_id) = resolve_track_id_for_active_moto(&state).await {
        if let Some(publisher) = &state.ingest_publisher {
            let envelope =
                build_control_intent_envelope(track_id, RaceControlIntentV1::ForceFinish);
            if let Err(error) = publisher.publish_race_control_intent(&envelope).await {
                warn!(error = %error, "Failed to publish force-finish race control intent");
            }
        } else {
            warn!(
                "Skipping force-finish race control intent publish: ingest publisher unavailable"
            );
        }
    }

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

async fn resolve_track_id_for_active_moto(state: &AppState) -> Option<String> {
    let active_moto_id = {
        let engine = state.engine.lock().await;
        match engine.state_snapshot() {
            RaceEvent::StateSnapshot { moto_id, .. } => moto_id,
            _ => None,
        }
    };

    let Some(moto_id) = active_moto_id else {
        return None;
    };

    let row = match sqlx::query_as::<_, (String,)>(
        "SELECT events.track_id FROM motos JOIN events ON events.id = motos.event_id WHERE motos.id = ?",
    )
    .bind(moto_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(row) => row,
        Err(error) => {
            warn!(error = %error, "Failed to resolve track id for active moto");
            return None;
        }
    };

    row.map(|(track_id,)| track_id)
}

fn now_unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}
