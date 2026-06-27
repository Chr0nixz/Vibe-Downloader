-- 001_init.sql
-- Baseline schema for Vibe Downloader.
--
-- This file is a consolidated baseline that combines the original 14
-- incremental migrations (001_init.sql .. 014_task_files_version.sql) into a
-- single authoritative schema definition. It is semantically equivalent to
-- running all prior migrations in order: same tables, columns, types,
-- constraints, foreign keys, and indexes.
--
-- The consolidation was performed pre-release (no production users), so there
-- is no migration history to preserve. A fresh install only records this
-- single migration in `_sqlx_migrations`.
--
-- Sections:
--   1. Core task tables (tasks, task_files, task_work_units, task_events)
--   2. HTTP request diagnostics (task_requests, task_request_headers)
--   3. Browser handoff (browser_messages)
--   4. Credentials & proxy (task_credentials, task_proxy_settings)
--   5. Integrity verification (task_checksums)
--   6. Classification rules (classification_rules)
--   7. BitTorrent (torrent_tasks, torrent_runtime_snapshots)
--   8. HLS (hls_tasks, hls_segments)
--   9. DASH (dash_tasks, dash_segments)
--  10. Metalink (metalink_tasks, metalink_resources)
--  11. SFTP known hosts (sftp_known_hosts)
--  12. Settings (settings)
--  13. Indexes

-- ============================================================================
-- 1. Core task tables
-- ============================================================================

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
    updated_at TEXT NOT NULL,
    -- Transfer options (originally added in 005)
    task_speed_limit_bps TEXT,
    priority TEXT NOT NULL DEFAULT 'normal',
    queue_position INTEGER NOT NULL DEFAULT 0,
    category_key TEXT,
    -- Schedule obedience (originally added in 007)
    obey_schedule INTEGER NOT NULL DEFAULT 1,
    -- Internal optimization: bumped only when task_files *structure* changes
    -- (originally added in 014)
    files_version INTEGER NOT NULL DEFAULT 0
);

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

CREATE TABLE task_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload TEXT,
    created_at TEXT NOT NULL
);

-- ============================================================================
-- 2. HTTP request diagnostics
-- ============================================================================

CREATE TABLE task_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    range_header TEXT,
    -- if_range_header originally added in 003
    if_range_header TEXT,
    status_code INTEGER,
    etag TEXT,
    last_modified TEXT,
    content_length INTEGER,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

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

-- ============================================================================
-- 3. Browser handoff
-- ============================================================================

CREATE TABLE browser_messages (
    request_id TEXT PRIMARY KEY NOT NULL,
    browser TEXT NOT NULL,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    created_at TEXT NOT NULL
);

-- ============================================================================
-- 4. Credentials & proxy
-- ============================================================================

-- Encrypted protocol credentials (FTP/FTPS, SFTP, WebDAV). Ciphertext is
-- ChaCha20-Poly1305 with the nonce stored alongside.
CREATE TABLE task_credentials (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    protocol TEXT NOT NULL,
    credentials_ciphertext TEXT NOT NULL,
    nonce TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

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

-- ============================================================================
-- 5. Integrity verification
-- ============================================================================

-- Per-task and per-file checksum records. The legacy sha256 columns on tasks
-- remain for backwards compatibility but new verification uses this table.
CREATE TABLE task_checksums (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id TEXT REFERENCES task_files(id) ON DELETE CASCADE,
    algorithm TEXT NOT NULL,
    expected_hash TEXT NOT NULL,
    actual_hash TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    source_kind TEXT NOT NULL DEFAULT 'manual',
    source_url TEXT,
    source_label TEXT,
    is_primary INTEGER NOT NULL DEFAULT 0,
    weak INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    discovered_at TEXT,
    verified_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(task_id, file_id, algorithm, expected_hash)
);

-- ============================================================================
-- 6. Classification rules
-- ============================================================================

CREATE TABLE classification_rules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    position INTEGER NOT NULL DEFAULT 0,
    match_kind TEXT NOT NULL,
    pattern TEXT NOT NULL,
    target_subdir TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- ============================================================================
-- 7. BitTorrent
-- ============================================================================

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
    -- Seeding options originally added in 007
    seeding_enabled INTEGER NOT NULL DEFAULT 0,
    seed_ratio_limit REAL,
    seed_time_limit_seconds INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

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
    updated_at TEXT NOT NULL,
    -- Extended runtime fields originally added in 007
    piece_count INTEGER NOT NULL DEFAULT 0,
    piece_bitfield_base64 TEXT,
    dht_status TEXT,
    trackers_json TEXT,
    seeding_enabled INTEGER NOT NULL DEFAULT 0,
    seeding_state TEXT NOT NULL DEFAULT 'disabled',
    last_error_code TEXT,
    last_error_message TEXT
);

