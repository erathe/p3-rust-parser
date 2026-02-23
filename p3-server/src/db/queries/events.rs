use sqlx::SqlitePool;

use crate::db::models::{EventClassRow, EventRow};

// --- Events ---

pub async fn list_events(pool: &SqlitePool) -> Result<Vec<EventRow>, sqlx::Error> {
    sqlx::query_as::<_, EventRow>("SELECT * FROM events ORDER BY date DESC")
        .fetch_all(pool)
        .await
}

pub async fn get_event(pool: &SqlitePool, id: &str) -> Result<Option<EventRow>, sqlx::Error> {
    sqlx::query_as::<_, EventRow>("SELECT * FROM events WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn create_event(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    date: &str,
    track_id: &str,
) -> Result<EventRow, sqlx::Error> {
    sqlx::query("INSERT INTO events (id, name, date, track_id) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(name)
        .bind(date)
        .bind(track_id)
        .execute(pool)
        .await?;

    get_event(pool, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn update_event(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    date: &str,
    status: &str,
) -> Result<EventRow, sqlx::Error> {
    sqlx::query("UPDATE events SET name = ?, date = ?, status = ? WHERE id = ?")
        .bind(name)
        .bind(date)
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;

    get_event(pool, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn delete_event(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM events WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Event Classes ---

pub async fn list_classes(
    pool: &SqlitePool,
    event_id: &str,
) -> Result<Vec<EventClassRow>, sqlx::Error> {
    sqlx::query_as::<_, EventClassRow>(
        "SELECT * FROM event_classes WHERE event_id = ? ORDER BY name",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
}

pub async fn get_class(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<EventClassRow>, sqlx::Error> {
    sqlx::query_as::<_, EventClassRow>("SELECT * FROM event_classes WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn create_class(
    pool: &SqlitePool,
    id: &str,
    event_id: &str,
    name: &str,
    age_group: Option<&str>,
    skill_level: Option<&str>,
    gender: Option<&str>,
    equipment: Option<&str>,
    race_format: &str,
    scoring: &str,
) -> Result<EventClassRow, sqlx::Error> {
    sqlx::query(
        "INSERT INTO event_classes (id, event_id, name, age_group, skill_level, gender, equipment, race_format, scoring) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(event_id)
    .bind(name)
    .bind(age_group)
    .bind(skill_level)
    .bind(gender)
    .bind(equipment)
    .bind(race_format)
    .bind(scoring)
    .execute(pool)
    .await?;

    get_class(pool, id).await?.ok_or(sqlx::Error::RowNotFound)
}

pub async fn delete_class(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM event_classes WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Class Riders ---

pub async fn add_rider_to_class(
    pool: &SqlitePool,
    class_id: &str,
    rider_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO event_class_riders (class_id, rider_id) VALUES (?, ?)")
        .bind(class_id)
        .bind(rider_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remove_rider_from_class(
    pool: &SqlitePool,
    class_id: &str,
    rider_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM event_class_riders WHERE class_id = ? AND rider_id = ?")
        .bind(class_id)
        .bind(rider_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_class_rider_ids(
    pool: &SqlitePool,
    class_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT rider_id FROM event_class_riders WHERE class_id = ? ORDER BY rider_id",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|(id,)| id).collect())
}
