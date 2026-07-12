use sqlx::{Row, SqlitePool};

use crate::models::{RequestDiagnostic, RequestDiagnosticRecord};

/// Maximum age (in days) for retained request diagnostic rows.
/// Rows older than this are removed by [`prune_request_diagnostics`].
pub const REQUEST_DIAGNOSTICS_MAX_AGE_DAYS: i64 = 14;

/// Maximum number of request diagnostic rows kept per task.
/// When a task exceeds this count, only the most recent rows are retained.
pub const REQUEST_DIAGNOSTICS_MAX_PER_TASK: i64 = 200;

pub async fn insert_request_diagnostic(
    pool: &SqlitePool,
    record: &RequestDiagnosticRecord,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO task_requests (
            task_id, method, url, range_header, if_range_header, status_code, etag, last_modified,
            content_length, error_message, retry_count, duration_ms, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.task_id)
    .bind(&record.method)
    .bind(&record.url)
    .bind(&record.range_header)
    .bind(&record.if_range_header)
    .bind(record.status_code)
    .bind(&record.etag)
    .bind(&record.last_modified)
    .bind(record.content_length)
    .bind(&record.error_message)
    .bind(record.retry_count)
    .bind(record.duration_ms)
    .bind(&created_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn list_request_diagnostics_page(
    pool: &SqlitePool,
    task_id: &str,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<RequestDiagnostic>, String> {
    let limit = limit.clamp(1, 500);
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, method, url, range_header, if_range_header, status_code, etag, last_modified,
               content_length, error_message, retry_count, duration_ms, created_at
        FROM task_requests
        WHERE task_id = ? AND (? IS NULL OR id < ?)
        ORDER BY id DESC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(before_id)
    .bind(before_id)
    .bind(limit + 1)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|row| RequestDiagnostic {
            id: row.get::<i64, _>("id").to_string(),
            task_id: row.get("task_id"),
            method: row.get("method"),
            url: row.get("url"),
            range_header: row.get("range_header"),
            if_range_header: row.get("if_range_header"),
            status_code: row.get("status_code"),
            etag: row.get("etag"),
            last_modified: row.get("last_modified"),
            content_length: row
                .get::<Option<i64>, _>("content_length")
                .map(|value| value.to_string()),
            error_message: row.get("error_message"),
            retry_count: row.get("retry_count"),
            duration_ms: row.get::<i64, _>("duration_ms").to_string(),
            created_at: row.get("created_at"),
        })
        .collect())
}

/// Prunes stale request diagnostic rows to keep the `task_requests` table bounded.
///
/// Two retention rules are applied:
/// 1. **Age cap**: rows whose `created_at` is older than
///    [`REQUEST_DIAGNOSTICS_MAX_AGE_DAYS`] days are deleted.
/// 2. **Per-task cap**: for each task, only the most recent
///    [`REQUEST_DIAGNOSTICS_MAX_PER_TASK`] rows are kept; older surplus rows
///    are deleted even if they fall within the age window.
///
/// Returns the total number of rows deleted. Failures of individual steps are
/// returned as errors; partial deletions from the age cap may have already
/// committed before a per-task-cap error surfaces.
pub async fn prune_request_diagnostics(pool: &SqlitePool) -> Result<u64, String> {
    // Step 1: age-based deletion. RFC 3339 lexical ordering matches SQLite
    // TEXT comparison because `now_iso()` emits a fixed-offset UTC string.
    let cutoff = chrono::Utc::now() - chrono::Duration::days(REQUEST_DIAGNOSTICS_MAX_AGE_DAYS);
    let cutoff_iso = cutoff.to_rfc3339();
    let age_result = sqlx::query("DELETE FROM task_requests WHERE created_at < ?")
        .bind(&cutoff_iso)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut removed = age_result.rows_affected();

    // Step 2: per-task cap. SQLite supports ROW_NUMBER() (>= 3.25), so we
    // rank rows within each task_id partition by descending id and delete
    // any row whose rank exceeds the cap.
    let cap_result = sqlx::query(
        r#"
        DELETE FROM task_requests
        WHERE id IN (
            SELECT id FROM (
                SELECT id,
                       ROW_NUMBER() OVER (
                           PARTITION BY task_id
                           ORDER BY id DESC
                       ) AS rn
                FROM task_requests
            )
            WHERE rn > ?
        )
        "#,
    )
    .bind(REQUEST_DIAGNOSTICS_MAX_PER_TASK)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    removed += cap_result.rows_affected();
    Ok(removed)
}
