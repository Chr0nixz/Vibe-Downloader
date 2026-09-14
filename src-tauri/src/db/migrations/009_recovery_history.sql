-- Recovery Center: durable log of recovery resolutions (per-task resolve,
-- batch safe retry, credential updates) so the page can show what was done,
-- when, and from which surface. task_events is pruned after 14 days / 200
-- rows per task and carries no dedicated attention event, so this table is
-- the long-lived audit trail. Rows are pruned to a recent window by
-- db::insert_recovery_record.
CREATE TABLE IF NOT EXISTS recovery_history (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    task_file_name TEXT,
    action TEXT NOT NULL,
    source TEXT NOT NULL,
    error_code TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_recovery_history_created
    ON recovery_history(created_at DESC);
