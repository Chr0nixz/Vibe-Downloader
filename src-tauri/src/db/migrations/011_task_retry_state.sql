-- B4: task-level automatic retry budget and reason survive process restarts.
-- The next deadline remains in tasks.retry_after_at so the existing queue
-- index and scheduler query continue to serve both manual and automatic waits.
CREATE TABLE task_auto_retry_state (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    attempt INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_task_auto_retry_state_updated
    ON task_auto_retry_state(updated_at);
