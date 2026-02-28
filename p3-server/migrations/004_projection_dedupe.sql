CREATE TABLE IF NOT EXISTS projection_dedupe (
    idempotency_key TEXT PRIMARY KEY,
    processed_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
