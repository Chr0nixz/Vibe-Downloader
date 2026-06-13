CREATE TABLE task_credentials (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    protocol TEXT NOT NULL,
    credentials_ciphertext TEXT NOT NULL,
    nonce TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_task_credentials_protocol
ON task_credentials(protocol);
