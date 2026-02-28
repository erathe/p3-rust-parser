use p3_contracts::{
    FinishResultV1, RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1, RaceEventEnvelopeV1,
    RaceEventPayloadV1, RiderPositionV1, StagedRiderV1,
};
use p3_server::db;
use p3_server::db::queries::race_projection::{
    ProcessOutcome, get_race_state_projection, project_race_event,
};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use uuid::Uuid;

#[tokio::test]
async fn deterministic_replay_produces_identical_state_and_side_effects() -> anyhow::Result<()> {
    let db_one = test_pool().await?;
    let db_two = test_pool().await?;

    let track_id = "track-a";
    let event_id = "event-a";
    let class_id = "class-a";
    let moto_id = "moto-a";
    let riders = staged_riders(track_id);

    seed_projection_fixture(&db_one, track_id, event_id, class_id, moto_id, &riders).await?;
    seed_projection_fixture(&db_two, track_id, event_id, class_id, moto_id, &riders).await?;

    let sequence = deterministic_sequence(track_id, moto_id, &riders);
    for envelope in &sequence {
        assert_eq!(
            project_race_event(&db_one, envelope).await?,
            ProcessOutcome::Applied
        );
    }
    for envelope in &sequence {
        assert_eq!(
            project_race_event(&db_two, envelope).await?,
            ProcessOutcome::Applied
        );
    }

    let state_one = get_race_state_projection(&db_one, track_id)
        .await?
        .expect("projection exists in db one");
    let state_two = get_race_state_projection(&db_two, track_id)
        .await?
        .expect("projection exists in db two");

    assert_eq!(state_one.phase, state_two.phase);
    assert_eq!(state_one.moto_id, state_two.moto_id);
    assert_eq!(state_one.class_name, state_two.class_name);
    assert_eq!(state_one.round_type, state_two.round_type);
    assert_eq!(state_one.finished_count, state_two.finished_count);
    assert_eq!(state_one.total_riders, state_two.total_riders);
    assert_eq!(
        serde_json::to_string(&state_one.riders)?,
        serde_json::to_string(&state_two.riders)?
    );
    assert_eq!(
        serde_json::to_string(&state_one.positions)?,
        serde_json::to_string(&state_two.positions)?
    );
    assert_eq!(
        serde_json::to_string(&state_one.results)?,
        serde_json::to_string(&state_two.results)?
    );

    let status_one: String = sqlx::query_scalar("SELECT status FROM motos WHERE id = ?")
        .bind(moto_id)
        .fetch_one(&db_one)
        .await?;
    let status_two: String = sqlx::query_scalar("SELECT status FROM motos WHERE id = ?")
        .bind(moto_id)
        .fetch_one(&db_two)
        .await?;
    assert_eq!(status_one, status_two);

    let entries_one = fetch_entry_results(&db_one, moto_id).await?;
    let entries_two = fetch_entry_results(&db_two, moto_id).await?;
    assert_eq!(entries_one, entries_two);

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

fn staged_riders(track_id: &str) -> Vec<StagedRiderV1> {
    vec![
        StagedRiderV1 {
            rider_id: format!("{track_id}-r1"),
            first_name: "Ava".to_string(),
            last_name: "Lane".to_string(),
            plate_number: "11".to_string(),
            transponder_id: 1001,
            lane: 1,
        },
        StagedRiderV1 {
            rider_id: format!("{track_id}-r2"),
            first_name: "Ben".to_string(),
            last_name: "Gate".to_string(),
            plate_number: "22".to_string(),
            transponder_id: 1002,
            lane: 2,
        },
        StagedRiderV1 {
            rider_id: format!("{track_id}-r3"),
            first_name: "Cy".to_string(),
            last_name: "Finish".to_string(),
            plate_number: "33".to_string(),
            transponder_id: 1003,
            lane: 3,
        },
    ]
}

async fn seed_projection_fixture(
    pool: &SqlitePool,
    track_id: &str,
    event_id: &str,
    class_id: &str,
    moto_id: &str,
    riders: &[StagedRiderV1],
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO tracks (id, name, hill_type, gate_beacon_id) VALUES (?, ?, '8m', 9992)",
    )
    .bind(track_id)
    .bind(format!("Track {track_id}"))
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO events (id, name, date, track_id, status) VALUES (?, ?, '2026-01-01', ?, 'active')",
    )
    .bind(event_id)
    .bind(format!("Event {event_id}"))
    .bind(track_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO event_classes (id, event_id, name, race_format, scoring) VALUES (?, ?, 'Expert', 'motos_main', 'total_points')",
    )
    .bind(class_id)
    .bind(event_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO motos (id, event_id, class_id, round_type, round_number, sequence, status) VALUES (?, ?, ?, 'main', 1, 1, 'pending')",
    )
    .bind(moto_id)
    .bind(event_id)
    .bind(class_id)
    .execute(pool)
    .await?;

    for rider in riders {
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
            .bind(format!("entry-{moto_id}-{}", rider.rider_id))
            .bind(moto_id)
            .bind(&rider.rider_id)
            .bind(i64::from(rider.lane))
            .execute(pool)
            .await?;
    }

    Ok(())
}

