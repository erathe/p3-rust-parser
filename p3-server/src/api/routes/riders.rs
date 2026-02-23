use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::models::RiderRow;
use crate::db::queries::riders::{self, CreateRider};

#[derive(Deserialize)]
pub struct ListQuery {
    pub search: Option<String>,
}

#[derive(Deserialize)]
pub struct RiderRequest {
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub transponder_id: i64,
    pub transponder_string: Option<String>,
    pub age_group: Option<String>,
    pub skill_level: Option<String>,
    pub gender: Option<String>,
    pub equipment: Option<String>,
}

impl From<RiderRequest> for CreateRider {
    fn from(r: RiderRequest) -> Self {
        CreateRider {
            first_name: r.first_name,
            last_name: r.last_name,
            plate_number: r.plate_number,
            transponder_id: r.transponder_id,
            transponder_string: r.transponder_string,
            age_group: r.age_group,
            skill_level: r.skill_level,
            gender: r.gender,
            equipment: r.equipment,
        }
    }
}

pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<RiderRow>>, ApiError> {
    let rows = riders::list_riders(&state.db, query.search.as_deref()).await?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RiderRow>, ApiError> {
    riders::get_rider(&state.db, &id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Rider {} not found", id)))
        .map(Json)
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<RiderRequest>,
) -> Result<(StatusCode, Json<RiderRow>), ApiError> {
    let rider = riders::create_rider(&state.db, body.into()).await?;
    Ok((StatusCode::CREATED, Json(rider)))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RiderRequest>,
) -> Result<Json<RiderRow>, ApiError> {
    riders::update_rider(&state.db, &id, body.into())
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Rider {} not found", id)))
        .map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if riders::delete_rider(&state.db, &id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Rider {} not found", id)))
    }
}
