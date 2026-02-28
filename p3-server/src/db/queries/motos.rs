use sqlx::SqlitePool;

use crate::db::models::{MotoEntryRow, MotoRow};

pub async fn list_motos_for_event(
    pool: &SqlitePool,
    event_id: &str,
) -> Result<Vec<MotoRow>, sqlx::Error> {
    sqlx::query_as::<_, MotoRow>("SELECT * FROM motos WHERE event_id = ? ORDER BY sequence")
        .bind(event_id)
        .fetch_all(pool)
        .await
}

pub async fn list_motos_for_class(
    pool: &SqlitePool,
    class_id: &str,
) -> Result<Vec<MotoRow>, sqlx::Error> {
    sqlx::query_as::<_, MotoRow>("SELECT * FROM motos WHERE class_id = ? ORDER BY sequence")
        .bind(class_id)
        .fetch_all(pool)
        .await
}

pub async fn get_moto(pool: &SqlitePool, id: &str) -> Result<Option<MotoRow>, sqlx::Error> {
    sqlx::query_as::<_, MotoRow>("SELECT * FROM motos WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn create_moto(
    pool: &SqlitePool,
    id: &str,
    event_id: &str,
    class_id: &str,
    round_type: &str,
    round_number: Option<i64>,
    sequence: i64,
) -> Result<MotoRow, sqlx::Error> {
    sqlx::query(
        "INSERT INTO motos (id, event_id, class_id, round_type, round_number, sequence) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(event_id)
    .bind(class_id)
    .bind(round_type)
    .bind(round_number)
    .bind(sequence)
    .execute(pool)
    .await?;

    get_moto(pool, id).await?.ok_or(sqlx::Error::RowNotFound)
}

pub async fn delete_motos_for_class(pool: &SqlitePool, class_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM motos WHERE class_id = ?")
        .bind(class_id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Moto Entries ---

pub async fn list_entries(
    pool: &SqlitePool,
    moto_id: &str,
) -> Result<Vec<MotoEntryRow>, sqlx::Error> {
    sqlx::query_as::<_, MotoEntryRow>("SELECT * FROM moto_entries WHERE moto_id = ? ORDER BY lane")
        .bind(moto_id)
        .fetch_all(pool)
        .await
}

pub async fn create_entry(
    pool: &SqlitePool,
    id: &str,
    moto_id: &str,
    rider_id: &str,
    lane: i64,
) -> Result<MotoEntryRow, sqlx::Error> {
    sqlx::query("INSERT INTO moto_entries (id, moto_id, rider_id, lane) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(moto_id)
        .bind(rider_id)
        .bind(lane)
        .execute(pool)
        .await?;

    sqlx::query_as::<_, MotoEntryRow>("SELECT * FROM moto_entries WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
}
