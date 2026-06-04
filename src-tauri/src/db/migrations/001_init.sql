-- DRAFT: pre-HTTP-MVP, breaking changes allowed

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    final_url TEXT,
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
    supports_range INTEGER NOT NULL DEFAULT 0,
    source_host TEXT,
    connection_count INTEGER NOT NULL DEFAULT 0,
    speed_bps INTEGER NOT NULL DEFAULT 0,
    health_summary TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS segments (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    range_start INTEGER NOT NULL,
    range_end INTEGER NOT NULL,
    downloaded_until INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS idx_segments_task_id ON segments(task_id);

CREATE TABLE IF NOT EXISTS task_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
