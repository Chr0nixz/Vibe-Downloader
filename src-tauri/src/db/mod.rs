use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

use crate::models::{
    AppSettings, BrowserKind, SegmentStatus, TaskRecord, TaskSegmentRecord, TaskStatus,
};

pub const DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 16 * 1024 * 1024;
pub const MIN_MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 0;
pub const MAX_MULTI_CONNECTION_THRESHOLD_BYTES: i64 = 1024_i64 * 1024 * 1024 * 1024;
pub const DEFAULT_SEGMENT_COUNT: i32 = 4;
pub const MIN_SEGMENT_COUNT: i32 = 1;
pub const MAX_SEGMENT_COUNT: i32 = 8;
pub const MAX_AUTO_SEGMENT_COUNT: usize = 8;
pub const DEFAULT_MAX_CONNECTIONS_PER_HOST: i32 = 8;
pub const MIN_MAX_CONNECTIONS_PER_HOST: i32 = 1;
pub const MAX_MAX_CONNECTIONS_PER_HOST: i32 = 16;
pub const DEFAULT_MAX_ACTIVE_TASKS: i32 = 2;
pub const MIN_MAX_ACTIVE_TASKS: i32 = 1;
pub const MAX_MAX_ACTIVE_TASKS: i32 = 8;
const SETTING_MAX_ACTIVE_TASKS: &str = "max_active_tasks";
const SETTING_DEFAULT_SAVE_DIR: &str = "default_save_dir";
const SETTING_GLOBAL_SPEED_LIMIT_BPS: &str = "global_speed_limit_bps";
const SETTING_MULTI_CONNECTION_THRESHOLD_BYTES: &str = "multi_connection_threshold_bytes";
const SETTING_SEGMENT_COUNT: &str = "segment_count";
const SETTING_MAX_CONNECTIONS_PER_HOST: &str = "max_connections_per_host";

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

    tracing::info!(db_path = %db_path.display(), "database connected and migrations applied");
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

pub async fn list_queued_task_records(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<TaskRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, url, final_url, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_range, source_host, connection_count, speed_bps,
               health_summary, error_message, created_at, updated_at
        FROM tasks
        WHERE status = 'queued'
        ORDER BY created_at ASC
        LIMIT ?
        "#,
    )
    .bind(limit.max(0))
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_task).collect()
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

pub async fn get_settings(
    pool: &SqlitePool,
    default_save_dir: String,
) -> Result<AppSettings, String> {
    let max_active_tasks = get_setting_value(pool, SETTING_MAX_ACTIVE_TASKS)
        .await?
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_MAX_ACTIVE_TASKS)
        .clamp(MIN_MAX_ACTIVE_TASKS, MAX_MAX_ACTIVE_TASKS);
    let default_save_dir = get_setting_value(pool, SETTING_DEFAULT_SAVE_DIR)
        .await?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_save_dir);
    let global_speed_limit_bps = get_setting_value(pool, SETTING_GLOBAL_SPEED_LIMIT_BPS)
        .await?
        .and_then(|value| normalize_speed_limit_bps(&value));
    let multi_connection_threshold_bytes =
        get_setting_value(pool, SETTING_MULTI_CONNECTION_THRESHOLD_BYTES)
            .await?
            .and_then(|value| normalize_multi_connection_threshold_bytes(&value))
            .unwrap_or_else(|| DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES.to_string());
    let segment_count = get_setting_value(pool, SETTING_SEGMENT_COUNT)
        .await?
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_SEGMENT_COUNT)
        .clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT);
    let max_connections_per_host = get_setting_value(pool, SETTING_MAX_CONNECTIONS_PER_HOST)
        .await?
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_MAX_CONNECTIONS_PER_HOST)
        .clamp(MIN_MAX_CONNECTIONS_PER_HOST, MAX_MAX_CONNECTIONS_PER_HOST);

    Ok(AppSettings {
        max_active_tasks,
        default_save_dir,
        global_speed_limit_bps,
        multi_connection_threshold_bytes,
        segment_count,
        max_connections_per_host,
    })
}

pub async fn upsert_settings(pool: &SqlitePool, settings: &AppSettings) -> Result<(), String> {
    upsert_setting_value(
        pool,
        SETTING_MAX_ACTIVE_TASKS,
        &settings.max_active_tasks.to_string(),
    )
    .await?;
    upsert_setting_value(pool, SETTING_DEFAULT_SAVE_DIR, &settings.default_save_dir).await?;
    upsert_setting_value(
        pool,
        SETTING_GLOBAL_SPEED_LIMIT_BPS,
        settings.global_speed_limit_bps.as_deref().unwrap_or(""),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_MULTI_CONNECTION_THRESHOLD_BYTES,
        &settings.multi_connection_threshold_bytes,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SEGMENT_COUNT,
        &settings.segment_count.to_string(),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_MAX_CONNECTIONS_PER_HOST,
        &settings.max_connections_per_host.to_string(),
    )
    .await
}

