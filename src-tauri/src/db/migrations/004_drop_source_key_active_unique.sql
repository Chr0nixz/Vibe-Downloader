-- ARC-01: drop the host-level active source_key unique index.
-- Different URLs on the same host must be allowed to coexist while active;
-- BT uniqueness remains enforced by torrent_tasks.info_hash UNIQUE.
DROP INDEX IF EXISTS idx_tasks_source_key_active;
