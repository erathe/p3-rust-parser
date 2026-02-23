use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::models::{MotoEntryRow, MotoRow, RiderRow};
use crate::db::queries::{events as event_queries, motos as moto_queries};
use crate::domain::race_format;

#[derive(Debug, Serialize)]
pub struct MotoWithEntries {
    #[serde(flatten)]
    pub moto: MotoRow,
    pub entries: Vec<EntryWithRider>,
}

#[derive(Debug, Serialize)]
pub struct EntryWithRider {
    #[serde(flatten)]
    pub entry: MotoEntryRow,
    pub rider: Option<RiderRow>,
}

#[derive(Debug, Serialize)]
pub struct GenerateResult {
    pub format: String,
    pub motos_created: usize,
}

/// GET /api/events/:event_id/motos — List motos for an event
pub async fn list_for_event(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
) -> Result<Json<Vec<MotoRow>>, ApiError> {
    let motos = moto_queries::list_motos_for_event(&state.db, &event_id).await?;
    Ok(Json(motos))
}

/// GET /api/events/:event_id/classes/:class_id/motos — List motos for a class
pub async fn list_for_class(
    State(state): State<AppState>,
    Path((_event_id, class_id)): Path<(String, String)>,
) -> Result<Json<Vec<MotoRow>>, ApiError> {
    let motos = moto_queries::list_motos_for_class(&state.db, &class_id).await?;
    Ok(Json(motos))
}

/// GET /api/motos/:id — Get moto detail with entries and rider info
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MotoWithEntries>, ApiError> {
    let moto = moto_queries::get_moto(&state.db, &id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Moto not found".into()))?;

    let entries = moto_queries::list_entries(&state.db, &id).await?;

    let mut entries_with_riders = Vec::new();
    for entry in entries {
        let rider = crate::db::queries::riders::get_rider(&state.db, &entry.rider_id).await?;
        entries_with_riders.push(EntryWithRider {
            entry,
            rider,
        });
    }

    Ok(Json(MotoWithEntries {
        moto,
        entries: entries_with_riders,
    }))
}

/// POST /api/events/:event_id/classes/:class_id/generate-motos
///
/// Generates moto sheets for qualifying rounds + elimination round placeholders.
/// Deletes any existing motos for this class first.
pub async fn generate(
    State(state): State<AppState>,
    Path((event_id, class_id)): Path<(String, String)>,
) -> Result<Json<GenerateResult>, ApiError> {
    // Verify class exists and belongs to this event
    let class = event_queries::get_class(&state.db, &class_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Class not found".into()))?;

    if class.event_id != event_id {
        return Err(ApiError::BadRequest("Class does not belong to this event".into()));
    }

    // Get riders in this class
    let rider_ids = event_queries::list_class_rider_ids(&state.db, &class_id).await?;

    if rider_ids.is_empty() {
        return Err(ApiError::BadRequest("No riders in class".into()));
    }

    // Delete existing motos for this class
    moto_queries::delete_motos_for_class(&state.db, &class_id).await?;

    // Determine format and generate moto sheets
    let format = race_format::determine_format(rider_ids.len());
    let qualifying = race_format::generate_qualifying_motos(&rider_ids);

    let last_qual_seq = qualifying.last().map(|m| m.sequence).unwrap_or(0);
    let elimination = race_format::generate_elimination_motos(&format, last_qual_seq + 1);

    let total_motos = qualifying.len() + elimination.len();

    // Insert all motos and entries into DB
    for assignment in qualifying.iter().chain(elimination.iter()) {
        let moto_id = uuid::Uuid::new_v4().to_string();
        moto_queries::create_moto(
            &state.db,
            &moto_id,
            &event_id,
            &class_id,
            &assignment.round_type,
            assignment.round_number,
            assignment.sequence,
        )
        .await?;

        for (rider_id, lane) in &assignment.entries {
            let entry_id = uuid::Uuid::new_v4().to_string();
            moto_queries::create_entry(&state.db, &entry_id, &moto_id, rider_id, *lane)
                .await?;
        }
    }

    Ok(Json(GenerateResult {
        format: format.as_str().to_string(),
        motos_created: total_motos,
    }))
}
