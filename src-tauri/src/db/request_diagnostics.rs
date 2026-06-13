use sqlx::{Row, SqlitePool};

use crate::models::{RequestDiagnostic, RequestDiagnosticRecord};

pub async fn insert_request_diagnostic(
    pool: &SqlitePool,
    record: &RequestDiagnosticRecord,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO task_requests (
            task_id, method, url, range_header, status_code, etag, last_modified,
            content_length, error_message, retry_count, duration_ms, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.task_id)
    .bind(&record.method)
    .bind(&record.url)
    .bind(&record.range_header)
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
        SELECT id, task_id, method, url, range_header, status_code, etag, last_modified,
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
