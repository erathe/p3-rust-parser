use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::models::{EventClassRow, EventRow, RiderRow};
use crate::db::queries::events as queries;

// --- Request/Response types ---

#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    pub name: String,
    pub date: String,
    pub track_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEventRequest {
    pub name: String,
    pub date: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct EventWithClasses {
    #[serde(flatten)]
    pub event: EventRow,
    pub classes: Vec<ClassWithRiders>,
}

#[derive(Debug, Deserialize)]
pub struct CreateClassRequest {
    pub name: String,
    pub age_group: Option<String>,
    pub skill_level: Option<String>,
    pub gender: Option<String>,
    pub equipment: Option<String>,
    pub race_format: Option<String>,
    pub scoring: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClassWithRiders {
    #[serde(flatten)]
    pub class: EventClassRow,
    pub riders: Vec<RiderRow>,
}

#[derive(Debug, Deserialize)]
pub struct ClassRiderRequest {
    pub rider_id: String,
}

// --- Event CRUD ---

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<EventRow>>, ApiError> {
    let events = queries::list_events(&state.db).await?;
    Ok(Json(events))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EventWithClasses>, ApiError> {
    let event = queries::get_event(&state.db, &id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Event not found".into()))?;

    let classes = queries::list_classes(&state.db, &id).await?;

    let mut classes_with_riders = Vec::new();
    for class in classes {
        let rider_ids = queries::list_class_rider_ids(&state.db, &class.id).await?;
        let mut riders = Vec::new();
        for rid in &rider_ids {
            if let Some(rider) = crate::db::queries::riders::get_rider(&state.db, rid).await? {
                riders.push(rider);
            }
        }
        classes_with_riders.push(ClassWithRiders { class, riders });
    }

    Ok(Json(EventWithClasses {
        event,
        classes: classes_with_riders,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateEventRequest>,
) -> Result<Json<EventRow>, ApiError> {
    let id = uuid::Uuid::new_v4().to_string();
    let event = queries::create_event(&state.db, &id, &req.name, &req.date, &req.track_id).await?;
    Ok(Json(event))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateEventRequest>,
) -> Result<Json<EventRow>, ApiError> {
    let event = queries::update_event(&state.db, &id, &req.name, &req.date, &req.status)
        .await
        .map_err(|_| ApiError::NotFound("Event not found".into()))?;
    Ok(Json(event))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    queries::delete_event(&state.db, &id).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

// --- Class CRUD ---

pub async fn create_class(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
    Json(req): Json<CreateClassRequest>,
) -> Result<Json<EventClassRow>, ApiError> {
    // Verify event exists
    queries::get_event(&state.db, &event_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Event not found".into()))?;

    let id = uuid::Uuid::new_v4().to_string();
    let class = queries::create_class(
        &state.db,
        &id,
        &event_id,
        &req.name,
        req.age_group.as_deref(),
        req.skill_level.as_deref(),
        req.gender.as_deref(),
        req.equipment.as_deref(),
        req.race_format.as_deref().unwrap_or("motos_only"),
        req.scoring.as_deref().unwrap_or("total_points"),
    )
    .await?;
    Ok(Json(class))
}

pub async fn delete_class(
    State(state): State<AppState>,
    Path((_event_id, class_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    queries::delete_class(&state.db, &class_id).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

// --- Class Riders ---

pub async fn add_class_rider(
    State(state): State<AppState>,
    Path((_event_id, class_id)): Path<(String, String)>,
    Json(req): Json<ClassRiderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    queries::add_rider_to_class(&state.db, &class_id, &req.rider_id).await?;
    Ok(Json(serde_json::json!({"added": true})))
}

pub async fn remove_class_rider(
    State(state): State<AppState>,
    Path((_event_id, class_id, rider_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    queries::remove_rider_from_class(&state.db, &class_id, &rider_id).await?;
    Ok(Json(serde_json::json!({"removed": true})))
}

// --- Standings ---

pub async fn class_standings(
    State(state): State<AppState>,
    Path((_event_id, class_id)): Path<(String, String)>,
) -> Result<Json<Vec<crate::db::queries::results::RiderStanding>>, ApiError> {
    let standings = crate::db::queries::results::get_class_standings(&state.db, &class_id).await?;
    Ok(Json(standings))
}
