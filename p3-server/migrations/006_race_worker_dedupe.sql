CREATE TABLE IF NOT EXISTS race_worker_dedupe (
    dedupe_key    TEXT PRIMARY KEY,
    track_id      TEXT NOT NULL,
    source        TEXT NOT NULL,
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_race_worker_dedupe_track
    ON race_worker_dedupe(track_id);