pub fn normalize_speed_limit_bps(value: &str) -> Option<String> {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|limit| *limit > 0)
        .map(|limit| limit.to_string())
}

pub fn parse_speed_limit_bps(value: Option<&str>) -> Option<i64> {
    value
        .and_then(normalize_speed_limit_bps)
        .and_then(|value| value.parse::<i64>().ok())
}

pub fn normalize_multi_connection_threshold_bytes(value: &str) -> Option<String> {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .map(|limit| {
            limit.clamp(
                MIN_MULTI_CONNECTION_THRESHOLD_BYTES,
                MAX_MULTI_CONNECTION_THRESHOLD_BYTES,
            )
        })
        .map(|limit| limit.to_string())
}

pub fn parse_multi_connection_threshold_bytes(value: &str) -> i64 {
    normalize_multi_connection_threshold_bytes(value)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES)
}

async fn get_setting_value(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(row.map(|row| row.get("value")))
}

async fn upsert_setting_value(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO settings (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn browser_message_exists(pool: &SqlitePool, request_id: &str) -> Result<bool, String> {
    let row = sqlx::query("SELECT 1 FROM browser_messages WHERE request_id = ?")
        .bind(request_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(row.is_some())
}

pub async fn insert_browser_message(
    pool: &SqlitePool,
    request_id: &str,
    browser: BrowserKind,
    url: &str,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        INSERT INTO browser_messages (
            request_id, browser, url, status, error_message, created_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(request_id)
    .bind(browser.as_str())
    .bind(url)
    .bind(status)
    .bind(error_message)
    .bind(&created_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_browser_message_status(
    pool: &SqlitePool,
    request_id: &str,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE browser_messages
        SET status = ?, error_message = ?
        WHERE request_id = ?
        "#,
    )
    .bind(status)
    .bind(error_message)
    .bind(request_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn latest_browser_error(
    pool: &SqlitePool,
    browser: BrowserKind,
) -> Result<Option<String>, String> {
    let row = sqlx::query(
        r#"
        SELECT error_message FROM browser_messages
        WHERE browser = ? AND error_message IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(browser.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row.map(|row| row.get("error_message")))
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
    ensure_task_segments_with_plan(
        pool,
        task,
        DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        DEFAULT_SEGMENT_COUNT,
    )
    .await
}

pub async fn ensure_task_segments_with_settings(
    pool: &SqlitePool,
    task: &TaskRecord,
    settings: &AppSettings,
) -> Result<Vec<TaskSegmentRecord>, String> {
    ensure_task_segments_with_plan(
        pool,
        task,
        parse_multi_connection_threshold_bytes(&settings.multi_connection_threshold_bytes),
        settings
            .segment_count
            .min(settings.max_connections_per_host)
            .clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT),
    )
    .await
}

async fn ensure_task_segments_with_plan(
    pool: &SqlitePool,
    task: &TaskRecord,
    multi_connection_threshold_bytes: i64,
    segment_count: i32,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let existing = list_segment_records(pool, &task.id).await?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let segments =
        planned_segments_for_task_with_plan(task, multi_connection_threshold_bytes, segment_count);
    for segment in &segments {
        insert_segment_record(pool, segment).await?;
    }
    Ok(segments)
}

pub fn planned_segment_count(task: &TaskRecord) -> usize {
    planned_segment_count_with_plan(
        task,
        DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        DEFAULT_SEGMENT_COUNT,
    )
}

pub fn planned_segment_count_with_plan(
    task: &TaskRecord,
    multi_connection_threshold_bytes: i64,
    segment_count: i32,
) -> usize {
    let segment_count = segment_count.clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT) as usize;
    if task.supports_range
        && task.total_size > 0
        && task.total_size >= multi_connection_threshold_bytes.max(0)
    {
        segment_count
    } else {
        1
    }
}

pub fn planned_segments_for_task(task: &TaskRecord) -> Vec<TaskSegmentRecord> {
    planned_segments_for_task_with_plan(
        task,
        DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        DEFAULT_SEGMENT_COUNT,
    )
}

pub fn planned_segments_for_task_with_plan(
    task: &TaskRecord,
    multi_connection_threshold_bytes: i64,
    segment_count: i32,
) -> Vec<TaskSegmentRecord> {
    let count =
        planned_segment_count_with_plan(task, multi_connection_threshold_bytes, segment_count);
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

pub async fn update_segment_downloaded_until(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE segments
        SET downloaded_until = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_retry(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
    retry_count: i32,
    last_error: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE segments
        SET downloaded_until = ?, retry_count = ?, status = 'pending', last_error = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(retry_count)
    .bind(last_error)
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

pub async fn update_task_final_path(
    pool: &SqlitePool,
    task_id: &str,
    file_name: &str,
    final_path: &str,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET file_name = ?, final_path = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(file_name)
    .bind(final_path)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_task_save_target(
    pool: &SqlitePool,
    task_id: &str,
    file_name: &str,
    save_dir: &str,
    final_path: &str,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET file_name = ?, save_dir = ?, final_path = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(file_name)
    .bind(save_dir)
    .bind(final_path)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_task_remote_metadata(
    pool: &SqlitePool,
    task_id: &str,
    final_url: &str,
    total_size: i64,
    etag: Option<&str>,
    last_modified: Option<&str>,
    content_type: Option<&str>,
    supports_range: bool,
    source_host: &str,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET final_url = ?, total_size = ?, etag = ?, last_modified = ?,
            content_type = ?, supports_range = ?, source_host = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(final_url)
    .bind(total_size)
    .bind(etag)
    .bind(last_modified)
    .bind(content_type)
    .bind(if supports_range { 1 } else { 0 })
    .bind(source_host)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn reset_task_download_state(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = 0, speed_bps = 0, connection_count = 0,
            status = 'queued', health_summary = 'Queued', error_message = NULL,
            updated_at = ?
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

pub async fn delete_segments_for_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM segments WHERE task_id = ?")
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

pub async fn complete_unknown_size_task(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
    downloaded_bytes: i64,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    let final_size = downloaded_bytes.max(0);

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'completed', total_size = ?, downloaded_bytes = ?, speed_bps = 0,
            connection_count = 0, health_summary = 'Completed',
            error_message = NULL, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(final_size)
    .bind(final_size)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE segments
        SET range_start = 0, range_end = ?, downloaded_until = ?, status = 'completed',
            last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(final_size.saturating_sub(1).max(0))
    .bind(final_size)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct SegmentSplit {
    pub original_segment_id: String,
    pub original_range_end: i64,
    pub tail_segment: TaskSegmentRecord,
}

pub async fn split_largest_remaining_segment(
    pool: &SqlitePool,
    task_id: &str,
    min_remaining_bytes: i64,
    max_segments: usize,
) -> Result<Option<SegmentSplit>, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM segments WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= max_segments {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }

    let row = sqlx::query(
        r#"
        SELECT id, task_id, range_start, range_end, downloaded_until,
               status, retry_count, last_error
        FROM segments
        WHERE task_id = ? AND downloaded_until <= range_end
        ORDER BY (range_end - downloaded_until) DESC
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let Some(row) = row else {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    };
    let segment = row_to_segment(&row)?;
    let remaining = segment
        .range_end
        .saturating_sub(segment.downloaded_until)
        .saturating_add(1);
    if remaining < min_remaining_bytes {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }

    let first_remaining_len = remaining / 2;
    if first_remaining_len <= 0 {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let original_range_end = segment.downloaded_until + first_remaining_len - 1;
    let tail_start = original_range_end + 1;
    if tail_start > segment.range_end {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }

    sqlx::query("UPDATE segments SET range_end = ? WHERE id = ?")
        .bind(original_range_end)
        .bind(&segment.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let tail_segment = TaskSegmentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: segment.task_id.clone(),
        range_start: tail_start,
        range_end: segment.range_end,
        downloaded_until: tail_start,
        status: SegmentStatus::Pending,
        retry_count: 0,
        last_error: None,
    };

    sqlx::query(
        r#"
        INSERT INTO segments (
            id, task_id, range_start, range_end, downloaded_until,
            status, retry_count, last_error
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&tail_segment.id)
    .bind(&tail_segment.task_id)
    .bind(tail_segment.range_start)
    .bind(tail_segment.range_end)
    .bind(tail_segment.downloaded_until)
    .bind(tail_segment.status.as_str())
    .bind(tail_segment.retry_count)
    .bind(&tail_segment.last_error)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(Some(SegmentSplit {
        original_segment_id: segment.id,
        original_range_end,
        tail_segment,
    }))
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
