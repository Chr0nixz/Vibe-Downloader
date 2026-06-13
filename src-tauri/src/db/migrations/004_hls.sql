CREATE TABLE hls_tasks (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    input_url TEXT NOT NULL,
    media_url TEXT NOT NULL,
    playlist_kind TEXT NOT NULL DEFAULT 'live',
    selected_bandwidth INTEGER,
    selected_resolution TEXT,
    target_duration INTEGER NOT NULL DEFAULT 6,
    last_media_sequence INTEGER,
    output_format TEXT NOT NULL DEFAULT 'mp4',
    staging_dir TEXT NOT NULL,
    finish_requested INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_hls_tasks_finish_requested
ON hls_tasks(finish_requested);

CREATE TABLE hls_segments (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    media_sequence INTEGER NOT NULL,
    discontinuity_sequence INTEGER NOT NULL DEFAULT 0,
    uri TEXT NOT NULL,
    local_path TEXT NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    byte_range_start INTEGER,
    byte_range_length INTEGER,
    key_method TEXT,
    key_uri TEXT,
    key_iv TEXT,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_hls_segments_task_sequence
ON hls_segments(task_id, media_sequence, discontinuity_sequence);

CREATE INDEX idx_hls_segments_task_status
ON hls_segments(task_id, status);
