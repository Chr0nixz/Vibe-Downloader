-- Rollback for migration 005: ARC-02 final_path active unique indexes.
-- sqlx::migrate! does NOT auto-execute files in migrations/rollback/.
--
--     DROP INDEX IF EXISTS idx_tasks_final_path_active;
--     DROP INDEX IF EXISTS idx_task_files_final_path_selected;
SELECT 1;
