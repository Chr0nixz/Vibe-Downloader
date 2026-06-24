-- 012_dedup_unique.sql
-- Prevent concurrent duplicate task creation at the database level.
--
-- The application-level dedup check (find_duplicate_task_record → insert_task_record)
-- has a TOCTOU race: two concurrent creates can both pass the SELECT before either
-- INSERTs, resulting in two active tasks writing to the same .vibe-downloading file.
--
-- This partial unique index acts as a database-level safety net for tasks that share
-- a non-empty source_key (BitTorrent info-hash, HLS/DASH manifest URLs, etc.).
-- The WHERE clause limits enforcement to active (non-terminal) statuses so that
-- completed/failed tasks with the same source_key are allowed.
--
-- For HTTP tasks with empty source_key, the race is closed by wrapping the
-- find-duplicate + insert in an IMMEDIATE transaction in the create flow.

-- Pre-cleanup: if there are duplicate active tasks sharing the same source_key,
-- keep only the most recently created one and remove the rest.
DELETE FROM task_events WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM task_files WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM task_work_units WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM task_request_headers WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM task_requests WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
-- Defense-in-depth: clean up remaining child tables that have ON DELETE CASCADE.
-- If foreign_keys happens to be OFF during migration, these explicit deletes
-- prevent orphaned rows (especially encrypted credentials in task_credentials).
DELETE FROM task_credentials WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM task_proxy_settings WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM task_checksums WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM hls_segments WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM hls_tasks WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM metalink_resources WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM metalink_tasks WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM torrent_runtime_snapshots WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM torrent_tasks WHERE task_id IN (
    SELECT id FROM tasks
    WHERE source_key IS NOT NULL AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      AND id NOT IN (
          SELECT id FROM (
              SELECT id, ROW_NUMBER() OVER (
                  PARTITION BY source_key ORDER BY created_at DESC, id
              ) AS rn
              FROM tasks
              WHERE source_key IS NOT NULL AND source_key != ''
                AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
          ) WHERE rn = 1
      )
);
DELETE FROM tasks
WHERE source_key IS NOT NULL AND source_key != ''
  AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
  AND id NOT IN (
      SELECT id FROM (
          SELECT id, ROW_NUMBER() OVER (
              PARTITION BY source_key ORDER BY created_at DESC, id
          ) AS rn
          FROM tasks
          WHERE source_key IS NOT NULL AND source_key != ''
            AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention')
      ) WHERE rn = 1
  );

CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_source_key_active
    ON tasks (source_key)
    WHERE source_key IS NOT NULL
      AND source_key != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention');
