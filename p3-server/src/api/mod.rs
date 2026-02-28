pub mod auth;
pub mod error;
pub mod metrics;
pub mod routes;
pub mod state;
pub mod ws;

use axum::{
    Router,
    routing::{get, post, put},
};
use state::AppState;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub fn router(state: AppState) -> Router {
    let enable_dev_ingest = state.enable_dev_ingest;

    let router = Router::new()
        // WebSocket
        .route("/ws/v1/live", get(ws::ws_live_handler))
        // Tracks
        .route(
            "/api/tracks",
            get(routes::tracks::list).post(routes::tracks::create),
        )
        .route(
            "/api/tracks/{id}",
            get(routes::tracks::get)
                .put(routes::tracks::update)
                .delete(routes::tracks::delete),
        )
        .route(
            "/api/tracks/{track_id}/loops",
            post(routes::tracks::create_loop),
        )
        .route(
            "/api/tracks/{track_id}/sections",
            put(routes::tracks::save_sections),
        )
        .route(
            "/api/tracks/{track_id}/loops/{loop_id}",
            put(routes::tracks::update_loop).delete(routes::tracks::delete_loop),
        )
        .route(
            "/api/tracks/{track_id}/onboarding/discovery",
            get(routes::onboarding::discovery),
        )
        // Riders
        .route(
            "/api/riders",
            get(routes::riders::list).post(routes::riders::create),
        )
        .route(
            "/api/riders/{id}",
            get(routes::riders::get)
                .put(routes::riders::update)
                .delete(routes::riders::delete),
        )
        // Events
        .route(
            "/api/events",
            get(routes::events::list).post(routes::events::create),
        )
        .route(
            "/api/events/{id}",
            get(routes::events::get)
                .put(routes::events::update)
                .delete(routes::events::delete),
        )
        // Event classes
        .route(
            "/api/events/{event_id}/classes",
            post(routes::events::create_class),
        )
        .route(
            "/api/events/{event_id}/classes/{class_id}",
            axum::routing::delete(routes::events::delete_class),
        )
        // Class riders
        .route(
            "/api/events/{event_id}/classes/{class_id}/riders",
            post(routes::events::add_class_rider),
        )
        .route(
            "/api/events/{event_id}/classes/{class_id}/riders/{rider_id}",
            axum::routing::delete(routes::events::remove_class_rider),
        )
        // Motos
        .route(
            "/api/events/{event_id}/motos",
            get(routes::motos::list_for_event),
        )
        .route(
            "/api/events/{event_id}/classes/{class_id}/motos",
            get(routes::motos::list_for_class),
        )
        .route(
            "/api/events/{event_id}/classes/{class_id}/generate-motos",
            post(routes::motos::generate),
        )
        .route("/api/motos/{id}", get(routes::motos::get))
        // Standings
        .route(
            "/api/events/{event_id}/classes/{class_id}/standings",
            get(routes::events::class_standings),
        )
        // Race control
        .route("/api/race/state", get(routes::race::get_state))
        .route("/api/race/stage", post(routes::race::stage))
        .route("/api/race/reset", post(routes::race::reset))
        .route("/api/race/force-finish", post(routes::race::force_finish))
        // Seed demo data
        .route("/api/seed-demo", post(routes::seed::seed_demo))
        // Track ingest v2
        .route("/api/ingest/batch", post(routes::ingest::ingest_batch))
        // Prometheus metrics
        .route("/metrics", get(metrics::metrics_handler));

    let router = if enable_dev_ingest {
        router
            // Dev ingest + replay
            .route(
                "/api/dev/ingest/batch",
                post(routes::dev_ingest::ingest_batch),
            )
            .route(
                "/api/dev/ingest/messages",
                get(routes::dev_ingest::list_messages),
            )
            .route("/api/dev/ingest/replay", post(routes::dev_ingest::replay))
    } else {
        router
    };

    router
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::metrics::AppMetrics;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use p3_parser::Message;
    use std::sync::Arc;
    use tokio::sync::broadcast;
    use tower::util::ServiceExt;

    async fn test_state(enable_dev_ingest: bool) -> AppState {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::run_migrations(&db).await.unwrap();
        let (message_tx, _) = broadcast::channel::<Arc<Message>>(32);

        AppState::new(
            message_tx,
            db,
            None,
            "nats://127.0.0.1:4222".to_string(),
            enable_dev_ingest,
            auth::TrackAuthConfig::disabled(),
            Arc::new(AppMetrics::new()),
        )
    }

    #[tokio::test]
    async fn dev_ingest_batch_route_returns_404_when_disabled() {
        let app = router(test_state(false).await);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/dev/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dev_ingest_batch_route_exists_when_enabled() {
        let app = router(test_state(true).await);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/dev/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metrics_route_returns_prometheus_text() {
        let app = router(test_state(false).await);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("ingest_requests_total"));
    }
}
