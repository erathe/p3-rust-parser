use sqlx::SqlitePool;
use uuid::Uuid;

use crate::db::models::{TimingLoopRow, TrackRow, TrackSectionRow};

pub async fn list_tracks(pool: &SqlitePool) -> sqlx::Result<Vec<TrackRow>> {
    sqlx::query_as::<_, TrackRow>("SELECT * FROM tracks ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn get_track(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<TrackRow>> {
    sqlx::query_as::<_, TrackRow>("SELECT * FROM tracks WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn create_track(
    pool: &SqlitePool,
    name: &str,
    hill_type: &str,
    gate_beacon_id: i64,
) -> sqlx::Result<TrackRow> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO tracks (id, name, hill_type, gate_beacon_id) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(hill_type)
    .bind(gate_beacon_id)
    .execute(pool)
    .await?;

    // unwrap is safe: we just inserted it
    get_track(pool, &id).await.map(|t| t.unwrap())
}

pub async fn update_track(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    hill_type: &str,
    gate_beacon_id: i64,
) -> sqlx::Result<Option<TrackRow>> {
    let result = sqlx::query(
        "UPDATE tracks SET name = ?, hill_type = ?, gate_beacon_id = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(name)
    .bind(hill_type)
    .bind(gate_beacon_id)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    get_track(pool, id).await
}

pub async fn delete_track(pool: &SqlitePool, id: &str) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM tracks WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// Timing loops

pub async fn get_loops_for_track(
    pool: &SqlitePool,
    track_id: &str,
) -> sqlx::Result<Vec<TimingLoopRow>> {
    sqlx::query_as::<_, TimingLoopRow>(
        "SELECT * FROM timing_loops WHERE track_id = ? ORDER BY position",
    )
    .bind(track_id)
    .fetch_all(pool)
    .await
}

pub async fn create_timing_loop(
    pool: &SqlitePool,
    track_id: &str,
    name: &str,
    decoder_id: &str,
    position: i64,
    is_finish: bool,
    is_start: bool,
) -> sqlx::Result<TimingLoopRow> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO timing_loops (id, track_id, name, decoder_id, position, is_finish, is_start) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(track_id)
    .bind(name)
    .bind(decoder_id)
    .bind(position)
    .bind(is_finish)
    .bind(is_start)
    .execute(pool)
    .await?;

    Ok(sqlx::query_as::<_, TimingLoopRow>("SELECT * FROM timing_loops WHERE id = ?")
        .bind(&id)
        .fetch_one(pool)
        .await?)
}

pub async fn update_timing_loop(
    pool: &SqlitePool,
    loop_id: &str,
    name: &str,
    decoder_id: &str,
    position: i64,
    is_finish: bool,
    is_start: bool,
) -> sqlx::Result<Option<TimingLoopRow>> {
    let result = sqlx::query(
        "UPDATE timing_loops SET name = ?, decoder_id = ?, position = ?, is_finish = ?, is_start = ? WHERE id = ?",
    )
    .bind(name)
    .bind(decoder_id)
    .bind(position)
    .bind(is_finish)
    .bind(is_start)
    .bind(loop_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    Ok(Some(
        sqlx::query_as::<_, TimingLoopRow>("SELECT * FROM timing_loops WHERE id = ?")
            .bind(loop_id)
            .fetch_one(pool)
            .await?,
    ))
}

pub async fn delete_timing_loop(pool: &SqlitePool, loop_id: &str) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM timing_loops WHERE id = ?")
        .bind(loop_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// Track sections

pub async fn get_sections_for_track(
    pool: &SqlitePool,
    track_id: &str,
) -> sqlx::Result<Vec<TrackSectionRow>> {
    sqlx::query_as::<_, TrackSectionRow>(
        "SELECT * FROM track_sections WHERE track_id = ? ORDER BY position",
    )
    .bind(track_id)
    .fetch_all(pool)
    .await
}

pub struct NewSection {
    pub name: String,
    pub section_type: String,
    pub length_m: f64,
    pub position: i64,
    pub loop_id: Option<String>,
}

pub async fn replace_all_sections(
    pool: &SqlitePool,
    track_id: &str,
    sections: Vec<NewSection>,
) -> sqlx::Result<Vec<TrackSectionRow>> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM track_sections WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    for section in &sections {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO track_sections (id, track_id, name, section_type, length_m, position, loop_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(track_id)
        .bind(&section.name)
        .bind(&section.section_type)
        .bind(section.length_m)
        .bind(section.position)
        .bind(section.loop_id.as_deref())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    get_sections_for_track(pool, track_id).await
}
