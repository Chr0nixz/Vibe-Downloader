DROP INDEX IF EXISTS idx_tasks_completed_at_desc;

-- SQLite cannot drop a column on all supported versions. Rollback tooling must
-- rebuild the tasks table explicitly if this migration is ever reverted.
