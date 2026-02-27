use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct PreparedIngestEvent {
    pub seq: i64,
    pub captured_at_us: i64,
    pub message_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct InsertSummary {
    pub accepted: usize,
    pub duplicates: usize,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct IngestMessageRow {
    pub id: String,
    pub session_id: String,
    pub track_id: String,
    pub client_id: String,
    pub seq: i64,
    pub captured_at_us: i64,
    pub message_type: String,
    pub payload_json: String,
    pub received_at: String,
}

pub async fn insert_batch(
    pool: &SqlitePool,
    session_id: &str,
    track_id: &str,
    client_id: &str,
    events: &[PreparedIngestEvent],
) -> Result<InsertSummary, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut summary = InsertSummary::default();

    for event in events {
        let id = uuid::Uuid::new_v4().to_string();
        let result = sqlx::query(
            "INSERT INTO ingest_messages \
             (id, session_id, track_id, client_id, seq, captured_at_us, message_type, payload_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(session_id, client_id, seq) DO NOTHING",
        )
        .bind(&id)
        .bind(session_id)
        .bind(track_id)
        .bind(client_id)
        .bind(event.seq)
        .bind(event.captured_at_us)
        .bind(&event.message_type)
        .bind(&event.payload_json)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 1 {
            summary.accepted += 1;
        } else {
            summary.duplicates += 1;
        }
    }

    tx.commit().await?;
    Ok(summary)
}

pub async fn list_messages(
    pool: &SqlitePool,
    session_id: &str,
    track_id: Option<&str>,
    client_id: Option<&str>,
    limit: i64,
) -> Result<Vec<IngestMessageRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, IngestMessageRow>(
        "SELECT id, session_id, track_id, client_id, seq, captured_at_us, message_type, payload_json, received_at \
         FROM ingest_messages \
         WHERE session_id = ? \
           AND (? IS NULL OR track_id = ?) \
           AND (? IS NULL OR client_id = ?) \
         ORDER BY client_id ASC, seq ASC \
         LIMIT ?",
    )
    .bind(session_id)
    .bind(track_id)
    .bind(track_id)
    .bind(client_id)
    .bind(client_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
