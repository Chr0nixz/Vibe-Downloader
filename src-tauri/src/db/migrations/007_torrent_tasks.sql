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

CREATE INDEX IF NOT EXISTS idx_torrent_tasks_info_hash
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
