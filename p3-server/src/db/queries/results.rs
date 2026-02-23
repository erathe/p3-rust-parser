use sqlx::SqlitePool;

use crate::domain::race_event::FinishResult;

/// Persist race results to the database after a moto finishes.
/// Updates moto_entries with finish position, elapsed time, points, and DNF status.
/// Also updates the moto status to 'finished'.
pub async fn persist_results(
    pool: &SqlitePool,
    moto_id: &str,
    results: &[FinishResult],
) -> Result<(), sqlx::Error> {
    // Update moto status
    sqlx::query("UPDATE motos SET status = 'finished' WHERE id = ?")
        .bind(moto_id)
        .execute(pool)
        .await?;

    // Update each rider's moto entry
    for result in results {
        // Points: 1st=1, 2nd=2, 3rd=3, etc. (golf scoring, lower is better)
        // DNF gets max points (rider count + 1 typically, but we'll use position)
        let points = if result.dnf {
            results.len() as i64 + 1
        } else {
            result.position as i64
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
        .bind(if result.dnf { None } else { Some(result.position as i64) })
        .bind(result.elapsed_us.map(|us| us as i64))
        .bind(points)
        .bind(result.dnf)
        .bind(result.dns)
        .bind(moto_id)
        .bind(&result.rider_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Get total points for a rider across all motos in a class.
pub async fn get_class_standings(
    pool: &SqlitePool,
    class_id: &str,
) -> Result<Vec<RiderStanding>, sqlx::Error> {
    let rows = sqlx::query_as::<_, RiderStandingRow>(
        "SELECT \
            r.id as rider_id, \
            r.first_name, \
            r.last_name, \
            r.plate_number, \
            COALESCE(SUM(me.points), 0) as total_points, \
            COUNT(CASE WHEN me.finish_position IS NOT NULL THEN 1 END) as motos_completed, \
            COUNT(CASE WHEN me.dnf = 1 THEN 1 END) as dnf_count \
         FROM riders r \
         JOIN event_class_riders ecr ON ecr.rider_id = r.id \
         LEFT JOIN moto_entries me ON me.rider_id = r.id \
         LEFT JOIN motos m ON m.id = me.moto_id AND m.class_id = ? AND m.status = 'finished' \
         WHERE ecr.class_id = ? \
         GROUP BY r.id \
         ORDER BY total_points ASC, motos_completed DESC",
    )
    .bind(class_id)
    .bind(class_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| RiderStanding {
        rider_id: r.rider_id,
        first_name: r.first_name,
        last_name: r.last_name,
        plate_number: r.plate_number,
        total_points: r.total_points,
        motos_completed: r.motos_completed,
        dnf_count: r.dnf_count,
    }).collect())
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RiderStandingRow {
    rider_id: String,
    first_name: String,
    last_name: String,
    plate_number: String,
    total_points: i64,
    motos_completed: i64,
    dnf_count: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RiderStanding {
    pub rider_id: String,
    pub first_name: String,
    pub last_name: String,
    pub plate_number: String,
    pub total_points: i64,
    pub motos_completed: i64,
    pub dnf_count: i64,
}
