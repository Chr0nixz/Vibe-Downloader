use sqlx::{Row, SqlitePool};

use crate::models::task::now_iso;

#[derive(Debug, Clone)]
pub struct MetalinkTaskUpsert<'a> {
    pub task_id: &'a str,
    pub manifest_url: &'a str,
    pub manifest_format: &'a str,
    pub file_count: i64,
}

#[derive(Debug, Clone)]
pub struct MetalinkResourceRecord {
    pub id: String,
    pub task_id: String,
    pub file_id: String,
    pub url: String,
    pub priority: i64,
    pub location: Option<String>,
    pub status: String,
    pub failure_count: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MetalinkResourceInsert<'a> {
    pub id: &'a str,
    pub task_id: &'a str,
    pub file_id: &'a str,
    pub url: &'a str,
    pub priority: i64,
    pub location: Option<&'a str>,
}

pub async fn upsert_metalink_task(
    pool: &SqlitePool,
    input: MetalinkTaskUpsert<'_>,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        INSERT INTO metalink_tasks (
            task_id, manifest_url, manifest_format, file_count, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            manifest_url = excluded.manifest_url,
            manifest_format = excluded.manifest_format,
            file_count = excluded.file_count,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(input.task_id)
    .bind(input.manifest_url)
    .bind(input.manifest_format)
    .bind(input.file_count.max(0))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn insert_metalink_resource(
    pool: &SqlitePool,
    input: MetalinkResourceInsert<'_>,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO metalink_resources (
            id, task_id, file_id, url, priority, location, status,
            failure_count, last_error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'pending', 0, NULL, ?, ?)
        "#,
    )
    .bind(input.id)
    .bind(input.task_id)
    .bind(input.file_id)
    .bind(input.url)
    .bind(input.priority)
    .bind(input.location)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list_metalink_resources_for_file(
    pool: &SqlitePool,
    file_id: &str,
) -> Result<Vec<MetalinkResourceRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, url, priority, location, status,
               failure_count, last_error
        FROM metalink_resources
        WHERE file_id = ?
        ORDER BY priority ASC, rowid ASC
        "#,
    )
    .bind(file_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(row_to_resource).collect())
}

pub async fn mark_metalink_resource_failed(
    pool: &SqlitePool,
    id: &str,
    error: &str,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET status = 'failed',
            failure_count = failure_count + 1,
            last_error = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(error)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn mark_metalink_resource_completed(pool: &SqlitePool, id: &str) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET status = 'completed',
            last_error = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn row_to_resource(row: sqlx::sqlite::SqliteRow) -> MetalinkResourceRecord {
    MetalinkResourceRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        file_id: row.get("file_id"),
        url: row.get("url"),
        priority: row.get("priority"),
        location: row.get("location"),
        status: row.get("status"),
        failure_count: row.get("failure_count"),
        last_error: row.get("last_error"),
    }
}

pub async fn list_metalink_resources_for_task(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<MetalinkResourceRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, url, priority, location, status,
               failure_count, last_error
        FROM metalink_resources
        WHERE task_id = ?
        ORDER BY priority ASC, rowid ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(row_to_resource).collect())
}

pub async fn reset_metalink_resource_statuses(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET status = 'pending', failure_count = 0, last_error = NULL, updated_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(&now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Boosts the preferred mirror's priority to 0 (tried first) so the Metalink
/// engine selects it before other mirrors on the next download attempt.
pub async fn promote_metalink_resource_for_retry(
    pool: &SqlitePool,
    task_id: &str,
    mirror_url: &str,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET priority = 0, updated_at = ?
        WHERE task_id = ? AND url = ?
        "#,
    )
    .bind(&now)
    .bind(task_id)
    .bind(mirror_url)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
