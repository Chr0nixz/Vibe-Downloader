ALTER TABLE tasks ADD COLUMN obey_schedule INTEGER NOT NULL DEFAULT 1;

ALTER TABLE torrent_tasks ADD COLUMN seeding_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE torrent_tasks ADD COLUMN seed_ratio_limit REAL;
ALTER TABLE torrent_tasks ADD COLUMN seed_time_limit_seconds INTEGER;

ALTER TABLE torrent_runtime_snapshots ADD COLUMN piece_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE torrent_runtime_snapshots ADD COLUMN piece_bitfield_base64 TEXT;
ALTER TABLE torrent_runtime_snapshots ADD COLUMN dht_status TEXT;
ALTER TABLE torrent_runtime_snapshots ADD COLUMN trackers_json TEXT;
ALTER TABLE torrent_runtime_snapshots ADD COLUMN seeding_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE torrent_runtime_snapshots ADD COLUMN seeding_state TEXT NOT NULL DEFAULT 'disabled';
ALTER TABLE torrent_runtime_snapshots ADD COLUMN last_error_code TEXT;
ALTER TABLE torrent_runtime_snapshots ADD COLUMN last_error_message TEXT;

CREATE TABLE task_proxy_settings (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    mode TEXT NOT NULL DEFAULT 'inherit',
    proxy_url TEXT,
    proxy_username TEXT,
    proxy_password_ciphertext TEXT,
    nonce TEXT,
    no_proxy TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_tasks_obey_schedule_status
ON tasks(obey_schedule, status);
