-- Tracks
CREATE TABLE IF NOT EXISTS tracks (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    hill_type   TEXT NOT NULL DEFAULT '8m' CHECK (hill_type IN ('5m', '8m')),
    gate_beacon_id INTEGER NOT NULL DEFAULT 9992,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Timing loops for a track (ordered by position along the track)
CREATE TABLE IF NOT EXISTS timing_loops (
    id          TEXT PRIMARY KEY,
    track_id    TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    decoder_id  TEXT NOT NULL,
    position    INTEGER NOT NULL,
    is_finish   INTEGER NOT NULL DEFAULT 0,
    is_start    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(track_id, position),
    UNIQUE(track_id, decoder_id)
);

-- Riders
CREATE TABLE IF NOT EXISTS riders (
    id                 TEXT PRIMARY KEY,
    first_name         TEXT NOT NULL,
    last_name          TEXT NOT NULL,
    plate_number       TEXT NOT NULL,
    transponder_id     INTEGER NOT NULL,
    transponder_string TEXT,
    age_group          TEXT,
    skill_level        TEXT CHECK (skill_level IN ('Novice', 'Intermediate', 'Expert')),
    gender             TEXT CHECK (gender IN ('Male', 'Female')),
    equipment          TEXT CHECK (equipment IN ('20"', 'Cruiser')) DEFAULT '20"',
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Events (race day)
CREATE TABLE IF NOT EXISTS events (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    date        TEXT NOT NULL,
    track_id    TEXT NOT NULL REFERENCES tracks(id),
    status      TEXT NOT NULL DEFAULT 'setup'
                CHECK (status IN ('setup', 'active', 'completed')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Classes within an event
CREATE TABLE IF NOT EXISTS event_classes (
    id          TEXT PRIMARY KEY,
    event_id    TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    age_group   TEXT,
    skill_level TEXT,
    gender      TEXT,
    equipment   TEXT,
    race_format TEXT NOT NULL CHECK (race_format IN (
        'motos_only', 'motos_main', 'motos_semis_main', 'motos_quarters_semis_main'
    )),
    scoring     TEXT NOT NULL DEFAULT 'total_points'
                CHECK (scoring IN ('total_points', 'transfer')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Riders registered in a class for an event
CREATE TABLE IF NOT EXISTS event_class_riders (
    id          TEXT PRIMARY KEY,
    class_id    TEXT NOT NULL REFERENCES event_classes(id) ON DELETE CASCADE,
    rider_id    TEXT NOT NULL REFERENCES riders(id),
    UNIQUE(class_id, rider_id)
);

-- Motos (individual heats)
CREATE TABLE IF NOT EXISTS motos (
    id          TEXT PRIMARY KEY,
    event_id    TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    class_id    TEXT NOT NULL REFERENCES event_classes(id),
    round_type  TEXT NOT NULL CHECK (round_type IN (
        'moto1', 'moto2', 'moto3', 'quarter', 'semi', 'main'
    )),
    round_number INTEGER,
    sequence    INTEGER NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'staged', 'racing', 'finished')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Riders in a specific moto with lane assignments
CREATE TABLE IF NOT EXISTS moto_entries (
    id          TEXT PRIMARY KEY,
    moto_id     TEXT NOT NULL REFERENCES motos(id) ON DELETE CASCADE,
    rider_id    TEXT NOT NULL REFERENCES riders(id),
    lane        INTEGER NOT NULL CHECK (lane BETWEEN 1 AND 8),
    finish_position INTEGER,
    elapsed_us  INTEGER,
    points      INTEGER,
    dnf         INTEGER NOT NULL DEFAULT 0,
    dns         INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(moto_id, lane),
    UNIQUE(moto_id, rider_id)
);

-- Raw passing records (for replay/audit)
CREATE TABLE IF NOT EXISTS passings (
    id              TEXT PRIMARY KEY,
    event_id        TEXT REFERENCES events(id),
    moto_id         TEXT REFERENCES motos(id),
    passing_number  INTEGER NOT NULL,
    transponder_id  INTEGER NOT NULL,
    decoder_id      TEXT,
    rtc_time_us     INTEGER NOT NULL,
    strength        INTEGER,
    hits            INTEGER,
    transponder_string TEXT,
    is_gate_drop    INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Split times (derived from passings)
CREATE TABLE IF NOT EXISTS split_times (
    id          TEXT PRIMARY KEY,
    moto_id     TEXT NOT NULL REFERENCES motos(id) ON DELETE CASCADE,
    rider_id    TEXT NOT NULL REFERENCES riders(id),
    loop_id     TEXT NOT NULL REFERENCES timing_loops(id),
    timestamp_us INTEGER NOT NULL,
    elapsed_us  INTEGER NOT NULL,
    position    INTEGER NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(moto_id, rider_id, loop_id)
);

-- Decoder diagnostics (latest status per decoder)
CREATE TABLE IF NOT EXISTS decoder_status (
    decoder_id  TEXT PRIMARY KEY,
    noise       INTEGER,
    temperature INTEGER,
    gps_status  INTEGER,
    satellites  INTEGER,
    last_seen   TEXT NOT NULL DEFAULT (datetime('now'))
);
