use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupeSource {
    Raw,
    Control,
}

impl DedupeSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Control => "control",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOutcome {
    Claimed,
    Duplicate,
}

pub async fn claim(
    pool: &SqlitePool,
    dedupe_key: &str,
    track_id: &str,
    source: DedupeSource,
) -> Result<ClaimOutcome, sqlx::Error> {
    let insert = sqlx::query(
        "INSERT INTO race_worker_dedupe (dedupe_key, track_id, source) VALUES (?, ?, ?) \
         ON CONFLICT(dedupe_key) DO NOTHING",
    )
    .bind(dedupe_key)
    .bind(track_id)
    .bind(source.as_str())
    .execute(pool)
    .await?;

    if insert.rows_affected() == 0 {
        Ok(ClaimOutcome::Duplicate)
    } else {
        Ok(ClaimOutcome::Claimed)
    }
}

pub async fn release(pool: &SqlitePool, dedupe_key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM race_worker_dedupe WHERE dedupe_key = ?")
        .bind(dedupe_key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn claim_returns_duplicate_for_same_key() {
        let pool = test_pool().await;

        let first = claim(
            &pool,
            "raw:track-a:client-a:boot-a:1",
            "track-a",
            DedupeSource::Raw,
        )
        .await
        .unwrap();
        let second = claim(
            &pool,
            "raw:track-a:client-a:boot-a:1",
            "track-a",
            DedupeSource::Raw,
        )
        .await
        .unwrap();

        assert_eq!(first, ClaimOutcome::Claimed);
        assert_eq!(second, ClaimOutcome::Duplicate);
    }

    #[tokio::test]
    async fn release_allows_claiming_again() {
        let pool = test_pool().await;
        let key = "control:track-a:event-123";

        let first = claim(&pool, key, "track-a", DedupeSource::Control)
            .await
            .unwrap();
        assert_eq!(first, ClaimOutcome::Claimed);

        release(&pool, key).await.unwrap();

        let second = claim(&pool, key, "track-a", DedupeSource::Control)
            .await
            .unwrap();
        assert_eq!(second, ClaimOutcome::Claimed);
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
}
