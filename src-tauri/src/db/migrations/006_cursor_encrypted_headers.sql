ALTER TABLE task_request_headers ADD COLUMN headers_ciphertext TEXT;
ALTER TABLE task_request_headers ADD COLUMN nonce TEXT;

CREATE INDEX IF NOT EXISTS idx_tasks_error_code_updated_at_desc
ON tasks(error_code, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_retry_after_at
ON tasks(retry_after_at);
