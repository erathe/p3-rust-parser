use p3_contracts::{
    FinishResultV1, RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1, RaceEventEnvelopeV1,
    RaceEventPayloadV1, RiderPositionV1, StagedRiderV1,
};
use p3_server::db;
use p3_server::db::queries::race_projection::{
    ProcessOutcome, ProjectedRaceState, get_race_state_projection, project_race_event,
};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use uuid::Uuid;

struct TrackFixture {
    track_id: &'static str,
    event_id: &'static str,
    class_id: &'static str,
    moto_id: &'static str,
    riders: Vec<StagedRiderV1>,
}

#[tokio::test]
async fn projection_isolates_three_tracks_with_interleaved_events() -> anyhow::Result<()> {
    let pool = test_pool().await?;

    let track_a = fixture("track-a", "event-a", "class-a", "moto-a", 1);
    let track_b = fixture("track-b", "event-b", "class-b", "moto-b", 11);
    let track_c = fixture("track-c", "event-c", "class-c", "moto-c", 21);

    seed_projection_fixture(&pool, &track_a).await?;
    seed_projection_fixture(&pool, &track_b).await?;
    seed_projection_fixture(&pool, &track_c).await?;

    let events = interleaved_events(&track_a, &track_b, &track_c);
    for envelope in &events {
        assert_eq!(
            project_race_event(&pool, envelope).await?,
            ProcessOutcome::Applied
        );
    }

    let state_a = get_race_state_projection(&pool, track_a.track_id)
        .await?
        .expect("projection exists for track-a");
    let state_b = get_race_state_projection(&pool, track_b.track_id)
        .await?
        .expect("projection exists for track-b");
    let state_c = get_race_state_projection(&pool, track_c.track_id)
        .await?
        .expect("projection exists for track-c");

    assert_track_state_isolated(&state_a, &track_a, 2);
    assert_track_state_isolated(&state_b, &track_b, 1);
    assert_track_state_isolated(&state_c, &track_c, 0);

    assert_ne!(state_a.moto_id.as_deref(), Some(track_b.moto_id));
    assert_ne!(state_a.moto_id.as_deref(), Some(track_c.moto_id));
    assert_ne!(state_b.moto_id.as_deref(), Some(track_a.moto_id));
    assert_ne!(state_b.moto_id.as_deref(), Some(track_c.moto_id));
    assert_ne!(state_c.moto_id.as_deref(), Some(track_a.moto_id));
    assert_ne!(state_c.moto_id.as_deref(), Some(track_b.moto_id));

    Ok(())
}

async fn test_pool() -> anyhow::Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    db::run_migrations(&pool).await?;
    Ok(pool)
}

fn fixture(
    track_id: &'static str,
    event_id: &'static str,
    class_id: &'static str,
    moto_id: &'static str,
    transponder_base: u32,
) -> TrackFixture {
    TrackFixture {
        track_id,
        event_id,
        class_id,
        moto_id,
        riders: vec![
            StagedRiderV1 {
                rider_id: format!("{track_id}-r1"),
                first_name: "Rider".to_string(),
                last_name: "One".to_string(),
                plate_number: "11".to_string(),
                transponder_id: transponder_base,
                lane: 1,
            },
            StagedRiderV1 {
                rider_id: format!("{track_id}-r2"),
                first_name: "Rider".to_string(),
                last_name: "Two".to_string(),
                plate_number: "22".to_string(),
                transponder_id: transponder_base + 1,
                lane: 2,
            },
        ],
    }
}

