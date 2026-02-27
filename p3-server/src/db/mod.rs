pub mod models;
pub mod queries;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

pub async fn create_pool(db_path: &str) -> anyhow::Result<SqlitePool> {
    let url = format!("sqlite:{}?mode=rwc", db_path);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;

    info!(path = %db_path, "Database connected");
    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    // Enable WAL mode and foreign keys
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;

    let migrations = [
        include_str!("../../migrations/001_initial_schema.sql"),
        include_str!("../../migrations/002_track_sections.sql"),
        include_str!("../../migrations/003_dev_ingest.sql"),
    ];

    for migration_sql in &migrations {
        for statement in migration_sql.split(';') {
            let stmt = statement.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await?;
            }
        }
    }

    info!("Database migrations applied");
    Ok(())
}
