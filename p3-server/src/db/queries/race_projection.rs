use anyhow::{Context, anyhow};
use p3_contracts::{
    FinishResultV1, RaceEventEnvelopeV1, RaceEventPayloadV1, RiderPositionV1, StagedRiderV1,
};
use sqlx::{Sqlite, SqlitePool, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessOutcome {
    Applied,
    Duplicate,
}

#[derive(Debug, Clone)]
pub struct ProjectedRaceState {
    pub track_id: String,
    pub phase: String,
    pub moto_id: Option<String>,
    pub class_name: Option<String>,
    pub round_type: Option<String>,
    pub riders: Vec<StagedRiderV1>,
    pub positions: Vec<RiderPositionV1>,
    pub gate_drop_time_us: Option<u64>,
    pub finished_count: u32,
    pub total_riders: u32,
    pub results: Vec<FinishResultV1>,
    pub updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct RaceProjectionRow {
    track_id: String,
    phase: String,
    moto_id: Option<String>,
    class_name: Option<String>,
    round_type: Option<String>,
    riders_json: String,
    positions_json: String,
    gate_drop_time_us: Option<i64>,
    finished_count: i64,
    total_riders: i64,
    results_json: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct MutableProjectionState {
    phase: String,
    moto_id: Option<String>,
    class_name: Option<String>,
    round_type: Option<String>,
    riders: Vec<StagedRiderV1>,
    positions: Vec<RiderPositionV1>,
    gate_drop_time_us: Option<u64>,
    finished_count: u32,
    total_riders: u32,
    results: Vec<FinishResultV1>,
}

impl MutableProjectionState {
    fn idle() -> Self {
        Self {
            phase: "idle".to_string(),
            moto_id: None,
            class_name: None,
            round_type: None,
            riders: Vec::new(),
            positions: Vec::new(),
            gate_drop_time_us: None,
            finished_count: 0,
            total_riders: 0,
            results: Vec::new(),
        }
    }
}

pub async fn project_race_event(
    pool: &SqlitePool,
    envelope: &RaceEventEnvelopeV1,
) -> anyhow::Result<ProcessOutcome> {
    let mut tx = pool.begin().await?;

    let dedupe = sqlx::query(
        "INSERT INTO race_projection_dedupe (event_id) VALUES (?) \
         ON CONFLICT(event_id) DO NOTHING",
    )
    .bind(envelope.event_id.to_string())
    .execute(&mut *tx)
    .await?;

    if dedupe.rows_affected() == 0 {
        return Ok(ProcessOutcome::Duplicate);
    }

    let maybe_current = load_projection_state(&mut tx, &envelope.track_id).await?;
    let mut state = maybe_current.unwrap_or_else(MutableProjectionState::idle);

    apply_payload_side_effects(&mut tx, &envelope.payload).await?;

    let should_persist = apply_payload(&mut state, &envelope.payload);
    if should_persist {
        persist_projection_state(&mut tx, &envelope.track_id, &state).await?;
    }

    tx.commit().await?;
    Ok(ProcessOutcome::Applied)
}

async fn apply_payload_side_effects(
    tx: &mut Transaction<'_, Sqlite>,
    payload: &RaceEventPayloadV1,
) -> anyhow::Result<()> {
    match payload {
        RaceEventPayloadV1::RaceStaged { moto_id, .. } => {
            set_moto_status(tx, moto_id, "staged").await?;
        }
        RaceEventPayloadV1::GateDrop { moto_id, .. } => {
            set_moto_status(tx, moto_id, "racing").await?;
        }
        RaceEventPayloadV1::RaceFinished { moto_id, results } => {
            set_moto_status(tx, moto_id, "finished").await?;
            persist_moto_results(tx, moto_id, results).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn set_moto_status(
    tx: &mut Transaction<'_, Sqlite>,
    moto_id: &str,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE motos SET status = ? WHERE id = ?")
        .bind(status)
        .bind(moto_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn persist_moto_results(
    tx: &mut Transaction<'_, Sqlite>,
    moto_id: &str,
    results: &[FinishResultV1],
) -> anyhow::Result<()> {
    for result in results {
        let points = if result.dnf {
            i64::try_from(results.len())
                .unwrap_or(i64::MAX)
                .saturating_add(1)
        } else {
            i64::from(result.position)
        };

        sqlx::query(
            "UPDATE moto_entries SET \
             finish_position = ?, \
             elapsed_us = ?, \
             points = ?, \
             dnf = ?, \
             dns = ? \
             WHERE moto_id = ? AND rider_id = ?",
        )
        .bind(if result.dnf {
            None
        } else {
            Some(i64::from(result.position))
        })
        .bind(result.elapsed_us.map(i64::try_from).transpose()?)
        .bind(points)
        .bind(result.dnf)
        .bind(result.dns)
        .bind(moto_id)
        .bind(&result.rider_id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

pub async fn get_race_state_projection(
    pool: &SqlitePool,
    track_id: &str,
) -> anyhow::Result<Option<ProjectedRaceState>> {
    let row = sqlx::query_as::<_, RaceProjectionRow>(
        "SELECT track_id, phase, moto_id, class_name, round_type, riders_json, positions_json, \
            gate_drop_time_us, finished_count, total_riders, results_json, updated_at \
         FROM race_state_projection \
         WHERE track_id = ?",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await?;

    row.map(to_projected_state).transpose()
}

fn apply_payload(state: &mut MutableProjectionState, payload: &RaceEventPayloadV1) -> bool {
    match payload {
        RaceEventPayloadV1::RaceStaged {
            moto_id,
            class_name,
            round_type,
            riders,
        } => {
            state.phase = "staged".to_string();
            state.moto_id = Some(moto_id.clone());
            state.class_name = Some(class_name.clone());
            state.round_type = Some(round_type.clone());
            state.riders = riders.clone();
            state.positions.clear();
            state.gate_drop_time_us = None;
            state.finished_count = 0;
            state.total_riders = riders.len() as u32;
            state.results.clear();
            true
        }
        RaceEventPayloadV1::GateDrop {
            moto_id,
            timestamp_us,
        } => {
            state.phase = "racing".to_string();
            state.moto_id = Some(moto_id.clone());
            state.gate_drop_time_us = Some(*timestamp_us);
            true
        }
        RaceEventPayloadV1::PositionsUpdate { moto_id, positions } => {
            state.moto_id = Some(moto_id.clone());
            state.positions = positions.clone();
            state.finished_count = state.positions.iter().filter(|row| row.finished).count() as u32;
            true
        }
        RaceEventPayloadV1::RaceFinished { moto_id, results } => {
            state.phase = "finished".to_string();
            state.moto_id = Some(moto_id.clone());
            state.results = results.clone();
            true
        }
        RaceEventPayloadV1::RaceReset => {
            *state = MutableProjectionState::idle();
            true
        }
        RaceEventPayloadV1::StateSnapshot {
            phase,
            moto_id,
            class_name,
            round_type,
            riders,
            positions,
            gate_drop_time_us,
            finished_count,
            total_riders,
        } => {
            let existing_results = state.results.clone();
            state.phase = phase.clone();
            state.moto_id = moto_id.clone();
            state.class_name = class_name.clone();
            state.round_type = round_type.clone();
            state.riders = riders.clone();
            state.positions = positions.clone();
            state.gate_drop_time_us = *gate_drop_time_us;
            state.finished_count = *finished_count;
            state.total_riders = *total_riders;
            if phase == "idle" {
                state.results.clear();
            } else {
                state.results = existing_results;
            }
            true
        }
        RaceEventPayloadV1::SplitTime { .. }
        | RaceEventPayloadV1::RiderFinished { .. }
        | RaceEventPayloadV1::DecoderMessage { .. } => false,
    }
}

async fn load_projection_state(
    tx: &mut Transaction<'_, Sqlite>,
    track_id: &str,
) -> anyhow::Result<Option<MutableProjectionState>> {
    let row = sqlx::query_as::<_, RaceProjectionRow>(
        "SELECT track_id, phase, moto_id, class_name, round_type, riders_json, positions_json, \
            gate_drop_time_us, finished_count, total_riders, results_json, updated_at \
         FROM race_state_projection \
         WHERE track_id = ?",
    )
    .bind(track_id)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(|row| {
        Ok(MutableProjectionState {
            phase: row.phase,
            moto_id: row.moto_id,
            class_name: row.class_name,
            round_type: row.round_type,
            riders: serde_json::from_str(&row.riders_json).with_context(|| {
                format!("failed to decode riders_json for track {}", row.track_id)
            })?,
            positions: serde_json::from_str(&row.positions_json).with_context(|| {
                format!("failed to decode positions_json for track {}", row.track_id)
            })?,
            gate_drop_time_us: row
                .gate_drop_time_us
                .map(|value| {
                    u64::try_from(value).map_err(|_| anyhow!("negative gate_drop_time_us"))
                })
                .transpose()?,
            finished_count: u32::try_from(row.finished_count)
                .map_err(|_| anyhow!("negative finished_count in projection row"))?,
            total_riders: u32::try_from(row.total_riders)
                .map_err(|_| anyhow!("negative total_riders in projection row"))?,
            results: serde_json::from_str(&row.results_json).with_context(|| {
                format!("failed to decode results_json for track {}", row.track_id)
            })?,
        })
    })
    .transpose()
}

async fn persist_projection_state(
    tx: &mut Transaction<'_, Sqlite>,
    track_id: &str,
    state: &MutableProjectionState,
) -> anyhow::Result<()> {
    let riders_json = serde_json::to_string(&state.riders)?;
    let positions_json = serde_json::to_string(&state.positions)?;
    let results_json = serde_json::to_string(&state.results)?;

    sqlx::query(
        "INSERT INTO race_state_projection \
            (track_id, phase, moto_id, class_name, round_type, riders_json, positions_json, gate_drop_time_us, \
             finished_count, total_riders, results_json, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(track_id) DO UPDATE SET \
            phase = excluded.phase, \
            moto_id = excluded.moto_id, \
            class_name = excluded.class_name, \
            round_type = excluded.round_type, \
            riders_json = excluded.riders_json, \
            positions_json = excluded.positions_json, \
            gate_drop_time_us = excluded.gate_drop_time_us, \
            finished_count = excluded.finished_count, \
            total_riders = excluded.total_riders, \
            results_json = excluded.results_json, \
            updated_at = datetime('now')",
    )
    .bind(track_id)
    .bind(&state.phase)
    .bind(&state.moto_id)
    .bind(&state.class_name)
    .bind(&state.round_type)
    .bind(riders_json)
    .bind(positions_json)
    .bind(state.gate_drop_time_us.map(i64::try_from).transpose()?)
    .bind(i64::from(state.finished_count))
    .bind(i64::from(state.total_riders))
    .bind(results_json)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn to_projected_state(row: RaceProjectionRow) -> anyhow::Result<ProjectedRaceState> {
    Ok(ProjectedRaceState {
        track_id: row.track_id,
        phase: row.phase,
        moto_id: row.moto_id,
        class_name: row.class_name,
        round_type: row.round_type,
        riders: serde_json::from_str(&row.riders_json).context("failed to decode riders_json")?,
        positions: serde_json::from_str(&row.positions_json)
            .context("failed to decode positions_json")?,
        gate_drop_time_us: row
            .gate_drop_time_us
            .map(|value| u64::try_from(value).map_err(|_| anyhow!("negative gate_drop_time_us")))
            .transpose()?,
        finished_count: u32::try_from(row.finished_count)
            .map_err(|_| anyhow!("negative finished_count in projection row"))?,
        total_riders: u32::try_from(row.total_riders)
            .map_err(|_| anyhow!("negative total_riders in projection row"))?,
        results: serde_json::from_str(&row.results_json)
            .context("failed to decode results_json")?,
        updated_at: row.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use p3_contracts::{
        RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1, RaceEventEnvelopeV1, RaceEventPayloadV1,
        RiderPositionV1, StagedRiderV1,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    #[tokio::test]
    async fn replay_idempotency_skips_duplicate_event() {
        let pool = test_pool().await;
        let staged = race_staged_envelope("track-a", "moto-1", 1);

        let first = project_race_event(&pool, &staged).await.unwrap();
        let state_after_first = get_race_state_projection(&pool, "track-a")
            .await
            .unwrap()
            .unwrap();

        let second = project_race_event(&pool, &staged).await.unwrap();
        let state_after_second = get_race_state_projection(&pool, "track-a")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(first, ProcessOutcome::Applied);
        assert_eq!(second, ProcessOutcome::Duplicate);
        assert_eq!(state_after_first.track_id, state_after_second.track_id);
        assert_eq!(state_after_first.phase, state_after_second.phase);
        assert_eq!(state_after_first.moto_id, state_after_second.moto_id);
        assert_eq!(state_after_first.class_name, state_after_second.class_name);
        assert_eq!(state_after_first.round_type, state_after_second.round_type);
        assert_eq!(
            state_after_first.finished_count,
            state_after_second.finished_count
        );
        assert_eq!(
            state_after_first.total_riders,
            state_after_second.total_riders
        );
        assert_eq!(state_after_first.updated_at, state_after_second.updated_at);
        assert_eq!(
            serde_json::to_string(&state_after_first.riders).unwrap(),
            serde_json::to_string(&state_after_second.riders).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&state_after_first.positions).unwrap(),
            serde_json::to_string(&state_after_second.positions).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&state_after_first.results).unwrap(),
            serde_json::to_string(&state_after_second.results).unwrap()
        );
    }

    #[tokio::test]
    async fn interleaved_tracks_remain_isolated() {
        let pool = test_pool().await;

        let staged_a = race_staged_envelope("track-a", "moto-a", 2);
        let staged_b = race_staged_envelope("track-b", "moto-b", 3);
        let positions_a = RaceEventEnvelopeV1 {
            event_id: Uuid::new_v4(),
            contract_version: RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
            track_id: "track-a".to_string(),
            source_event_id: Uuid::new_v4(),
            ts_us: 100,
            payload: RaceEventPayloadV1::PositionsUpdate {
                moto_id: "moto-a".to_string(),
                positions: vec![RiderPositionV1 {
                    rider_id: "r-a1".to_string(),
                    plate_number: "11".to_string(),
                    first_name: "Ava".to_string(),
                    last_name: "Rider".to_string(),
                    lane: 1,
                    position: 1,
                    last_loop: Some("Finish".to_string()),
                    elapsed_us: Some(1_500_000),
                    gap_to_leader_us: Some(0),
                    finished: true,
                    dnf: false,
                }],
            },
        };

        project_race_event(&pool, &staged_a).await.unwrap();
        project_race_event(&pool, &staged_b).await.unwrap();
        project_race_event(&pool, &positions_a).await.unwrap();

        let track_a = get_race_state_projection(&pool, "track-a")
            .await
            .unwrap()
            .unwrap();
        let track_b = get_race_state_projection(&pool, "track-b")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(track_a.moto_id.as_deref(), Some("moto-a"));
        assert_eq!(track_a.positions.len(), 1);
        assert_eq!(track_a.finished_count, 1);

        assert_eq!(track_b.moto_id.as_deref(), Some("moto-b"));
        assert!(track_b.positions.is_empty());
        assert_eq!(track_b.finished_count, 0);
    }

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db::run_migrations(&pool).await.unwrap();
        pool
    }

    fn race_staged_envelope(
        track_id: &str,
        moto_id: &str,
        rider_count: usize,
    ) -> RaceEventEnvelopeV1 {
        let riders = (0..rider_count)
            .map(|idx| StagedRiderV1 {
                rider_id: format!("{track_id}-rider-{idx}"),
                first_name: format!("R{idx}"),
                last_name: "Test".to_string(),
                plate_number: format!("{}", idx + 1),
                transponder_id: (idx + 100) as u32,
                lane: (idx + 1) as u32,
            })
            .collect();

        RaceEventEnvelopeV1 {
            event_id: Uuid::new_v4(),
            contract_version: RACE_EVENTS_ENVELOPE_CONTRACT_VERSION_V1.to_string(),
            track_id: track_id.to_string(),
            source_event_id: Uuid::new_v4(),
            ts_us: 0,
            payload: RaceEventPayloadV1::RaceStaged {
                moto_id: moto_id.to_string(),
                class_name: "Expert".to_string(),
                round_type: "main".to_string(),
                riders,
            },
        }
    }
}
