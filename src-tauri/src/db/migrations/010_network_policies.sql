-- Missing rows intentionally retain public-only access for legacy tasks.
CREATE TABLE task_network_policies (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    policy_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE network_authorizations (
    id TEXT PRIMARY KEY NOT NULL,
    policy_json TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
