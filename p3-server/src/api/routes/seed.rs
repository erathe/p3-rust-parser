use axum::{Json, extract::State};
use serde::Serialize;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::queries::{events as event_queries, motos as moto_queries};
use crate::domain::race_format;

/// Decoder IDs matching the test-server full-race scenario.
/// These are formatted as the parser outputs them (LE byte order hex).
/// Note: Gate decoder (D0000C00) is not a timing loop — the gate is detected
/// by the reserved transponder ID (9992), not by decoder_id.
const DECODER_START_HILL: &str = "D1000C00";
const DECODER_CORNER1: &str = "D2000C00";
const DECODER_FINISH: &str = "D3000C00";

#[derive(Debug, Serialize)]
pub struct SeedResult {
    pub track_id: String,
    pub event_id: String,
    pub class_id: String,
    pub riders_created: usize,
    pub motos_created: usize,
}

/// POST /api/seed-demo — Create demo data matching the test-server full-race scenario
pub async fn seed_demo(State(state): State<AppState>) -> Result<Json<SeedResult>, ApiError> {
    let db = &state.db;

    // Check if demo track already exists
    let existing =
        sqlx::query_scalar::<_, String>("SELECT id FROM tracks WHERE name = 'Demo BMX Track'")
            .fetch_optional(db)
            .await?;

    if let Some(track_id) = existing {
        // Already seeded — return existing IDs
        let event_id =
            sqlx::query_scalar::<_, String>("SELECT id FROM events WHERE track_id = ? LIMIT 1")
                .bind(&track_id)
                .fetch_optional(db)
                .await?
                .unwrap_or_default();

        let class_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM event_classes WHERE event_id = ? LIMIT 1",
        )
        .bind(&event_id)
        .fetch_optional(db)
        .await?
        .unwrap_or_default();

        return Ok(Json(SeedResult {
            track_id,
            event_id,
            class_id,
            riders_created: 0,
            motos_created: 0,
        }));
    }

    // --- Create track ---
    let track_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO tracks (id, name, hill_type, gate_beacon_id) VALUES (?, 'Demo BMX Track', '8m', 9992)",
    )
    .bind(&track_id)
    .execute(db)
    .await?;

    // --- Create timing loops (matching test-server decoder IDs) ---
    let loops = [
        ("Start Hill", DECODER_START_HILL, 0, true, false),
        ("Corner 1", DECODER_CORNER1, 1, false, false),
        ("Finish", DECODER_FINISH, 2, false, true),
    ];

    for (name, decoder_id, position, is_start, is_finish) in &loops {
        let loop_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO timing_loops (id, track_id, name, decoder_id, position, is_start, is_finish) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&loop_id)
        .bind(&track_id)
        .bind(name)
        .bind(decoder_id)
        .bind(position)
        .bind(is_start)
        .bind(is_finish)
        .execute(db)
        .await?;
    }

    // --- Create riders (matching test-server transponder IDs) ---
    let rider_data = [
        (1001, "Alex", "Racer", "101"),
        (1002, "Blake", "Speed", "102"),
        (1003, "Casey", "Dash", "103"),
        (1004, "Dana", "Surge", "104"),
        (1005, "Ellis", "Storm", "105"),
        (1006, "Finn", "Blaze", "106"),
        (1007, "Gray", "Flash", "107"),
        (1008, "Harper", "Bolt", "108"),
    ];

    let mut rider_ids = Vec::new();
    for (transponder_id, first_name, last_name, plate_number) in &rider_data {
        // Check if rider with this transponder already exists
        let existing =
            sqlx::query_scalar::<_, String>("SELECT id FROM riders WHERE transponder_id = ?")
                .bind(transponder_id)
                .fetch_optional(db)
                .await?;

        let rider_id = if let Some(id) = existing {
            id
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO riders (id, first_name, last_name, plate_number, transponder_id) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(first_name)
            .bind(last_name)
            .bind(plate_number)
            .bind(transponder_id)
            .execute(db)
            .await?;
            id
        };

        rider_ids.push(rider_id);
    }

    // --- Create event ---
    let event_id = uuid::Uuid::new_v4().to_string();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    sqlx::query("INSERT INTO events (id, name, date, track_id, status) VALUES (?, 'Demo Race Day', ?, ?, 'active')")
        .bind(&event_id)
        .bind(&today)
        .bind(&track_id)
        .execute(db)
        .await?;

    // --- Create class ---
    let class_id = uuid::Uuid::new_v4().to_string();
    event_queries::create_class(
        db,
        &class_id,
        &event_id,
        "Open BMX",
        None,
        None,
        None,
        None,
        "motos_only",
        "total_points",
    )
    .await?;

    // --- Add riders to class ---
    for rider_id in &rider_ids {
        event_queries::add_rider_to_class(db, &class_id, rider_id).await?;
    }

    // --- Generate motos ---
    let format = race_format::determine_format(rider_ids.len());
    let qualifying = race_format::generate_qualifying_motos(&rider_ids);
    let last_qual_seq = qualifying.last().map(|m| m.sequence).unwrap_or(0);
    let elimination = race_format::generate_elimination_motos(&format, last_qual_seq + 1);
    let total_motos = qualifying.len() + elimination.len();

    for assignment in qualifying.iter().chain(elimination.iter()) {
        let moto_id = uuid::Uuid::new_v4().to_string();
        moto_queries::create_moto(
            db,
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
            moto_queries::create_entry(db, &entry_id, &moto_id, rider_id, *lane).await?;
        }
    }

    Ok(Json(SeedResult {
        track_id,
        event_id,
        class_id,
        riders_created: rider_ids.len(),
        motos_created: total_motos,
    }))
}