-- ============================================================================
-- 8. HLS
-- ============================================================================

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
    -- EXT-X-MAP init segment fields originally added in 006
    init_map_uri TEXT,
    init_map_local_path TEXT,
    init_map_byte_range_start INTEGER,
    init_map_byte_range_length INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- ============================================================================
-- 9. DASH
-- ============================================================================

CREATE TABLE dash_tasks (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    input_url TEXT NOT NULL,
    manifest_url TEXT NOT NULL,
    staging_dir TEXT NOT NULL,
    video_representation_id TEXT,
    video_bandwidth INTEGER,
    video_codecs TEXT,
    audio_representation_id TEXT,
    audio_bandwidth INTEGER,
    audio_codecs TEXT,
    output_format TEXT NOT NULL DEFAULT 'mp4',
    segment_count INTEGER NOT NULL DEFAULT 0,
    finish_requested INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE dash_segments (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    track_kind TEXT NOT NULL,
    segment_index INTEGER NOT NULL,
    uri TEXT NOT NULL,
    local_path TEXT NOT NULL,
    byte_range_start INTEGER,
    byte_range_length INTEGER,
    init_segment_uri TEXT,
    init_segment_local_path TEXT,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- ============================================================================
-- 10. Metalink
-- ============================================================================

CREATE TABLE metalink_tasks (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    manifest_url TEXT NOT NULL,
    manifest_format TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE metalink_resources (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL REFERENCES task_files(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 999999,
    location TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(file_id, url)
);

-- ============================================================================
-- 11. SFTP known hosts (TOFU)
-- ============================================================================

CREATE TABLE sftp_known_hosts (
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    algorithm TEXT NOT NULL,
    fingerprint_sha256 TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    PRIMARY KEY (host, port)
);

-- ============================================================================
-- 12. Settings (key/value)
-- ============================================================================

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

-- ============================================================================
-- 13. Indexes
-- ============================================================================
--
-- Indexes mirror the cumulative set created across the original migrations.
-- Query-pattern indexes (010) use composite (sort_col, id ASC) tails to support
-- cursor pagination. The partial unique index on tasks(source_key) enforces
-- dedup at the DB level for active tasks (012).

-- tasks indexes
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_protocol ON tasks(protocol);
CREATE INDEX idx_tasks_source_key ON tasks(source_key);
CREATE INDEX idx_tasks_updated_at_desc ON tasks(updated_at DESC);
CREATE INDEX idx_tasks_status_updated_at_desc ON tasks(status, updated_at DESC);
CREATE INDEX idx_tasks_source_key_updated_at_desc ON tasks(source_key, updated_at DESC);
CREATE INDEX idx_tasks_error_code_updated_at_desc ON tasks(error_code, updated_at DESC);
CREATE INDEX idx_tasks_retry_after_at ON tasks(retry_after_at);
-- Transfer options & schedule (005, 007)
CREATE INDEX idx_tasks_queue_position ON tasks(queue_position);
CREATE INDEX idx_tasks_priority ON tasks(priority);
CREATE INDEX idx_tasks_category_key ON tasks(category_key);
CREATE INDEX idx_tasks_obey_schedule_status ON tasks(obey_schedule, status);
-- Queue dispatch order (008)
CREATE INDEX idx_tasks_queue_order ON tasks(status, queue_position, created_at);
-- Cursor pagination (010)
CREATE INDEX idx_tasks_updated_at_id ON tasks(updated_at DESC, id ASC);
CREATE INDEX idx_tasks_created_at_id ON tasks(created_at DESC, id ASC);
CREATE INDEX idx_tasks_status_updated_at_id ON tasks(status, updated_at DESC, id ASC);
CREATE INDEX idx_tasks_source_key_updated_at_id ON tasks(source_key, updated_at DESC, id ASC);
CREATE INDEX idx_tasks_error_code_updated_at_id ON tasks(error_code, updated_at DESC, id ASC);
-- Dedup partial unique index (012): one active task per non-empty source_key.
CREATE UNIQUE INDEX idx_tasks_source_key_active
    ON tasks (source_key)
    WHERE source_key IS NOT NULL
      AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention');

-- task_files
CREATE INDEX idx_task_files_task_id ON task_files(task_id);

-- task_work_units
CREATE INDEX idx_work_units_task_id ON task_work_units(task_id);
CREATE INDEX idx_work_units_file_id ON task_work_units(file_id);
CREATE INDEX idx_work_units_task_id_status ON task_work_units(task_id, status);

-- task_events
CREATE INDEX idx_task_events_task_id_created_at_desc
ON task_events(task_id, created_at DESC);

-- task_requests
CREATE INDEX idx_task_requests_task_id ON task_requests(task_id);
CREATE INDEX idx_task_requests_created_at ON task_requests(created_at);
CREATE INDEX idx_task_requests_task_id_created_at_desc
ON task_requests(task_id, created_at DESC);

-- task_request_headers
CREATE INDEX idx_task_request_headers_expires_at
ON task_request_headers(expires_at);

-- browser_messages
CREATE INDEX idx_browser_messages_browser_created_at
ON browser_messages(browser, created_at DESC);

-- task_credentials
CREATE INDEX idx_task_credentials_protocol
ON task_credentials(protocol);

-- task_checksums
CREATE INDEX idx_task_checksums_task_id ON task_checksums(task_id);
CREATE INDEX idx_task_checksums_status ON task_checksums(status);
CREATE INDEX idx_task_checksums_file_id ON task_checksums(file_id);

-- classification_rules
CREATE INDEX idx_classification_rules_enabled_position
ON classification_rules(enabled, position);

-- torrent
CREATE INDEX idx_torrent_tasks_info_hash
ON torrent_tasks(info_hash);

-- hls
CREATE INDEX idx_hls_tasks_finish_requested
ON hls_tasks(finish_requested);
CREATE UNIQUE INDEX idx_hls_segments_task_sequence
ON hls_segments(task_id, media_sequence, discontinuity_sequence);
CREATE INDEX idx_hls_segments_task_status
ON hls_segments(task_id, status);

-- dash
CREATE INDEX idx_dash_tasks_finish_requested
ON dash_tasks(finish_requested);
CREATE UNIQUE INDEX idx_dash_segments_task_track_index
ON dash_segments(task_id, track_kind, segment_index);
CREATE INDEX idx_dash_segments_task_status
ON dash_segments(task_id, status);

-- metalink
CREATE INDEX idx_metalink_resources_task_id ON metalink_resources(task_id);
CREATE INDEX idx_metalink_resources_file_id_priority ON metalink_resources(file_id, priority);
CREATE INDEX idx_metalink_resources_status ON metalink_resources(status);

-- sftp known hosts
CREATE INDEX idx_sftp_known_hosts_fingerprint
ON sftp_known_hosts(fingerprint_sha256);
