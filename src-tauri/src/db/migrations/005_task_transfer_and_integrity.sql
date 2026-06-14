ALTER TABLE tasks ADD COLUMN task_speed_limit_bps TEXT;
ALTER TABLE tasks ADD COLUMN priority TEXT NOT NULL DEFAULT 'normal';
ALTER TABLE tasks ADD COLUMN queue_position INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN category_key TEXT;

UPDATE tasks
SET queue_position = CAST(strftime('%s', created_at) AS INTEGER) * 1000000 + rowid
WHERE queue_position = 0;

CREATE INDEX idx_tasks_queue_position ON tasks(queue_position);
CREATE INDEX idx_tasks_priority ON tasks(priority);
CREATE INDEX idx_tasks_category_key ON tasks(category_key);

CREATE TABLE task_checksums (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    algorithm TEXT NOT NULL,
    expected_hash TEXT NOT NULL,
    actual_hash TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    source_kind TEXT NOT NULL DEFAULT 'manual',
    source_url TEXT,
    source_label TEXT,
    is_primary INTEGER NOT NULL DEFAULT 0,
    weak INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    discovered_at TEXT,
    verified_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(task_id, algorithm, expected_hash)
);

CREATE INDEX idx_task_checksums_task_id ON task_checksums(task_id);
CREATE INDEX idx_task_checksums_status ON task_checksums(status);

CREATE TABLE classification_rules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    position INTEGER NOT NULL DEFAULT 0,
    match_kind TEXT NOT NULL,
    pattern TEXT NOT NULL,
    target_subdir TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_classification_rules_enabled_position
ON classification_rules(enabled, position);
