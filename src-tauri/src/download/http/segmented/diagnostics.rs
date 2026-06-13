use std::time::Duration;

use reqwest::header::{CONTENT_LENGTH, ETAG, LAST_MODIFIED};
use sqlx::SqlitePool;

use crate::{
    db,
    logging::sanitize_url,
    models::{RequestDiagnosticRecord, TaskRecord},
};

pub(in crate::download::http::segmented) struct RequestDiagnosticContext<'a> {
    pub(in crate::download::http::segmented) pool: &'a SqlitePool,
    pub(in crate::download::http::segmented) task_id: &'a str,
    pub(in crate::download::http::segmented) method: &'a str,
    pub(in crate::download::http::segmented) url: &'a str,
    pub(in crate::download::http::segmented) range_header: Option<String>,
    pub(in crate::download::http::segmented) if_range_header: Option<String>,
    pub(in crate::download::http::segmented) retry_count: i32,
    pub(in crate::download::http::segmented) duration: Duration,
}

pub(in crate::download::http::segmented) async fn persist_response_diagnostic(
    context: RequestDiagnosticContext<'_>,
    response: &reqwest::Response,
) {
    let RequestDiagnosticContext {
        pool,
        task_id,
        method,
        url,
        range_header,
        if_range_header,
        retry_count,
        duration,
    } = context;
    let record = response_diagnostic_record(
        task_id,
        method,
        url,
        range_header,
        if_range_header,
        response,
        retry_count,
        duration,
    );
    if let Err(error) = db::insert_request_diagnostic(pool, &record).await {
        tracing::warn!(task_id, error = %error, "failed to persist request diagnostic");
    }
}

