-- Browser handoff headers retain their existing TTL and recovery contract.
-- Task profiles separate lasting public overrides from expiring credentials.
CREATE TABLE task_request_profiles (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    origin TEXT NOT NULL,
    public_ciphertext TEXT NOT NULL,
    public_nonce TEXT NOT NULL,
    sensitive_ciphertext TEXT,
    sensitive_nonce TEXT,
    sensitive_names_json TEXT NOT NULL DEFAULT '[]',
    sensitive_expires_at TEXT,
    sensitive_expired INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_task_request_profiles_expiry ON task_request_profiles(sensitive_expires_at);
