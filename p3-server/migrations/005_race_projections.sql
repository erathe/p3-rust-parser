CREATE TABLE IF NOT EXISTS race_state_projection (
    track_id           TEXT PRIMARY KEY,
    phase              TEXT NOT NULL,
    moto_id            TEXT,
    class_name         TEXT,
    round_type         TEXT,
    riders_json        TEXT NOT NULL,
    positions_json     TEXT NOT NULL,
    gate_drop_time_us  INTEGER,
    finished_count     INTEGER NOT NULL,
    total_riders       INTEGER NOT NULL,
    results_json       TEXT NOT NULL,
    updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS race_projection_dedupe (
    event_id      TEXT PRIMARY KEY,
    processed_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
