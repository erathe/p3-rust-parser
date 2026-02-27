pub mod error;
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
    Router::new()
        // WebSocket
        .route("/ws", get(ws::ws_handler))
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
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
