-- B6: persist independent seeding state and media selection policy.
-- These fields are deliberately additive so existing tasks retain their
-- historical defaults and can be resumed without a data rewrite.

ALTER TABLE torrent_tasks ADD COLUMN seeding_started_at TEXT;
ALTER TABLE torrent_tasks ADD COLUMN seed_uploaded_bytes INTEGER NOT NULL DEFAULT 0;
ALTER TABLE torrent_tasks ADD COLUMN seed_stop_reason TEXT;
ALTER TABLE torrent_tasks ADD COLUMN seed_manual_stop INTEGER NOT NULL DEFAULT 0;
ALTER TABLE torrent_tasks ADD COLUMN seed_elapsed_seconds INTEGER NOT NULL DEFAULT 0;

ALTER TABLE hls_tasks ADD COLUMN max_missing_segments INTEGER NOT NULL DEFAULT 0;
ALTER TABLE hls_tasks ADD COLUMN missing_segments_json TEXT;
ALTER TABLE hls_tasks ADD COLUMN selected_track_catalog_json TEXT;

ALTER TABLE dash_tasks ADD COLUMN selected_subtitle_representation_id TEXT;
ALTER TABLE dash_tasks ADD COLUMN track_catalog_json TEXT;
