use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

use crate::models::{
    AppSettings, BrowserKind, HashVerificationStatus, RequestDiagnostic, RequestDiagnosticRecord,
    SegmentStatus, SegmentSummary, TaskEvent, TaskFileRecord, TaskKind, TaskRecord,
    TaskSegmentRecord, TaskStatus,
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
const SETTING_SYSTEM_NOTIFICATIONS: &str = "system_notifications";
const SETTING_CLOSE_TO_TRAY: &str = "close_to_tray";
const SETTING_START_ON_BOOT: &str = "start_on_boot";

pub async fn connect(db_path: &std::path::Path) -> Result<SqlitePool, String> {
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                sqlx::query("PRAGMA foreign_keys = ON")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
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
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               health_summary, error_message, expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
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
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               health_summary, error_message, expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
        FROM tasks WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    row.as_ref().map(row_to_task).transpose()
}

pub async fn insert_task_event(
    pool: &SqlitePool,
    task_id: &str,
    event_type: &str,
    payload: Option<&str>,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO task_events (task_id, event_type, payload, created_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(task_id)
    .bind(event_type)
    .bind(payload)
    .bind(&created_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn list_task_events(
    pool: &SqlitePool,
    task_id: &str,
    limit: i64,
) -> Result<Vec<TaskEvent>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, event_type, payload, created_at
        FROM task_events
        WHERE task_id = ?
        ORDER BY id DESC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|row| TaskEvent {
            id: row.get::<i64, _>("id").to_string(),
            task_id: row.get("task_id"),
            event_type: row.get("event_type"),
            payload: row.get("payload"),
            created_at: row.get("created_at"),
        })
        .collect())
}

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

pub async fn list_request_diagnostics(
    pool: &SqlitePool,
    task_id: &str,
    limit: i64,
) -> Result<Vec<RequestDiagnostic>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, method, url, range_header, status_code, etag, last_modified,
               content_length, error_message, retry_count, duration_ms, created_at
        FROM task_requests
        WHERE task_id = ?
        ORDER BY id DESC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(limit.clamp(1, 500))
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

pub async fn list_queued_task_records(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<TaskRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               health_summary, error_message, expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
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
        protocol: row.get("protocol"),
        task_kind: TaskKind::from_db_str(row.get::<String, _>("task_kind").as_str()),
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
        supports_resume: row.get::<i64, _>("supports_resume") != 0,
        supports_parallel: row.get::<i64, _>("supports_parallel") != 0,
        supports_multi_file: row.get::<i64, _>("supports_multi_file") != 0,
        source_key: row.get("source_key"),
        connection_count: row.get("connection_count"),
        speed_bps: row.get("speed_bps"),
        health_summary: row.get("health_summary"),
        error_message: row.get("error_message"),
        expected_hash_sha256: row.get("expected_hash_sha256"),
        actual_hash_sha256: row.get("actual_hash_sha256"),
        hash_status: HashVerificationStatus::from_db_str(
            row.get::<String, _>("hash_status").as_str(),
        ),
        hash_error: row.get("hash_error"),
        hash_verified_at: row.get("hash_verified_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn insert_task_record(pool: &SqlitePool, task: &TaskRecord) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
            total_size, downloaded_bytes, status, etag, last_modified, content_type,
            supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
            health_summary, error_message, expected_hash_sha256, actual_hash_sha256,
            hash_status, hash_error, hash_verified_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task.id)
    .bind(&task.url)
    .bind(&task.final_url)
    .bind(&task.protocol)
    .bind(task.task_kind.as_str())
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
    .bind(if task.supports_resume { 1 } else { 0 })
    .bind(if task.supports_parallel { 1 } else { 0 })
    .bind(if task.supports_multi_file { 1 } else { 0 })
    .bind(&task.source_key)
    .bind(task.connection_count)
    .bind(task.speed_bps)
    .bind(&task.health_summary)
    .bind(&task.error_message)
    .bind(&task.expected_hash_sha256)
    .bind(&task.actual_hash_sha256)
    .bind(task.hash_status.as_str())
    .bind(&task.hash_error)
    .bind(&task.hash_verified_at)
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
    let system_notifications = get_bool_setting(pool, SETTING_SYSTEM_NOTIFICATIONS, true).await?;
    let close_to_tray = get_bool_setting(pool, SETTING_CLOSE_TO_TRAY, false).await?;
    let start_on_boot = get_bool_setting(pool, SETTING_START_ON_BOOT, false).await?;

    Ok(AppSettings {
        max_active_tasks,
        default_save_dir,
        global_speed_limit_bps,
        multi_connection_threshold_bytes,
        segment_count,
        max_connections_per_host,
        system_notifications,
        close_to_tray,
        start_on_boot,
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
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SYSTEM_NOTIFICATIONS,
        bool_setting_value(settings.system_notifications),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_CLOSE_TO_TRAY,
        bool_setting_value(settings.close_to_tray),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_START_ON_BOOT,
        bool_setting_value(settings.start_on_boot),
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

async fn get_bool_setting(pool: &SqlitePool, key: &str, default: bool) -> Result<bool, String> {
    Ok(get_setting_value(pool, key)
        .await?
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default))
}

fn bool_setting_value(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
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

pub async fn insert_task_file_record(
    pool: &SqlitePool,
    file: &TaskFileRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO task_files (
            id, task_id, relative_path, file_name, save_dir, temp_path, final_path,
            total_size, downloaded_bytes, selected, status, content_type
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&file.id)
    .bind(&file.task_id)
    .bind(&file.relative_path)
    .bind(&file.file_name)
    .bind(&file.save_dir)
    .bind(&file.temp_path)
    .bind(&file.final_path)
    .bind(file.total_size)
    .bind(file.downloaded_bytes)
    .bind(if file.selected { 1 } else { 0 })
    .bind(file.status.as_str())
    .bind(&file.content_type)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn list_task_file_records(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<TaskFileRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, relative_path, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, selected, status, content_type
        FROM task_files
        WHERE task_id = ?
        ORDER BY relative_path ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_task_file).collect()
}

fn row_to_task_file(row: &sqlx::sqlite::SqliteRow) -> Result<TaskFileRecord, String> {
    Ok(TaskFileRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        relative_path: row.get("relative_path"),
        file_name: row.get("file_name"),
        save_dir: row.get("save_dir"),
        temp_path: row.get("temp_path"),
        final_path: row.get("final_path"),
        total_size: row.get("total_size"),
        downloaded_bytes: row.get("downloaded_bytes"),
        selected: row.get::<i64, _>("selected") != 0,
        status: TaskStatus::from_db_str(row.get::<String, _>("status").as_str()),
        content_type: row.get("content_type"),
    })
}

pub async fn insert_segment_record(
    pool: &SqlitePool,
    segment: &TaskSegmentRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO task_work_units (
            id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
            speed_bps, status, retry_count, last_error
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&segment.id)
    .bind(&segment.task_id)
    .bind(&segment.file_id)
    .bind(&segment.unit_kind)
    .bind(segment.range_start)
    .bind(segment.range_end)
    .bind(segment.downloaded_until)
    .bind(segment.speed_bps)
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

    let task_work_units =
        planned_segments_for_task_with_plan(task, multi_connection_threshold_bytes, segment_count);
    for segment in &task_work_units {
        insert_segment_record(pool, segment).await?;
    }
    Ok(task_work_units)
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
    if task.supports_parallel
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
            file_id: None,
            unit_kind: "http_range".to_string(),
            range_start: 0,
            range_end: total_size.saturating_sub(1).max(0),
            downloaded_until: task.downloaded_bytes.max(0),
            speed_bps: 0,
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
                file_id: None,
                unit_kind: "http_range".to_string(),
                range_start: start,
                range_end: end,
                downloaded_until,
                status: if completed {
                    SegmentStatus::Completed
                } else {
                    SegmentStatus::Pending
                },
                speed_bps: 0,
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
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
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

pub async fn list_segment_records_paged(
    pool: &SqlitePool,
    task_id: &str,
    page: i64,
    page_size: i64,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let limit = page_size.clamp(1, 500);
    let offset = page.max(0).saturating_mul(limit);
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
        WHERE task_id = ?
        ORDER BY range_start ASC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(task_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_segment).collect()
}

pub async fn segment_summary(pool: &SqlitePool, task_id: &str) -> Result<SegmentSummary, String> {
    let rows = list_segment_records(pool, task_id).await?;
    let total = i32::try_from(rows.len()).unwrap_or(i32::MAX);
    let active = i32::try_from(
        rows.iter()
            .filter(|segment| segment.status == SegmentStatus::Downloading)
            .count(),
    )
    .unwrap_or(i32::MAX);
    let completed = i32::try_from(
        rows.iter()
            .filter(|segment| segment.status == SegmentStatus::Completed)
            .count(),
    )
    .unwrap_or(i32::MAX);
    let failed = i32::try_from(
        rows.iter()
            .filter(|segment| segment.status == SegmentStatus::Failed)
            .count(),
    )
    .unwrap_or(i32::MAX);
    Ok(SegmentSummary {
        total,
        active,
        completed,
        failed,
        downloaded_bytes: total_segment_downloaded_bytes(&rows).to_string(),
        speed_bps: rows
            .iter()
            .map(|segment| segment.speed_bps)
            .sum::<i64>()
            .to_string(),
    })
}

pub async fn get_first_segment_record(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<TaskSegmentRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
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
        file_id: row.get("file_id"),
        unit_kind: row.get("unit_kind"),
        range_start: row.get("range_start"),
        range_end: row.get("range_end"),
        downloaded_until: row.get("downloaded_until"),
        speed_bps: row.get("speed_bps"),
        status: SegmentStatus::from_db_str(row.get::<String, _>("status").as_str()),
        retry_count: row.get("retry_count"),
        last_error: row.get("last_error"),
    })
}

pub async fn clear_tasks(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("DELETE FROM task_requests")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_events")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_work_units")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_files")
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

    sqlx::query(
        r#"
        UPDATE task_files
        SET downloaded_bytes = ?, status = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(downloaded_bytes)
    .bind(status.as_str())
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
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = ?, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes)
    .bind(speed_bps)
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
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = 0, status = ?, last_error = NULL
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

pub async fn update_segment_runtime_progress(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
    speed_bps: i64,
    status: SegmentStatus,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = ?, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(speed_bps.max(0))
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
        UPDATE task_work_units
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
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = 0, retry_count = ?, status = 'pending', last_error = ?
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

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = ?
        WHERE task_id = ?
        "#,
    )
    .bind(status.as_str())
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

    sqlx::query(
        r#"
        UPDATE task_files
        SET file_name = ?, relative_path = ?, final_path = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(file_name)
    .bind(file_name)
    .bind(final_path)
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

    let temp_path = format!("{final_path}.vibe-downloading");
    sqlx::query(
        r#"
        UPDATE task_files
        SET file_name = ?, relative_path = ?, save_dir = ?, final_path = ?, temp_path = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(file_name)
    .bind(file_name)
    .bind(save_dir)
    .bind(final_path)
    .bind(temp_path)
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
    supports_resume: bool,
    supports_parallel: bool,
    supports_multi_file: bool,
    source_key: &str,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET final_url = ?, total_size = ?, etag = ?, last_modified = ?,
            content_type = ?, supports_resume = ?, supports_parallel = ?,
            supports_multi_file = ?, source_key = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(final_url)
    .bind(total_size)
    .bind(etag)
    .bind(last_modified)
    .bind(content_type)
    .bind(if supports_resume { 1 } else { 0 })
    .bind(if supports_parallel { 1 } else { 0 })
    .bind(if supports_multi_file { 1 } else { 0 })
    .bind(source_key)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_hash_verification(
    pool: &SqlitePool,
    task_id: &str,
    actual_hash_sha256: Option<&str>,
    status: HashVerificationStatus,
    error_message: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    let verified_at = if matches!(
        status,
        HashVerificationStatus::Verified | HashVerificationStatus::Failed
    ) {
        Some(updated_at.as_str())
    } else {
        None
    };

    sqlx::query(
        r#"
        UPDATE tasks
        SET actual_hash_sha256 = ?, hash_status = ?, hash_error = ?,
            hash_verified_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(actual_hash_sha256)
    .bind(status.as_str())
    .bind(error_message)
    .bind(verified_at)
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

    sqlx::query(
        r#"
        UPDATE task_files
        SET downloaded_bytes = 0, status = 'queued'
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn delete_segments_for_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_work_units WHERE task_id = ?")
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

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = 'completed', downloaded_bytes = total_size
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    insert_task_event(pool, task_id, "completed", None).await?;

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
        UPDATE task_work_units
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
        UPDATE task_work_units
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
        UPDATE task_files
        SET status = 'completed', total_size = ?, downloaded_bytes = ?
        WHERE task_id = ?
        "#,
    )
    .bind(final_size)
    .bind(final_size)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_work_units
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

    insert_task_event(pool, task_id, "completed", None).await?;

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
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_work_units WHERE task_id = ?")
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
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
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

    sqlx::query("UPDATE task_work_units SET range_end = ? WHERE id = ?")
        .bind(original_range_end)
        .bind(&segment.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let tail_segment = TaskSegmentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: segment.task_id.clone(),
        file_id: segment.file_id.clone(),
        unit_kind: segment.unit_kind.clone(),
        range_start: tail_start,
        range_end: segment.range_end,
        downloaded_until: tail_start,
        speed_bps: 0,
        status: SegmentStatus::Pending,
        retry_count: 0,
        last_error: None,
    };

    sqlx::query(
        r#"
        INSERT INTO task_work_units (
            id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
            speed_bps, status, retry_count, last_error
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&tail_segment.id)
    .bind(&tail_segment.task_id)
    .bind(&tail_segment.file_id)
    .bind(&tail_segment.unit_kind)
    .bind(tail_segment.range_start)
    .bind(tail_segment.range_end)
    .bind(tail_segment.downloaded_until)
    .bind(tail_segment.speed_bps)
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
        UPDATE task_work_units
        SET status = ?, downloaded_until = COALESCE(?, downloaded_until),
            speed_bps = CASE WHEN ? = 'downloading' THEN speed_bps ELSE 0 END,
            last_error = ?
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(downloaded_until)
    .bind(status.as_str())
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
        UPDATE task_work_units
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

pub fn total_segment_downloaded_bytes(task_work_units: &[TaskSegmentRecord]) -> i64 {
    task_work_units.iter().map(segment_downloaded_bytes).sum()
}

pub async fn delete_task_record(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_requests WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_work_units WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_files WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_events WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

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
        UPDATE task_work_units
        SET status = 'pending', speed_bps = 0
        WHERE status = 'downloading'
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