async fn seed_projection_fixture(pool: &SqlitePool, fixture: &TrackFixture) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO tracks (id, name, hill_type, gate_beacon_id) VALUES (?, ?, '8m', 9992)",
    )
    .bind(fixture.track_id)
    .bind(format!("Track {}", fixture.track_id))
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO events (id, name, date, track_id, status) VALUES (?, ?, '2026-01-01', ?, 'active')",
    )
    .bind(fixture.event_id)
    .bind(format!("Event {}", fixture.event_id))
    .bind(fixture.track_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO event_classes (id, event_id, name, race_format, scoring) VALUES (?, ?, 'Expert', 'motos_main', 'total_points')",
    )
    .bind(fixture.class_id)
    .bind(fixture.event_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO motos (id, event_id, class_id, round_type, round_number, sequence, status) VALUES (?, ?, ?, 'main', 1, 1, 'pending')",
    )
    .bind(fixture.moto_id)
    .bind(fixture.event_id)
    .bind(fixture.class_id)
    .execute(pool)
    .await?;

    for rider in &fixture.riders {
        sqlx::query(
            "INSERT INTO riders (id, first_name, last_name, plate_number, transponder_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&rider.rider_id)
        .bind(&rider.first_name)
        .bind(&rider.last_name)
        .bind(&rider.plate_number)
        .bind(i64::from(rider.transponder_id))
        .execute(pool)
        .await?;

        sqlx::query("INSERT INTO moto_entries (id, moto_id, rider_id, lane) VALUES (?, ?, ?, ?)")
            .bind(format!("entry-{}-{}", fixture.moto_id, rider.rider_id))
            .bind(fixture.moto_id)
            .bind(&rider.rider_id)
            .bind(i64::from(rider.lane))
            .execute(pool)
            .await?;
    }

    Ok(())
}

fn interleaved_events(
    track_a: &TrackFixture,
    track_b: &TrackFixture,
    track_c: &TrackFixture,
) -> Vec<RaceEventEnvelopeV1> {
    vec![
        envelope(
            1,
            track_a.track_id,
            RaceEventPayloadV1::RaceStaged {
                moto_id: track_a.moto_id.to_string(),
                class_name: "Expert".to_string(),
                round_type: "main".to_string(),
                riders: track_a.riders.clone(),
            },
        ),
        envelope(
            2,
            track_b.track_id,
            RaceEventPayloadV1::RaceStaged {
                moto_id: track_b.moto_id.to_string(),
                class_name: "Expert".to_string(),
                round_type: "main".to_string(),
                riders: track_b.riders.clone(),
            },
        ),
        envelope(
            3,
            track_a.track_id,
            RaceEventPayloadV1::GateDrop {
                moto_id: track_a.moto_id.to_string(),
                timestamp_us: 3_000,
            },
        ),
        envelope(
            4,
            track_c.track_id,
            RaceEventPayloadV1::RaceStaged {
                moto_id: track_c.moto_id.to_string(),
                class_name: "Expert".to_string(),
                round_type: "main".to_string(),
                riders: track_c.riders.clone(),
            },
        ),
        envelope(
            5,
            track_b.track_id,
            RaceEventPayloadV1::PositionsUpdate {
                moto_id: track_b.moto_id.to_string(),
                positions: vec![
                    rider_position(&track_b.riders[0], 1, true, Some(1_450_000), Some(0)),
                    rider_position(&track_b.riders[1], 2, false, Some(1_520_000), Some(70_000)),
                ],
            },
        ),
        envelope(
            6,
            track_c.track_id,
            RaceEventPayloadV1::GateDrop {
                moto_id: track_c.moto_id.to_string(),
                timestamp_us: 6_000,
            },
        ),
        envelope(
            7,
            track_b.track_id,
            RaceEventPayloadV1::GateDrop {
                moto_id: track_b.moto_id.to_string(),
                timestamp_us: 7_000,
            },
        ),
        envelope(
            8,
            track_a.track_id,
            RaceEventPayloadV1::PositionsUpdate {
                moto_id: track_a.moto_id.to_string(),
                positions: vec![
                    rider_position(&track_a.riders[0], 1, true, Some(1_400_000), Some(0)),
                    rider_position(&track_a.riders[1], 2, true, Some(1_460_000), Some(60_000)),
                ],
            },
        ),
        envelope(
            9,
            track_c.track_id,
            RaceEventPayloadV1::PositionsUpdate {
                moto_id: track_c.moto_id.to_string(),
                positions: vec![
                    rider_position(&track_c.riders[0], 1, false, Some(1_480_000), Some(0)),
                    rider_position(&track_c.riders[1], 2, false, Some(1_540_000), Some(60_000)),
                ],
            },
        ),
        envelope(
            10,
            track_b.track_id,
            RaceEventPayloadV1::RaceFinished {
                moto_id: track_b.moto_id.to_string(),
                results: vec![
                    finish_result(&track_b.riders[0], 1, Some(1_450_000), Some(0)),
                    finish_result(&track_b.riders[1], 2, Some(1_520_000), Some(70_000)),
                ],
            },
        ),
        envelope(
            11,
            track_a.track_id,
            RaceEventPayloadV1::RaceFinished {
                moto_id: track_a.moto_id.to_string(),
                results: vec![
                    finish_result(&track_a.riders[0], 1, Some(1_400_000), Some(0)),
                    finish_result(&track_a.riders[1], 2, Some(1_460_000), Some(60_000)),
                ],
            },
        ),
        envelope(
            12,
            track_c.track_id,
            RaceEventPayloadV1::RaceFinished {
                moto_id: track_c.moto_id.to_string(),
                results: vec![
                    finish_result(&track_c.riders[0], 1, Some(1_480_000), Some(0)),
                    finish_result(&track_c.riders[1], 2, Some(1_540_000), Some(60_000)),
                ],
            },
        ),
    ]
}

