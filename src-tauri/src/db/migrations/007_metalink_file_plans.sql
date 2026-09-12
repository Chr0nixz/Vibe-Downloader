-- ARC-34: persist the parallel download plan identity for Metalink files so a
-- resume never recomputes range boundaries from the current healthy-mirror
-- count. Boundaries are a pure function of (total_size, worker_count); if the
-- stored identity differs from the fresh computation, the plan is stale and
-- every part file is discarded before a new plan is written.
CREATE TABLE IF NOT EXISTS metalink_file_plans (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL,
    worker_count INTEGER NOT NULL,
    total_size INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (task_id, file_id)
);
