//! Shared request diagnostic persistence for download engines.
//!
//! HTTP segmented downloads use `http::segmented::diagnostics` which is tightly
//! coupled to reqwest responses. The other engines (FTP, HLS, DASH, SFTP,
//! Metalink) don't have reqwest responses and previously each duplicated a
//! `persist_*_diagnostic` helper. This module provides a single shared entry
//! point so those engines can stop repeating the same boilerplate.

use std::time::Duration;

use sqlx::SqlitePool;

use crate::{
    db,
    logging::sanitize_url,
    models::RequestDiagnosticRecord,
};

/// Context for persisting an engine-level request diagnostic.
///
/// All fields mirror [`RequestDiagnosticRecord`] except `if_range_header`,
/// `etag`, and `last_modified` which are always `None` for non-HTTP engines.
#[derive(Debug, Clone)]
pub(crate) struct EngineDiagnosticContext<'a> {
    pub(crate) pool: &'a SqlitePool,
    pub(crate) task_id: &'a str,
    pub(crate) method: &'a str,
    pub(crate) url: &'a str,
    pub(crate) range_header: Option<String>,
    pub(crate) status_code: Option<i32>,
    pub(crate) content_length: Option<i64>,
    pub(crate) error: Option<&'a str>,
    pub(crate) retry_count: i32,
    pub(crate) duration: Duration,
}

/// Persist a request diagnostic record for a non-HTTP engine.
///
/// Failures are logged at `warn` level and swallowed — diagnostic persistence
/// must never break the download path.
pub(crate) async fn persist_engine_diagnostic(context: EngineDiagnosticContext<'_>) {
    let EngineDiagnosticContext {
        pool,
        task_id,
        method,
        url,
        range_header,
        status_code,
        content_length,
        error,
        retry_count,
        duration,
    } = context;
    let record = RequestDiagnosticRecord {
        task_id: task_id.to_string(),
        method: method.to_string(),
        url: sanitize_url(url),
        range_header,
        if_range_header: None,
        status_code,
        etag: None,
        last_modified: None,
        content_length,
        error_message: error.map(str::to_string),
        retry_count,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    };
    if let Err(error) = db::insert_request_diagnostic(pool, &record).await {
        tracing::warn!(task_id, error = %error, "failed to persist request diagnostic");
    }
}
