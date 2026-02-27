-- Dev-only ingest log for distributed track-client testing.
-- This is append-only event storage used for replay and diagnostics.

CREATE TABLE IF NOT EXISTS ingest_messages (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    track_id        TEXT NOT NULL,
    client_id       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    captured_at_us  INTEGER NOT NULL,
    message_type    TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    received_at     TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(client_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_ingest_messages_session_track
    ON ingest_messages(session_id, track_id);

CREATE INDEX IF NOT EXISTS idx_ingest_messages_session_order
    ON ingest_messages(session_id, client_id, seq);
