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

    migrate_legacy_ingest_unique_key(pool).await?;

    info!("Database migrations applied");
    Ok(())
}

async fn migrate_legacy_ingest_unique_key(pool: &SqlitePool) -> anyhow::Result<()> {
    let table_sql = sqlx::query_scalar::<_, String>(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'ingest_messages'",
    )
    .fetch_optional(pool)
    .await?;

    let Some(table_sql) = table_sql else {
        return Ok(());
    };

    let has_legacy_constraint = table_sql.contains("UNIQUE(client_id, seq)");
    let has_new_constraint = table_sql.contains("UNIQUE(session_id, client_id, seq)");

    if !has_legacy_constraint || has_new_constraint {
        return Ok(());
    }

    info!("Migrating ingest_messages dedupe key to include session_id");

    let mut tx = pool.begin().await?;

    sqlx::query("ALTER TABLE ingest_messages RENAME TO ingest_messages_legacy")
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "CREATE TABLE ingest_messages (
            id              TEXT PRIMARY KEY,
            session_id      TEXT NOT NULL,
            track_id        TEXT NOT NULL,
            client_id       TEXT NOT NULL,
            seq             INTEGER NOT NULL,
            captured_at_us  INTEGER NOT NULL,
            message_type    TEXT NOT NULL,
            payload_json    TEXT NOT NULL,
            received_at     TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(session_id, client_id, seq)
        )",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO ingest_messages \
         (id, session_id, track_id, client_id, seq, captured_at_us, message_type, payload_json, received_at) \
         SELECT id, session_id, track_id, client_id, seq, captured_at_us, message_type, payload_json, received_at \
         FROM ingest_messages_legacy",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query("DROP TABLE ingest_messages_legacy")
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ingest_messages_session_track \
         ON ingest_messages(session_id, track_id)",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ingest_messages_session_order \
         ON ingest_messages(session_id, client_id, seq)",
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
