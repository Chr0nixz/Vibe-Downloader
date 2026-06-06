use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

use crate::models::{SegmentStatus, TaskRecord, TaskSegmentRecord, TaskStatus};

pub const MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 16 * 1024 * 1024;
pub const MAX_SEGMENT_COUNT: usize = 4;

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

pub async fn insert_segment_record(
    pool: &SqlitePool,
    segment: &TaskSegmentRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO segments (
            id, task_id, range_start, range_end, downloaded_until,
            status, retry_count, last_error
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&segment.id)
    .bind(&segment.task_id)
    .bind(segment.range_start)
    .bind(segment.range_end)
    .bind(segment.downloaded_until)
    .bind(segment.status.as_str())
    .bind(segment.retry_count)
    .bind(&segment.last_error)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn ensure_single_segment_for_task(
    pool: &SqlitePool,
    task: &TaskRecord,
) -> Result<TaskSegmentRecord, String> {
    ensure_task_segments(pool, task)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "Task segment could not be created.".to_string())
}

pub async fn ensure_task_segments(
    pool: &SqlitePool,
    task: &TaskRecord,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let existing = list_segment_records(pool, &task.id).await?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let segments = planned_segments_for_task(task);
    for segment in &segments {
        insert_segment_record(pool, segment).await?;
    }
    Ok(segments)
}

pub fn planned_segment_count(task: &TaskRecord) -> usize {
    if task.supports_range && task.total_size >= MULTI_CONNECTION_THRESHOLD_BYTES {
        MAX_SEGMENT_COUNT
    } else {
        1
    }
}

pub fn planned_segments_for_task(task: &TaskRecord) -> Vec<TaskSegmentRecord> {
    let count = planned_segment_count(task);
    let total_size = task.total_size.max(0);
    let completed = task.downloaded_bytes >= total_size && total_size > 0;

    if count == 1 || total_size == 0 {
        return vec![TaskSegmentRecord {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            range_start: 0,
            range_end: total_size.saturating_sub(1).max(0),
            downloaded_until: task.downloaded_bytes.max(0),
            status: if completed {
                SegmentStatus::Completed
            } else {
                SegmentStatus::Pending
            },
            retry_count: 0,
            last_error: None,
        }];
    }

    let count_i64 = i64::try_from(count).unwrap_or(1);
    let base = total_size / count_i64;
    let remainder = total_size % count_i64;
    let mut start = 0_i64;

    (0..count)
        .map(|index| {
            let extra = if i64::try_from(index).unwrap_or(0) < remainder {
                1
            } else {
                0
            };
            let length = base + extra;
            let end = start + length - 1;
            let downloaded_until = if completed { end + 1 } else { start };
            let segment = TaskSegmentRecord {
                id: uuid::Uuid::new_v4().to_string(),
                task_id: task.id.clone(),
                range_start: start,
                range_end: end,
                downloaded_until,
                status: if completed {
                    SegmentStatus::Completed
                } else {
                    SegmentStatus::Pending
                },
                retry_count: 0,
                last_error: None,
            };
            start = end + 1;
            segment
        })
        .collect()
}

pub async fn list_segment_records(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, range_start, range_end, downloaded_until,
               status, retry_count, last_error
        FROM segments
        WHERE task_id = ?
        ORDER BY range_start ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_segment).collect()
}

pub async fn get_first_segment_record(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<TaskSegmentRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, task_id, range_start, range_end, downloaded_until,
               status, retry_count, last_error
        FROM segments
        WHERE task_id = ?
        ORDER BY range_start ASC
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    row.as_ref().map(row_to_segment).transpose()
}

fn row_to_segment(row: &sqlx::sqlite::SqliteRow) -> Result<TaskSegmentRecord, String> {
    Ok(TaskSegmentRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        range_start: row.get("range_start"),
        range_end: row.get("range_end"),
        downloaded_until: row.get("downloaded_until"),
        status: SegmentStatus::from_db_str(row.get::<String, _>("status").as_str()),
        retry_count: row.get("retry_count"),
        last_error: row.get("last_error"),
    })
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

pub async fn update_task_and_segment_progress(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
) -> Result<(), String> {
    update_task_progress(
        pool,
        task_id,
        downloaded_bytes,
        speed_bps,
        connection_count,
        status,
    )
    .await?;

    sqlx::query(
        r#"
        UPDATE segments
        SET downloaded_until = ?, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes)
    .bind(SegmentStatus::Downloading.as_str())
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_progress(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
    status: SegmentStatus,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE segments
        SET downloaded_until = ?, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(status.as_str())
    .bind(segment_id)
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

pub async fn complete_task_segment(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
) -> Result<(), String> {
    complete_task(pool, task_id).await?;

    sqlx::query(
        r#"
        UPDATE segments
        SET downloaded_until = range_end + 1, status = 'completed', last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn complete_segment(pool: &SqlitePool, segment_id: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE segments
        SET downloaded_until = range_end + 1, status = 'completed', last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_status(
    pool: &SqlitePool,
    segment_id: &str,
    status: SegmentStatus,
    downloaded_until: Option<i64>,
    last_error: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE segments
        SET status = ?, downloaded_until = COALESCE(?, downloaded_until), last_error = ?
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(downloaded_until)
    .bind(last_error)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segments_status_for_task(
    pool: &SqlitePool,
    task_id: &str,
    status: SegmentStatus,
    last_error: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE segments
        SET status = ?, last_error = ?
        WHERE task_id = ? AND status != 'completed'
        "#,
    )
    .bind(status.as_str())
    .bind(last_error)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn segment_downloaded_bytes(segment: &TaskSegmentRecord) -> i64 {
    let next_byte = segment
        .downloaded_until
        .clamp(segment.range_start, segment.range_end.saturating_add(1));
    next_byte.saturating_sub(segment.range_start)
}

pub fn total_segment_downloaded_bytes(segments: &[TaskSegmentRecord]) -> i64 {
    segments.iter().map(segment_downloaded_bytes).sum()
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

    sqlx::query(
        r#"
        UPDATE segments
        SET status = 'pending'
        WHERE status = 'downloading'
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
