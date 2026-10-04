-- U09: keep completion time independent from the mutable task update time.
-- Existing rows intentionally remain NULL because their exact completion time
-- cannot be reconstructed safely after event retention or database restore.
ALTER TABLE tasks ADD COLUMN completed_at TEXT;

CREATE INDEX idx_tasks_completed_at_desc ON tasks(completed_at DESC);
