-- ARC-02: atomically reserve final output paths for active tasks so concurrent
-- same-name creates cannot share or overwrite each other.
CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_final_path_active
    ON tasks (final_path)
    WHERE final_path IS NOT NULL
      AND final_path != ''
      AND status IN ('queued', 'downloading', 'retrying', 'paused', 'waiting_network', 'needs_attention');

CREATE UNIQUE INDEX IF NOT EXISTS idx_task_files_final_path_selected
    ON task_files (final_path)
    WHERE final_path IS NOT NULL
      AND final_path != ''
      AND selected = 1;
