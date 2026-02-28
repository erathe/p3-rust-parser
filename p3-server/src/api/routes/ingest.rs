use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header::AUTHORIZATION},
};
use p3_contracts::{
    TRACK_INGEST_CONTRACT_VERSION_V2, TrackIngestBatchRequest, TrackIngestBatchResponse,
    message_type_from_message,
};
use std::time::Instant;

use crate::api::auth::TrackAuthError;
use crate::api::error::ApiError;
use crate::api::state::AppState;

pub async fn ingest_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TrackIngestBatchRequest>,
) -> Result<Json<TrackIngestBatchResponse>, ApiError> {
    state.metrics.inc_ingest_requests();
    let _duration_guard = IngestDurationGuard::new(state.metrics.clone());

    if req.contract_version != TRACK_INGEST_CONTRACT_VERSION_V2 {
        return Err(ApiError::BadRequest(format!(
            "Unsupported contract_version: {}",
            req.contract_version
        )));
    }

    if req.track_id.trim().is_empty() {
        return Err(ApiError::BadRequest("track_id is required".to_string()));
    }

    let auth_token = extract_track_token(&headers);
    state
        .track_auth
        .authorize_track_token(&req.track_id, auth_token.as_deref())
        .map_err(|error| {
            state.metrics.inc_ingest_auth_rejections();
            map_auth_error(error)
        })?;

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

    state.metrics.add_ingest_events_accepted(accepted as u64);
    state
        .metrics
        .add_ingest_events_duplicates(duplicates as u64);

    Ok(Json(TrackIngestBatchResponse {
        accepted,
        duplicates,
    }))
}

struct IngestDurationGuard {
    metrics: std::sync::Arc<crate::api::metrics::AppMetrics>,
    started_at: Instant,
}

impl IngestDurationGuard {
    fn new(metrics: std::sync::Arc<crate::api::metrics::AppMetrics>) -> Self {
        Self {
            metrics,
            started_at: Instant::now(),
        }
    }
}

impl Drop for IngestDurationGuard {
    fn drop(&mut self) {
        let elapsed_us = self
            .started_at
            .elapsed()
            .as_micros()
            .min(u128::from(u64::MAX)) as u64;
        self.metrics.observe_ingest_request_duration_us(elapsed_us);
    }
}

fn extract_track_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(AUTHORIZATION)
        && let Ok(raw) = value.to_str()
    {
        let candidate = raw
            .strip_prefix("Bearer ")
            .or_else(|| raw.strip_prefix("bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty());

        if let Some(token) = candidate {
            return Some(token.to_string());
        }
    }

    headers
        .get("x-track-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}

fn map_auth_error(error: TrackAuthError) -> ApiError {
    match error {
        TrackAuthError::MissingToken => ApiError::Unauthorized(
            "track token is required (Authorization: Bearer <token> or x-track-token)".to_string(),
        ),
        TrackAuthError::InvalidToken => ApiError::Unauthorized("invalid track token".to_string()),
        TrackAuthError::ForbiddenTrack => {
            ApiError::Forbidden("track token is not authorized for this track_id".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::auth::TrackAuthConfig;
    use crate::api::metrics::AppMetrics;
    use p3_parser::Message;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    async fn test_state(auth: TrackAuthConfig) -> AppState {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::run_migrations(&db).await.unwrap();
        let (message_tx, _) = broadcast::channel::<Arc<Message>>(16);

        AppState::new(
            message_tx,
            db,
            None,
            "nats://127.0.0.1:4222".to_string(),
            false,
            auth,
            Arc::new(AppMetrics::new()),
        )
    }

    fn minimal_request(track_id: &str) -> TrackIngestBatchRequest {
        TrackIngestBatchRequest {
            contract_version: TRACK_INGEST_CONTRACT_VERSION_V2.to_string(),
            track_id: track_id.to_string(),
            events: Vec::new(),
        }
    }

    fn auth_config() -> TrackAuthConfig {
        let mut tokens = HashMap::new();
        tokens.insert("track-a".to_string(), "token-a".to_string());
        tokens.insert("track-b".to_string(), "token-b".to_string());
        TrackAuthConfig::new(true, tokens)
    }

    #[tokio::test]
    async fn ingest_requires_token_when_auth_enabled() {
        let state = test_state(auth_config()).await;
        let err = ingest_batch(
            State(state),
            HeaderMap::new(),
            Json(minimal_request("track-a")),
        )
        .await
        .expect_err("expected auth failure");

        assert!(matches!(err, ApiError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn ingest_rejects_token_for_different_track() {
        let state = test_state(auth_config()).await;
        let mut headers = HeaderMap::new();
        headers.insert("x-track-token", "token-b".parse().unwrap());

        let err = ingest_batch(State(state), headers, Json(minimal_request("track-a")))
            .await
            .expect_err("expected scoped auth failure");

        assert!(matches!(err, ApiError::Forbidden(_)));
    }

    #[tokio::test]
    async fn ingest_accepts_valid_track_token() {
        let state = test_state(auth_config()).await;
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer token-a".parse().unwrap());

        let response = ingest_batch(State(state), headers, Json(minimal_request("track-a")))
            .await
            .expect("expected success")
            .0;

        assert_eq!(response.accepted, 0);
        assert_eq!(response.duplicates, 0);
    }
}
