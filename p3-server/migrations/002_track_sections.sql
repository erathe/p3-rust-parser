-- Track sections: physical layout segments of a track
CREATE TABLE IF NOT EXISTS track_sections (
    id           TEXT PRIMARY KEY,
    track_id     TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    section_type TEXT NOT NULL DEFAULT 'custom',
    length_m     REAL NOT NULL DEFAULT 10.0,
    position     INTEGER NOT NULL,
    loop_id      TEXT REFERENCES timing_loops(id) ON DELETE SET NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(track_id, position)
);
