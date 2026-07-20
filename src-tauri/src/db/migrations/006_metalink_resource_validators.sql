-- FUN-09: Persist per-mirror HTTP validators for Metalink resume consistency.
ALTER TABLE metalink_resources ADD COLUMN etag TEXT;
ALTER TABLE metalink_resources ADD COLUMN last_modified TEXT;
