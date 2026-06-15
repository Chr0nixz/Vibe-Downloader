CREATE INDEX IF NOT EXISTS idx_tasks_queue_order
ON tasks(status, queue_position, created_at);
