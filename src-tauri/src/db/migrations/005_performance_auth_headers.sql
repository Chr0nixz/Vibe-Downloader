ALTER TABLE tasks ADD COLUMN error_code TEXT;
ALTER TABLE tasks ADD COLUMN recovery_actions TEXT;
ALTER TABLE tasks ADD COLUMN retry_after_at TEXT;

CREATE INDEX IF NOT EXISTS idx_tasks_updated_at_desc
ON tasks(updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_status_updated_at_desc
ON tasks(status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_source_key_updated_at_desc
ON tasks(source_key, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_events_task_id_created_at_desc
ON task_events(task_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_requests_task_id_created_at_desc
ON task_requests(task_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_work_units_task_id_status
ON task_work_units(task_id, status);

CREATE TABLE task_request_headers (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    headers_json TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    source_browser TEXT
);

CREATE INDEX IF NOT EXISTS idx_task_request_headers_expires_at
ON task_request_headers(expires_at);
