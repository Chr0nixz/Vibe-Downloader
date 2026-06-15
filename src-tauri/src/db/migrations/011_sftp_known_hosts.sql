CREATE TABLE IF NOT EXISTS sftp_known_hosts (
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    algorithm TEXT NOT NULL,
    fingerprint_sha256 TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    PRIMARY KEY (host, port)
);

CREATE INDEX IF NOT EXISTS idx_sftp_known_hosts_fingerprint
ON sftp_known_hosts(fingerprint_sha256);
