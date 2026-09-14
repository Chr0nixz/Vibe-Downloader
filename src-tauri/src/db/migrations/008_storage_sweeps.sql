-- Storage & Cleanup Center: persisted record of artifact sweeps (startup and
-- manual) so the page can show what the last sweep removed and when. Rows are
-- pruned to a small recent window by db::insert_sweep_record. Artifact
-- classification lives in download/artifacts.rs — this table only records
-- outcomes, never paths or ownership decisions.
CREATE TABLE IF NOT EXISTS storage_sweeps (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    finished_at TEXT NOT NULL,
    mode TEXT NOT NULL,
    removed_count INTEGER NOT NULL,
    failed_count INTEGER NOT NULL,
    reclaimed_bytes INTEGER NOT NULL,
    details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_storage_sweeps_finished
    ON storage_sweeps(finished_at DESC);
