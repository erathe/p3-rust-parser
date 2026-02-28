use anyhow::{Context, anyhow};
use p3_contracts::TrackIngestEvent;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::Path;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct SpoolRow {
    pub id: i64,
    pub event: TrackIngestEvent,
}

#[derive(Debug, Default)]
pub struct LoadedBatch {
    pub rows: Vec<SpoolRow>,
    pub dropped_invalid: usize,
}

#[derive(Clone)]
pub struct SpoolStore {
    pool: SqlitePool,
}

#[derive(Debug, sqlx::FromRow)]
struct DbSpoolRow {
    id: i64,
    event_json: String,
}

impl SpoolStore {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create spool directory {}",
                    parent.to_string_lossy()
                )
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .with_context(|| {
                format!(
                    "Failed to open spool sqlite database {}",
                    path.to_string_lossy()
                )
            })?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ingest_spool_events (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                event_json TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .context("Failed to create ingest spool table")?;

        Ok(Self { pool })
    }

    pub async fn enqueue(&self, event: &TrackIngestEvent) -> anyhow::Result<()> {
        let event_json =
            serde_json::to_string(event).context("Failed to serialize ingest event")?;

        sqlx::query("INSERT INTO ingest_spool_events (event_json) VALUES (?)")
            .bind(event_json)
            .execute(&self.pool)
            .await
            .context("Failed to append ingest event to spool")?;

        Ok(())
    }

    pub async fn load_batch(&self, limit: usize) -> anyhow::Result<LoadedBatch> {
        if limit == 0 {
            return Ok(LoadedBatch::default());
        }

        let limit_i64 = i64::try_from(limit).map_err(|_| anyhow!("batch size too large"))?;
        let rows = sqlx::query_as::<_, DbSpoolRow>(
            "SELECT id, event_json FROM ingest_spool_events ORDER BY id ASC LIMIT ?",
        )
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to load ingest spool batch")?;

        let mut valid_rows = Vec::with_capacity(rows.len());
        let mut invalid_ids = Vec::new();

        for row in rows {
            match serde_json::from_str::<TrackIngestEvent>(&row.event_json) {
                Ok(event) => valid_rows.push(SpoolRow { id: row.id, event }),
                Err(error) => {
                    warn!(
                        row_id = row.id,
                        error = %error,
                        "Dropping invalid ingest spool row",
                    );
                    invalid_ids.push(row.id);
                }
            }
        }

        if !invalid_ids.is_empty() {
            let _ = self.ack_batch(&invalid_ids).await?;
        }

        Ok(LoadedBatch {
            rows: valid_rows,
            dropped_invalid: invalid_ids.len(),
        })
    }

    pub async fn ack_batch(&self, ids: &[i64]) -> anyhow::Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to start spool ack tx")?;
        let mut deleted = 0usize;

        for id in ids {
            let result = sqlx::query("DELETE FROM ingest_spool_events WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("Failed to delete acked spool row")?;
            deleted = deleted.saturating_add(usize::try_from(result.rows_affected()).unwrap_or(0));
        }

        tx.commit().await.context("Failed to commit spool ack tx")?;
        Ok(deleted)
    }

    pub async fn drop_oldest(&self, count: usize) -> anyhow::Result<usize> {
        if count == 0 {
            return Ok(0);
        }

        let count_i64 = i64::try_from(count).map_err(|_| anyhow!("drop count too large"))?;
        let result = sqlx::query(
            "DELETE FROM ingest_spool_events
             WHERE id IN (
                SELECT id FROM ingest_spool_events ORDER BY id ASC LIMIT ?
             )",
        )
        .bind(count_i64)
        .execute(&self.pool)
        .await
        .context("Failed to trim oldest spool rows")?;

        Ok(usize::try_from(result.rows_affected()).unwrap_or(0))
    }

    pub async fn len(&self) -> anyhow::Result<usize> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ingest_spool_events")
            .fetch_one(&self.pool)
            .await
            .context("Failed to read spool depth")?;

        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p3_contracts::{EventIdContext, TrackIngestEvent, message_type_from_message};
    use p3_parser::{Message, StatusMessage};
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    #[tokio::test]
    async fn load_batch_preserves_enqueue_order() {
        let path = temp_spool_path("order");
        let store = SpoolStore::open(&path).await.unwrap();

        store.enqueue(&test_event(1)).await.unwrap();
        store.enqueue(&test_event(2)).await.unwrap();
        store.enqueue(&test_event(3)).await.unwrap();

        let loaded = store.load_batch(2).await.unwrap();
        assert_eq!(loaded.dropped_invalid, 0);
        assert_eq!(loaded.rows.len(), 2);
        assert_eq!(loaded.rows[0].event.event_id_context.seq, 1);
        assert_eq!(loaded.rows[1].event.event_id_context.seq, 2);

        let deleted = store
            .ack_batch(&[loaded.rows[0].id, loaded.rows[1].id])
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.len().await.unwrap(), 1);

        drop(store);
        cleanup_spool_files(&path);
    }

    #[tokio::test]
    async fn drop_oldest_trims_from_head() {
        let path = temp_spool_path("trim");
        let store = SpoolStore::open(&path).await.unwrap();

        for seq in 1..=5 {
            store.enqueue(&test_event(seq)).await.unwrap();
        }

        let dropped = store.drop_oldest(2).await.unwrap();
        assert_eq!(dropped, 2);

        let loaded = store.load_batch(10).await.unwrap();
        let seqs: Vec<u64> = loaded
            .rows
            .iter()
            .map(|row| row.event.event_id_context.seq)
            .collect();
        assert_eq!(seqs, vec![3, 4, 5]);

        drop(store);
        cleanup_spool_files(&path);
    }

    fn test_event(seq: u64) -> TrackIngestEvent {
        let payload = Message::Status(StatusMessage {
            noise: 55,
            gps_status: 1,
            temperature: 220,
            satellites: 8,
            decoder_id: Some("D1000C00".to_string()),
        });

        TrackIngestEvent {
            event_id: Uuid::new_v4(),
            track_id: "track-a".to_string(),
            event_id_context: EventIdContext {
                client_id: "client-a".to_string(),
                boot_id: "boot-a".to_string(),
                seq,
            },
            captured_at_us: 1_000_000 + seq,
            message_type: message_type_from_message(&payload).to_string(),
            payload,
        }
    }

    fn temp_spool_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "p3-track-client-spool-{}-{}.db",
            label,
            Uuid::new_v4()
        ))
    }

    fn cleanup_spool_files(path: &Path) {
        let _ = std::fs::remove_file(path);

        let wal = PathBuf::from(format!("{}-wal", path.to_string_lossy()));
        let shm = PathBuf::from(format!("{}-shm", path.to_string_lossy()));
        let _ = std::fs::remove_file(wal);
        let _ = std::fs::remove_file(shm);
    }
}
