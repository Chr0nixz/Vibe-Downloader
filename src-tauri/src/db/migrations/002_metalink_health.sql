-- Migration 002: Metalink mirror health tracking
--
-- Adds per-mirror health fields so the Metalink engine can:
--  - Skip mirrors in cooldown after a recent failure (`cooldown_until`)
--  - Track observed throughput for mirror selection (`avg_speed_bps`)
--  - Remember mirrors that do not support HTTP Range requests (`supports_range`)
--  - Skip mirrors that were just attempted and failed (`last_attempt_at`)
--
-- These fields are read by `list_healthy_mirrors_for_file` and written by
-- `update_mirror_speed`, `mark_mirror_unsupported_range`, and the worker
-- retry path in `download_metalink_file_parallel`.
--
-- Defaults are chosen so existing rows behave exactly as before:
--  - `supports_range = 1` (assume Range works until proven otherwise)
--  - `last_attempt_at = NULL` and `cooldown_until = NULL` (no cooldown)

ALTER TABLE metalink_resources ADD COLUMN last_attempt_at TEXT;
ALTER TABLE metalink_resources ADD COLUMN cooldown_until TEXT;
ALTER TABLE metalink_resources ADD COLUMN avg_speed_bps INTEGER NOT NULL DEFAULT 0;
ALTER TABLE metalink_resources ADD COLUMN supports_range INTEGER NOT NULL DEFAULT 1;