fn envelope(sequence: u128, track_id: &str, payload: RaceEventPayloadV1) -> RaceEventEnvelopeV1 {
    RaceEventEnvelopeV1 {
        event_id: Uuid::from_u128(50_000 + sequence),
        contract_version: RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id: track_id.to_string(),
        source_event_id: Uuid::from_u128(60_000 + sequence),
        ts_us: sequence as u64,
        payload,
    }
}

fn rider_position(
    rider: &StagedRiderV1,
    position: u32,
    finished: bool,
    elapsed_us: Option<u64>,
    gap_to_leader_us: Option<u64>,
) -> RiderPositionV1 {
    RiderPositionV1 {
        rider_id: rider.rider_id.clone(),
        plate_number: rider.plate_number.clone(),
        first_name: rider.first_name.clone(),
        last_name: rider.last_name.clone(),
        lane: rider.lane,
        position,
        last_loop: Some("Finish".to_string()),
        elapsed_us,
        gap_to_leader_us,
        finished,
        dnf: false,
    }
}

fn finish_result(
    rider: &StagedRiderV1,
    position: u32,
    elapsed_us: Option<u64>,
    gap_to_leader_us: Option<u64>,
) -> FinishResultV1 {
    FinishResultV1 {
        rider_id: rider.rider_id.clone(),
        plate_number: rider.plate_number.clone(),
        first_name: rider.first_name.clone(),
        last_name: rider.last_name.clone(),
        position,
        elapsed_us,
        gap_to_leader_us,
        dnf: false,
        dns: false,
    }
}

fn assert_track_state_isolated(
    state: &ProjectedRaceState,
    fixture: &TrackFixture,
    expected_finished_count: u32,
) {
    assert_eq!(state.track_id, fixture.track_id);
    assert_eq!(state.phase, "finished");
    assert_eq!(state.moto_id.as_deref(), Some(fixture.moto_id));
    assert_eq!(state.finished_count, expected_finished_count);
    assert_eq!(state.total_riders, fixture.riders.len() as u32);

    for rider in &state.riders {
        assert!(rider.rider_id.starts_with(fixture.track_id));
    }
    for position in &state.positions {
        assert!(position.rider_id.starts_with(fixture.track_id));
    }
    for result in &state.results {
        assert!(result.rider_id.starts_with(fixture.track_id));
    }
}
