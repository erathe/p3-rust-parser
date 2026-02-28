use axum::{Json, extract::State};
use p3_contracts::{
    TRACK_INGEST_CONTRACT_VERSION_V2, TrackIngestBatchRequest, TrackIngestBatchResponse,
    message_type_from_message,
};

use crate::api::error::ApiError;
use crate::api::state::AppState;

pub async fn ingest_batch(
    State(state): State<AppState>,
    Json(req): Json<TrackIngestBatchRequest>,
) -> Result<Json<TrackIngestBatchResponse>, ApiError> {
    if req.contract_version != TRACK_INGEST_CONTRACT_VERSION_V2 {
        return Err(ApiError::BadRequest(format!(
            "Unsupported contract_version: {}",
            req.contract_version
        )));
    }

    if req.track_id.trim().is_empty() {
        return Err(ApiError::BadRequest("track_id is required".to_string()));
    }

    if req.events.is_empty() {
        return Ok(Json(TrackIngestBatchResponse {
            accepted: 0,
            duplicates: 0,
        }));
    }

    for event in &req.events {
        if event.track_id.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "event.track_id is required".to_string(),
            ));
        }
        if event.track_id != req.track_id {
            return Err(ApiError::BadRequest(
                "event.track_id must match request track_id".to_string(),
            ));
        }
        if event.message_type.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "event.message_type is required".to_string(),
            ));
        }
        let derived_message_type = message_type_from_message(&event.payload);
        if event.message_type != derived_message_type {
            return Err(ApiError::BadRequest(format!(
                "event.message_type must match payload type: expected {}",
                derived_message_type
            )));
        }
        if event.event_id_context.client_id.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "event.event_id_context.client_id is required".to_string(),
            ));
        }
        if event.event_id_context.boot_id.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "event.event_id_context.boot_id is required".to_string(),
            ));
        }
    }

    let publisher = state
        .ingest_publisher
        .as_ref()
        .ok_or_else(|| ApiError::Internal("ingest publisher is not configured".to_string()))?;

    let mut accepted = 0usize;
    let mut duplicates = 0usize;

    for event in &req.events {
        let outcome = publisher
            .publish_event(event)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to publish ingest event: {e}")))?;
        if outcome.duplicate {
            duplicates += 1;
        } else {
            accepted += 1;
        }
    }

    Ok(Json(TrackIngestBatchResponse {
        accepted,
        duplicates,
    }))
}
