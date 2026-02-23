use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::models::{TimingLoopRow, TrackRow, TrackSectionRow};
use crate::db::queries::tracks;

// Response type that includes track + its loops + sections
#[derive(serde::Serialize)]
pub struct TrackWithLoops {
    #[serde(flatten)]
    pub track: TrackRow,
    pub loops: Vec<TimingLoopRow>,
    pub sections: Vec<TrackSectionRow>,
}

#[derive(Deserialize)]
pub struct CreateTrackRequest {
    pub name: String,
    pub hill_type: Option<String>,
    pub gate_beacon_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateLoopRequest {
    pub name: String,
    pub decoder_id: String,
    pub position: i64,
    #[serde(default)]
    pub is_finish: bool,
    #[serde(default)]
    pub is_start: bool,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<TrackRow>>, ApiError> {
    let rows = tracks::list_tracks(&state.db).await?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrackWithLoops>, ApiError> {
    let track = tracks::get_track(&state.db, &id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Track {} not found", id)))?;

    let loops = tracks::get_loops_for_track(&state.db, &id).await?;
    let sections = tracks::get_sections_for_track(&state.db, &id).await?;

    Ok(Json(TrackWithLoops {
        track,
        loops,
        sections,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateTrackRequest>,
) -> Result<(StatusCode, Json<TrackRow>), ApiError> {
    let hill_type = body.hill_type.as_deref().unwrap_or("8m");
    let gate_beacon_id = body.gate_beacon_id.unwrap_or(9992);

    if hill_type != "5m" && hill_type != "8m" {
        return Err(ApiError::BadRequest(
            "hill_type must be '5m' or '8m'".to_string(),
        ));
    }

    let track = tracks::create_track(&state.db, &body.name, hill_type, gate_beacon_id).await?;
    Ok((StatusCode::CREATED, Json(track)))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateTrackRequest>,
) -> Result<Json<TrackRow>, ApiError> {
    let hill_type = body.hill_type.as_deref().unwrap_or("8m");
    let gate_beacon_id = body.gate_beacon_id.unwrap_or(9992);

    tracks::update_track(&state.db, &id, &body.name, hill_type, gate_beacon_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Track {} not found", id)))
        .map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if tracks::delete_track(&state.db, &id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Track {} not found", id)))
    }
}

// Timing loop endpoints

pub async fn create_loop(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Json(body): Json<CreateLoopRequest>,
) -> Result<(StatusCode, Json<TimingLoopRow>), ApiError> {
    // Verify track exists
    tracks::get_track(&state.db, &track_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Track {} not found", track_id)))?;

    let timing_loop = tracks::create_timing_loop(
        &state.db,
        &track_id,
        &body.name,
        &body.decoder_id,
        body.position,
        body.is_finish,
        body.is_start,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(timing_loop)))
}

pub async fn update_loop(
    State(state): State<AppState>,
    Path((_track_id, loop_id)): Path<(String, String)>,
    Json(body): Json<CreateLoopRequest>,
) -> Result<Json<TimingLoopRow>, ApiError> {
    tracks::update_timing_loop(
        &state.db,
        &loop_id,
        &body.name,
        &body.decoder_id,
        body.position,
        body.is_finish,
        body.is_start,
    )
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Loop {} not found", loop_id)))
    .map(Json)
}

pub async fn delete_loop(
    State(state): State<AppState>,
    Path((_track_id, loop_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    if tracks::delete_timing_loop(&state.db, &loop_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Loop {} not found", loop_id)))
    }
}

// Track section endpoints

#[derive(Deserialize)]
pub struct SaveSectionsRequest {
    pub sections: Vec<SectionInput>,
}

#[derive(Deserialize)]
pub struct SectionInput {
    pub name: String,
    pub section_type: String,
    pub length_m: f64,
    pub loop_id: Option<String>,
}

pub async fn save_sections(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Json(body): Json<SaveSectionsRequest>,
) -> Result<Json<Vec<TrackSectionRow>>, ApiError> {
    tracks::get_track(&state.db, &track_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Track {} not found", track_id)))?;

    let new_sections: Vec<tracks::NewSection> = body
        .sections
        .into_iter()
        .enumerate()
        .map(|(i, s)| tracks::NewSection {
            name: s.name,
            section_type: s.section_type,
            length_m: s.length_m,
            position: i as i64,
            loop_id: s.loop_id,
        })
        .collect();

    let saved = tracks::replace_all_sections(&state.db, &track_id, new_sections).await?;
    Ok(Json(saved))
}
