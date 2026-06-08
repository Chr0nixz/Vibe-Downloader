ALTER TABLE tasks ADD COLUMN expected_hash_sha256 TEXT;
ALTER TABLE tasks ADD COLUMN actual_hash_sha256 TEXT;
ALTER TABLE tasks ADD COLUMN hash_status TEXT NOT NULL DEFAULT 'not_requested';
ALTER TABLE tasks ADD COLUMN hash_error TEXT;
ALTER TABLE tasks ADD COLUMN hash_verified_at TEXT;

ALTER TABLE task_work_units ADD COLUMN speed_bps INTEGER NOT NULL DEFAULT 0;

CREATE TABLE task_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    range_header TEXT,
    status_code INTEGER,
    etag TEXT,
    last_modified TEXT,
    content_length INTEGER,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_task_requests_task_id ON task_requests(task_id);
CREATE INDEX idx_task_requests_created_at ON task_requests(created_at);
