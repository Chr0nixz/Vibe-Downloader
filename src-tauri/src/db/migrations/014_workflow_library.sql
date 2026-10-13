CREATE TABLE workflow_library (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    format_version INTEGER NOT NULL,
    payload TEXT NOT NULL
);
