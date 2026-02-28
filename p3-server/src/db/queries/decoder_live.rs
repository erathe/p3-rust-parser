use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DecoderSnapshotRow {
    pub loop_id: String,
    pub loop_name: String,
    pub loop_position: i64,
    pub decoder_id: String,
    pub noise: Option<i64>,
    pub temperature: Option<i64>,
    pub gps_status: Option<i64>,
    pub satellites: Option<i64>,
    pub last_seen: Option<String>,
}

pub async fn list_decoder_snapshot_rows_for_track(
    pool: &SqlitePool,
    track_id: &str,
) -> Result<Vec<DecoderSnapshotRow>, sqlx::Error> {
    sqlx::query_as::<_, DecoderSnapshotRow>(
        "SELECT \
            timing_loops.id AS loop_id, \
            timing_loops.name AS loop_name, \
            timing_loops.position AS loop_position, \
            timing_loops.decoder_id AS decoder_id, \
            decoder_status.noise AS noise, \
            decoder_status.temperature AS temperature, \
            decoder_status.gps_status AS gps_status, \
            decoder_status.satellites AS satellites, \
            decoder_status.last_seen AS last_seen \
        FROM timing_loops \
        LEFT JOIN decoder_status ON decoder_status.decoder_id = timing_loops.decoder_id \
        WHERE timing_loops.track_id = ? \
        ORDER BY timing_loops.position ASC, timing_loops.id ASC",
    )
    .bind(track_id)
    .fetch_all(pool)
    .await
}
