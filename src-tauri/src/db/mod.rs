use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

use crate::models::{TaskRecord, TaskStatus};

pub async fn connect(db_path: &std::path::Path) -> Result<SqlitePool, String> {
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map_err(|e| format!("Database connection failed: {e}"))?;

    sqlx::migrate!("./src/db/migrations")
        .run(&pool)
        .await
        .map_err(|e| format!("Migration failed: {e}"))?;

    Ok(pool)
}

pub async fn list_task_records(pool: &SqlitePool) -> Result<Vec<TaskRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, url, final_url, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_range, source_host, connection_count, speed_bps,
               health_summary, error_message, created_at, updated_at
        FROM tasks
        ORDER BY
            CASE status
                WHEN 'downloading' THEN 0
                WHEN 'retrying' THEN 1
                WHEN 'queued' THEN 2
                WHEN 'paused' THEN 3
                WHEN 'waiting_network' THEN 4
                WHEN 'needs_attention' THEN 5
                WHEN 'failed' THEN 6
                ELSE 7
            END,
            updated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_task).collect()
}

pub async fn get_task_record(pool: &SqlitePool, id: &str) -> Result<Option<TaskRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, url, final_url, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_range, source_host, connection_count, speed_bps,
               health_summary, error_message, created_at, updated_at
        FROM tasks WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    row.as_ref().map(row_to_task).transpose()
}

fn row_to_task(row: &sqlx::sqlite::SqliteRow) -> Result<TaskRecord, String> {
    Ok(TaskRecord {
        id: row.get("id"),
        url: row.get("url"),
        final_url: row.get("final_url"),
        file_name: row.get("file_name"),
        save_dir: row.get("save_dir"),
        temp_path: row.get("temp_path"),
        final_path: row.get("final_path"),
        total_size: row.get("total_size"),
        downloaded_bytes: row.get("downloaded_bytes"),
        status: TaskStatus::from_db_str(row.get::<String, _>("status").as_str()),
        etag: row.get("etag"),
        last_modified: row.get("last_modified"),
        content_type: row.get("content_type"),
        supports_range: row.get::<i64, _>("supports_range") != 0,
        source_host: row.get("source_host"),
        connection_count: row.get("connection_count"),
        speed_bps: row.get("speed_bps"),
        health_summary: row.get("health_summary"),
        error_message: row.get("error_message"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn insert_task_record(pool: &SqlitePool, task: &TaskRecord) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, url, final_url, file_name, save_dir, temp_path, final_path,
            total_size, downloaded_bytes, status, etag, last_modified, content_type,
            supports_range, source_host, connection_count, speed_bps,
            health_summary, error_message, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task.id)
    .bind(&task.url)
    .bind(&task.final_url)
    .bind(&task.file_name)
    .bind(&task.save_dir)
    .bind(&task.temp_path)
    .bind(&task.final_path)
    .bind(task.total_size)
    .bind(task.downloaded_bytes)
    .bind(task.status.as_str())
    .bind(&task.etag)
    .bind(&task.last_modified)
    .bind(&task.content_type)
    .bind(if task.supports_range { 1 } else { 0 })
    .bind(&task.source_host)
    .bind(task.connection_count)
    .bind(task.speed_bps)
    .bind(&task.health_summary)
    .bind(&task.error_message)
    .bind(&task.created_at)
    .bind(&task.updated_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn clear_tasks(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("DELETE FROM task_events")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM segments")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM tasks")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_progress(
    pool: &SqlitePool,
    task_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = ?, speed_bps = ?, connection_count = ?, status = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes)
    .bind(speed_bps)
    .bind(connection_count)
    .bind(status.as_str())
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_task_status(
    pool: &SqlitePool,
    task_id: &str,
    status: TaskStatus,
    speed_bps: i64,
    connection_count: i32,
    health_summary: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = ?, speed_bps = ?, connection_count = ?,
            health_summary = ?, error_message = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(speed_bps)
    .bind(connection_count)
    .bind(health_summary)
    .bind(error_message)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn complete_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'completed', downloaded_bytes = total_size, speed_bps = 0,
            connection_count = 0, health_summary = 'Completed',
            error_message = NULL, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn delete_task_record(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn reset_interrupted_tasks(pool: &SqlitePool) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'paused', speed_bps = 0, connection_count = 0,
            health_summary = 'Paused after app restart', error_message = NULL,
            updated_at = ?
        WHERE status IN ('downloading', 'retrying')
        "#,
    )
    .bind(&updated_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
