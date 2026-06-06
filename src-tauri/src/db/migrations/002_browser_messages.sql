CREATE TABLE IF NOT EXISTS browser_messages (
    request_id TEXT PRIMARY KEY NOT NULL,
    browser TEXT NOT NULL,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_browser_messages_browser_created_at
ON browser_messages(browser, created_at DESC);
