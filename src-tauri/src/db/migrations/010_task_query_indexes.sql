CREATE INDEX IF NOT EXISTS idx_tasks_updated_at_id
ON tasks(updated_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS idx_tasks_created_at_id
ON tasks(created_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS idx_tasks_status_updated_at_id
ON tasks(status, updated_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS idx_tasks_source_key_updated_at_id
ON tasks(source_key, updated_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS idx_tasks_error_code_updated_at_id
ON tasks(error_code, updated_at DESC, id ASC);
