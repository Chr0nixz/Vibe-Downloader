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
    error_code TEXT,
    recovery_actions TEXT,
    retry_after_at TEXT,
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
CREATE INDEX idx_tasks_updated_at_desc ON tasks(updated_at DESC);
CREATE INDEX idx_tasks_status_updated_at_desc ON tasks(status, updated_at DESC);
CREATE INDEX idx_tasks_source_key_updated_at_desc ON tasks(source_key, updated_at DESC);
CREATE INDEX idx_tasks_error_code_updated_at_desc ON tasks(error_code, updated_at DESC);
CREATE INDEX idx_tasks_retry_after_at ON tasks(retry_after_at);

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
CREATE INDEX idx_work_units_task_id_status ON task_work_units(task_id, status);

CREATE TABLE task_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_task_events_task_id_created_at_desc
ON task_events(task_id, created_at DESC);

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
CREATE INDEX idx_task_requests_task_id_created_at_desc
ON task_requests(task_id, created_at DESC);

CREATE TABLE task_request_headers (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    headers_json TEXT NOT NULL,
    headers_ciphertext TEXT,
    nonce TEXT,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    source_browser TEXT
);

CREATE INDEX idx_task_request_headers_expires_at
ON task_request_headers(expires_at);

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

CREATE TABLE torrent_tasks (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    info_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    magnet_uri TEXT,
    torrent_blob BLOB,
    piece_length INTEGER NOT NULL DEFAULT 0,
    piece_count INTEGER NOT NULL DEFAULT 0,
    private INTEGER NOT NULL DEFAULT 0,
    trackers_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_torrent_tasks_info_hash
ON torrent_tasks(info_hash);

CREATE TABLE torrent_runtime_snapshots (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    metadata_status TEXT NOT NULL DEFAULT 'pending',
    completed_pieces INTEGER NOT NULL DEFAULT 0,
    verified_pieces INTEGER NOT NULL DEFAULT 0,
    peer_count INTEGER NOT NULL DEFAULT 0,
    seed_count INTEGER NOT NULL DEFAULT 0,
    upload_bytes INTEGER NOT NULL DEFAULT 0,
    upload_speed_bps INTEGER NOT NULL DEFAULT 0,
    ratio REAL NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
