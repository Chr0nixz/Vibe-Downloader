# Error Codes

This table documents the structured task error fields used by the backend and UI. UI filters should use `failure_category`; `error_code` remains the stable diagnostic code.

| Code | Category | Recoverable | Actions | User Message | Source |
| --- | --- | --- | --- | --- | --- |
| `remote_changed` | `remote_changed` | Yes | `check_url`, `restart` | Remote file changed; restart from the browser or URL. | Resume validation |
| `resume_unavailable` | `resume_unavailable` | Yes | `restart`, `check_url` | Server no longer supports resume for this task. | Resume validation |
| `temp_file_missing` | `temp_file` | Yes | `restart`, `choose_another_folder` | Temporary file is missing. | Resume validation |
| `temp_file_smaller_than_progress` | `temp_file` | Yes | `restart` | Temporary file is smaller than recorded progress. | Resume validation |
| `disk_write_failed` | `disk_write` | Yes | `free_disk_space`, `choose_another_folder`, `retry` | Could not write to disk. | Download writer |
| `http_*` | `http` | Usually | `retry`, `retry_later`, `check_url` | HTTP request failed. | HTTP engine |
| `server_rate_limited` | `http` | Yes | `retry_later`, `check_url` | Server asked the app to wait before retrying. | HTTP engine |
| `auth_headers_expired` | `auth` | Yes | `check_url`, `restart` | Browser authentication headers expired; send from browser again. | Header restore |
| `auth_headers_unavailable` | `auth` | Yes | `check_url`, `restart` | Stored browser authentication headers are unavailable; send from browser again. | OS key store/header restore |
| `final_path_conflict` | `other` | Yes | `choose_another_name`, `choose_another_folder` | Final output path conflicts with an existing file. | File finalization |

`RetryLater` keeps the task queued and sets `retry_after_at` to five minutes in the future. The scheduler skips queued tasks until the timestamp expires, including after app restart.
