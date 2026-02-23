use sqlx::SqlitePool;
use uuid::Uuid;

use crate::db::models::RiderRow;

pub async fn list_riders(
    pool: &SqlitePool,
    search: Option<&str>,
) -> sqlx::Result<Vec<RiderRow>> {
    match search {
        Some(term) => {
            let pattern = format!("%{}%", term);
            sqlx::query_as::<_, RiderRow>(
                "SELECT * FROM riders WHERE first_name LIKE ? OR last_name LIKE ? OR plate_number LIKE ? OR transponder_string LIKE ? ORDER BY last_name, first_name",
            )
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, RiderRow>(
                "SELECT * FROM riders ORDER BY last_name, first_name",
            )
            .fetch_all(pool)
            .await
        }
    }
}

pub async fn get_rider(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<RiderRow>> {
    sqlx::query_as::<_, RiderRow>("SELECT * FROM riders WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub struct CreateRider {
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub transponder_id: i64,
    pub transponder_string: Option<String>,
    pub age_group: Option<String>,
    pub skill_level: Option<String>,
    pub gender: Option<String>,
    pub equipment: Option<String>,
}

pub async fn create_rider(pool: &SqlitePool, input: CreateRider) -> sqlx::Result<RiderRow> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO riders (id, first_name, last_name, plate_number, transponder_id, transponder_string, age_group, skill_level, gender, equipment) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(&input.plate_number)
    .bind(input.transponder_id)
    .bind(&input.transponder_string)
    .bind(&input.age_group)
    .bind(&input.skill_level)
    .bind(&input.gender)
    .bind(&input.equipment)
    .execute(pool)
    .await?;

    get_rider(pool, &id).await.map(|r| r.unwrap())
}

pub async fn update_rider(pool: &SqlitePool, id: &str, input: CreateRider) -> sqlx::Result<Option<RiderRow>> {
    let result = sqlx::query(
        "UPDATE riders SET first_name = ?, last_name = ?, plate_number = ?, transponder_id = ?, transponder_string = ?, age_group = ?, skill_level = ?, gender = ?, equipment = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(&input.plate_number)
    .bind(input.transponder_id)
    .bind(&input.transponder_string)
    .bind(&input.age_group)
    .bind(&input.skill_level)
    .bind(&input.gender)
    .bind(&input.equipment)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    get_rider(pool, id).await
}

pub async fn delete_rider(pool: &SqlitePool, id: &str) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM riders WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
