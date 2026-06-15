PRAGMA foreign_keys = OFF;

CREATE TABLE task_checksums_new (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id TEXT REFERENCES task_files(id) ON DELETE CASCADE,
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
    UNIQUE(task_id, file_id, algorithm, expected_hash)
);

INSERT INTO task_checksums_new (
    id, task_id, file_id, algorithm, expected_hash, actual_hash, status,
    source_kind, source_url, source_label, is_primary, weak,
    error_message, discovered_at, verified_at, created_at, updated_at
)
SELECT
    id, task_id, NULL, algorithm, expected_hash, actual_hash, status,
    source_kind, source_url, source_label, is_primary, weak,
    error_message, discovered_at, verified_at, created_at, updated_at
FROM task_checksums;

DROP TABLE task_checksums;
ALTER TABLE task_checksums_new RENAME TO task_checksums;

CREATE INDEX idx_task_checksums_task_id ON task_checksums(task_id);
CREATE INDEX idx_task_checksums_file_id ON task_checksums(file_id);
CREATE INDEX idx_task_checksums_status ON task_checksums(status);

PRAGMA foreign_keys = ON;

CREATE TABLE metalink_tasks (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    manifest_url TEXT NOT NULL,
    manifest_format TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE metalink_resources (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL REFERENCES task_files(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 999999,
    location TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(file_id, url)
);

CREATE INDEX idx_metalink_resources_task_id ON metalink_resources(task_id);
CREATE INDEX idx_metalink_resources_file_id_priority ON metalink_resources(file_id, priority);
CREATE INDEX idx_metalink_resources_status ON metalink_resources(status);