fn deterministic_sequence(
    track_id: &str,
    moto_id: &str,
    riders: &[StagedRiderV1],
) -> Vec<RaceEventEnvelopeV1> {
    let staged = RaceEventPayloadV1::RaceStaged {
        moto_id: moto_id.to_string(),
        class_name: "Expert".to_string(),
        round_type: "main".to_string(),
        riders: riders.to_vec(),
    };

    let first_positions = RaceEventPayloadV1::PositionsUpdate {
        moto_id: moto_id.to_string(),
        positions: vec![
            rider_position(&riders[0], 1, true, Some(1_500_000), Some(0)),
            rider_position(&riders[1], 2, false, Some(1_560_000), Some(60_000)),
            rider_position(&riders[2], 3, false, None, None),
        ],
    };

    let final_positions = RaceEventPayloadV1::PositionsUpdate {
        moto_id: moto_id.to_string(),
        positions: vec![
            rider_position(&riders[0], 1, true, Some(1_500_000), Some(0)),
            rider_position(&riders[1], 2, true, Some(1_560_000), Some(60_000)),
            rider_position(&riders[2], 3, true, Some(1_620_000), Some(120_000)),
        ],
    };

    let finished = RaceEventPayloadV1::RaceFinished {
        moto_id: moto_id.to_string(),
        results: vec![
            finish_result(&riders[0], 1, Some(1_500_000), Some(0), false, false),
            finish_result(&riders[1], 2, Some(1_560_000), Some(60_000), false, false),
            finish_result(&riders[2], 3, Some(1_620_000), Some(120_000), false, false),
        ],
    };

    vec![
        envelope(1, track_id, 1_000, staged),
        envelope(
            2,
            track_id,
            2_000,
            RaceEventPayloadV1::GateDrop {
                moto_id: moto_id.to_string(),
                timestamp_us: 2_000,
            },
        ),
        envelope(3, track_id, 3_000, first_positions),
        envelope(4, track_id, 4_000, final_positions),
        envelope(5, track_id, 5_000, finished),
    ]
}

fn envelope(
    sequence: u128,
    track_id: &str,
    ts_us: u64,
    payload: RaceEventPayloadV1,
) -> RaceEventEnvelopeV1 {
    RaceEventEnvelopeV1 {
        event_id: Uuid::from_u128(sequence),
        contract_version: RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
        track_id: track_id.to_string(),
        source_event_id: Uuid::from_u128(10_000 + sequence),
        ts_us,
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
    dnf: bool,
    dns: bool,
) -> FinishResultV1 {
    FinishResultV1 {
        rider_id: rider.rider_id.clone(),
        plate_number: rider.plate_number.clone(),
        first_name: rider.first_name.clone(),
        last_name: rider.last_name.clone(),
        position,
        elapsed_us,
        gap_to_leader_us,
        dnf,
        dns,
    }
}

async fn fetch_entry_results(
    pool: &SqlitePool,
    moto_id: &str,
) -> anyhow::Result<Vec<(String, Option<i64>, Option<i64>, Option<i64>, bool, bool)>> {
    let rows = sqlx::query_as::<_, (String, Option<i64>, Option<i64>, Option<i64>, bool, bool)>(
        "SELECT rider_id, finish_position, elapsed_us, points, dnf, dns FROM moto_entries WHERE moto_id = ? ORDER BY rider_id",
    )
    .bind(moto_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