pub(in crate::download::http::segmented) async fn persist_error_diagnostic(
    context: RequestDiagnosticContext<'_>,
    error_message: &str,
) {
    let RequestDiagnosticContext {
        pool,
        task_id,
        method,
        url,
        range_header,
        if_range_header,
        retry_count,
        duration,
    } = context;
    let record = RequestDiagnosticRecord {
        task_id: task_id.to_string(),
        method: method.to_string(),
        url: sanitize_url(url),
        range_header,
        if_range_header,
        status_code: None,
        etag: None,
        last_modified: None,
        content_length: None,
        error_message: Some(error_message.to_string()),
        retry_count,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    };
    if let Err(error) = db::insert_request_diagnostic(pool, &record).await {
        tracing::warn!(task_id, error = %error, "failed to persist request diagnostic");
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::download::http::segmented) fn response_diagnostic_record(
    task_id: &str,
    method: &str,
    url: &str,
    range_header: Option<String>,
    if_range_header: Option<String>,
    response: &reqwest::Response,
    retry_count: i32,
    duration: Duration,
) -> RequestDiagnosticRecord {
    let headers = response.headers();
    RequestDiagnosticRecord {
        task_id: task_id.to_string(),
        method: method.to_string(),
        url: sanitize_url(response.url().as_str()).if_empty(|| sanitize_url(url)),
        range_header,
        if_range_header,
        status_code: Some(i32::from(response.status().as_u16())),
        etag: headers
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        last_modified: headers
            .get(LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        content_length: headers
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok()),
        error_message: None,
        retry_count,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::download::http::segmented) fn error_diagnostic_record(
    task_id: &str,
    method: &str,
    url: &str,
    range_header: Option<String>,
    if_range_header: Option<String>,
    error_message: &str,
    retry_count: i32,
    duration: Duration,
) -> RequestDiagnosticRecord {
    RequestDiagnosticRecord {
        task_id: task_id.to_string(),
        method: method.to_string(),
        url: sanitize_url(url),
        range_header,
        if_range_header,
        status_code: None,
        etag: None,
        last_modified: None,
        content_length: None,
        error_message: Some(error_message.to_string()),
        retry_count,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::download::http) struct ParsedContentRange {
    pub(in crate::download::http) start: i64,
    pub(in crate::download::http) end: i64,
    pub(in crate::download::http) total: i64,
}

pub(in crate::download::http) fn parse_content_range(value: &str) -> Option<ParsedContentRange> {
    let value = value.trim();
    let (unit, value) = value.split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some(ParsedContentRange {
        start: start.trim().parse::<i64>().ok()?,
        end: end.trim().parse::<i64>().ok()?,
        total: total.trim().parse::<i64>().ok()?,
    })
}

pub(in crate::download::http::segmented) fn if_range_header_value(
    task: &TaskRecord,
) -> Option<String> {
    if_range_header_from(task.etag.as_deref(), task.last_modified.as_deref())
}

pub(in crate::download::http) fn if_range_header_from(
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Option<String> {
    etag.filter(|etag| !is_weak_etag(etag))
        .map(str::to_string)
        .or_else(|| last_modified.map(str::to_string))
}

fn is_weak_etag(value: &str) -> bool {
    value.trim_start().starts_with("W/") || value.trim_start().starts_with("w/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HashVerificationStatus, TaskKind, TaskStatus};

    #[test]
    fn parses_valid_content_range() {
        assert_eq!(
            parse_content_range("bytes 100-199/1000"),
            Some(ParsedContentRange {
                start: 100,
                end: 199,
                total: 1000,
            })
        );
        assert_eq!(
            parse_content_range("Bytes 0-0/10"),
            Some(ParsedContentRange {
                start: 0,
                end: 0,
                total: 10,
            })
        );
    }

    #[test]
    fn rejects_invalid_content_range_forms() {
        assert!(parse_content_range("bytes 100-199/*").is_none());
        assert!(parse_content_range("items 100-199/1000").is_none());
        assert!(parse_content_range("bytes 100/1000").is_none());
    }

    #[test]
    fn if_range_prefers_strong_etag() {
        let mut task =
            task_record_with_validators(Some("\"strong\""), Some("Tue, 02 Jan 2024 00:00:00 GMT"));

        assert_eq!(if_range_header_value(&task).as_deref(), Some("\"strong\""));

        task.etag = Some("W/\"weak\"".to_string());
        assert_eq!(
            if_range_header_value(&task).as_deref(),
            Some("Tue, 02 Jan 2024 00:00:00 GMT")
        );

        task.etag = None;
        assert_eq!(
            if_range_header_value(&task).as_deref(),
            Some("Tue, 02 Jan 2024 00:00:00 GMT")
        );
    }

    #[test]
    fn error_diagnostic_record_keeps_if_range_header() {
        let record = error_diagnostic_record(
            "task",
            "GET",
            "https://example.com/file.bin",
            Some("bytes=10-99".to_string()),
            Some("\"strong\"".to_string()),
            "failed",
            2,
            Duration::from_millis(35),
        );

        assert_eq!(record.range_header.as_deref(), Some("bytes=10-99"));
        assert_eq!(record.if_range_header.as_deref(), Some("\"strong\""));
        assert_eq!(record.retry_count, 2);
        assert_eq!(record.duration_ms, 35);
    }

    fn task_record_with_validators(etag: Option<&str>, last_modified: Option<&str>) -> TaskRecord {
        TaskRecord {
            id: "task".to_string(),
            url: "https://example.com/file.bin".to_string(),
            final_url: Some("https://example.com/file.bin".to_string()),
            protocol: "https".to_string(),
            task_kind: TaskKind::SingleFile,
            file_name: "file.bin".to_string(),
            save_dir: ".".to_string(),
            temp_path: Some("file.bin.part".to_string()),
            final_path: Some("file.bin".to_string()),
            total_size: 1000,
            downloaded_bytes: 0,
            status: TaskStatus::Queued,
            etag: etag.map(str::to_string),
            last_modified: last_modified.map(str::to_string),
            content_type: None,
            supports_resume: true,
            supports_parallel: true,
            supports_multi_file: false,
            source_key: "example.com".to_string(),
            connection_count: 0,
            speed_bps: 0,
            health_summary: None,
            error_message: None,
            error_code: None,
            recovery_actions: Vec::new(),
            retry_after_at: None,
            expected_hash_sha256: None,
            actual_hash_sha256: None,
            hash_status: HashVerificationStatus::NotRequested,
            hash_error: None,
            hash_verified_at: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }
}

trait EmptyFallback {
    fn if_empty<F: FnOnce() -> String>(self, fallback: F) -> String;
}

impl EmptyFallback for String {
    fn if_empty<F: FnOnce() -> String>(self, fallback: F) -> String {
        if self.trim().is_empty() {
            fallback()
        } else {
            self
        }
    }
}
