-- DRAFT schema: pre-HTTP-MVP, destructive changes are allowed.

CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    final_url TEXT,
    protocol TEXT NOT NULL,
    task_kind TEXT NOT NULL DEFAULT 'single_file',
    file_name TEXT NOT NULL,
    save_dir TEXT NOT NULL,
    temp_path TEXT,
    final_path TEXT,
    total_size INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'queued',
    etag TEXT,
    last_modified TEXT,
    content_type TEXT,
    supports_resume INTEGER NOT NULL DEFAULT 0,
    supports_parallel INTEGER NOT NULL DEFAULT 0,
    supports_multi_file INTEGER NOT NULL DEFAULT 0,
    source_key TEXT NOT NULL,
    connection_count INTEGER NOT NULL DEFAULT 0,
    speed_bps INTEGER NOT NULL DEFAULT 0,
    health_summary TEXT,
    error_message TEXT,
    expected_hash_sha256 TEXT,
    actual_hash_sha256 TEXT,
    hash_status TEXT NOT NULL DEFAULT 'not_requested',
    hash_error TEXT,
    hash_verified_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_protocol ON tasks(protocol);
CREATE INDEX idx_tasks_source_key ON tasks(source_key);

CREATE TABLE task_files (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    file_name TEXT NOT NULL,
    save_dir TEXT NOT NULL,
    temp_path TEXT,
    final_path TEXT,
    total_size INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    selected INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'queued',
    content_type TEXT
);

CREATE INDEX idx_task_files_task_id ON task_files(task_id);

CREATE TABLE task_work_units (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id TEXT REFERENCES task_files(id) ON DELETE CASCADE,
    unit_kind TEXT NOT NULL,
    range_start INTEGER NOT NULL DEFAULT 0,
    range_end INTEGER NOT NULL DEFAULT 0,
    downloaded_until INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    speed_bps INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_work_units_task_id ON task_work_units(task_id);
CREATE INDEX idx_work_units_file_id ON task_work_units(file_id);

CREATE TABLE task_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload TEXT,
    created_at TEXT NOT NULL
);

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

CREATE TABLE browser_messages (
    request_id TEXT PRIMARY KEY NOT NULL,
    browser TEXT NOT NULL,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_browser_messages_browser_created_at
ON browser_messages(browser, created_at DESC);

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
