use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::models::{
    AppErrorPayload, HashVerificationStatus, RecoveryAction, TaskKind, TaskPriority, TaskRecord,
    TaskStatus,
};

use super::MAX_TASK_PAGE_SIZE;

#[derive(Debug, Clone)]
pub struct TaskListQuery {
    pub nav: String,
    pub search: String,
    pub sort_key: String,
    pub sort_direction: String,
    pub file_type: String,
    pub source: String,
    pub failure: String,
    pub resume: String,
    pub page: i64,
    pub page_size: i64,
    pub cursor_value: Option<String>,
    pub cursor_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskListPage {
    pub items: Vec<TaskRecord>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct TaskFilterOptions {
    pub sources: Vec<String>,
    pub failure_categories: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaskTransferOptionsUpdate {
    pub task_speed_limit_bps: Option<String>,
    pub priority: TaskPriority,
    pub queue_position: i64,
    pub category_key: Option<String>,
}

pub async fn list_task_records(pool: &SqlitePool) -> Result<Vec<TaskRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
               health_summary, error_message, error_code, recovery_actions, retry_after_at,
               expected_hash_sha256, actual_hash_sha256,
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

pub async fn list_task_records_page(
    pool: &SqlitePool,
    input: &TaskListQuery,
) -> Result<TaskListPage, String> {
    let page_size = input.page_size.clamp(1, MAX_TASK_PAGE_SIZE);
    let page = input.page.max(0);
    let offset = page.saturating_mul(page_size);

    let mut count_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM tasks");
    append_task_filters(&mut count_query, input);
    let total: i64 = count_query
        .build_query_scalar()
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
               health_summary, error_message, error_code, recovery_actions, retry_after_at,
               expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
        FROM tasks
        "#,
    );
    append_task_filters(&mut query, input);
    query.push(" ORDER BY ");
    query.push(task_sort_sql(&input.sort_key));
    query.push(if input.sort_direction == "asc" {
        " ASC"
    } else {
        " DESC"
    });
    query.push(", file_name ASC LIMIT ");
    query.push_bind(page_size);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let rows = query
        .build()
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(TaskListPage {
        items: rows
            .iter()
            .map(row_to_task)
            .collect::<Result<Vec<_>, _>>()?,
        total,
        page,
        page_size,
        has_more: offset + page_size < total,
    })
}

pub async fn list_task_records_cursor(
    pool: &SqlitePool,
    input: &TaskListQuery,
) -> Result<TaskListPage, String> {
    let page_size = input.page_size.clamp(1, MAX_TASK_PAGE_SIZE);
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
               health_summary, error_message, error_code, recovery_actions, retry_after_at,
               expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
        FROM tasks
        "#,
    );
    let mut has_where = append_task_filters(&mut query, input);
    append_cursor_filter(&mut query, &mut has_where, input);
    query.push(" ORDER BY ");
    query.push(task_sort_sql(&input.sort_key));
    query.push(if input.sort_direction == "asc" {
        " ASC"
    } else {
        " DESC"
    });
    query.push(", id ASC LIMIT ");
    query.push_bind(page_size + 1);

    let rows = query
        .build()
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut items = rows
        .iter()
        .map(row_to_task)
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = i64::try_from(items.len()).unwrap_or(0) > page_size;
    if has_more {
        items.truncate(usize::try_from(page_size).unwrap_or(100));
    }
    let total = i64::try_from(items.len()).unwrap_or(0) + if has_more { 1 } else { 0 };

    Ok(TaskListPage {
        items,
        total,
        page: 0,
        page_size,
        has_more,
    })
}

fn append_task_filters(query: &mut QueryBuilder<'_, Sqlite>, input: &TaskListQuery) -> bool {
    let mut has_where = false;

    match input.nav.as_str() {
        "downloading" => push_static_filter(
            query,
            &mut has_where,
            "status IN ('downloading', 'retrying')",
        ),
        "paused" => push_static_filter(query, &mut has_where, "status = 'paused'"),
        "completed" => push_static_filter(query, &mut has_where, "status = 'completed'"),
        "failed" => push_static_filter(
            query,
            &mut has_where,
            "status IN ('failed', 'needs_attention')",
        ),
        "settings" => push_static_filter(query, &mut has_where, "0 = 1"),
        _ => {}
    }

    let search = input.search.trim();
    if !search.is_empty() {
        let pattern = format!("%{}%", search.to_ascii_lowercase());
        push_filter_prefix(query, &mut has_where);
        query
            .push("(LOWER(file_name) LIKE ")
            .push_bind(pattern.clone())
            .push(" OR LOWER(source_key) LIKE ")
            .push_bind(pattern.clone())
            .push(" OR LOWER(url) LIKE ")
            .push_bind(pattern)
            .push(")");
    }

    if input.source != "all" {
        push_filter_prefix(query, &mut has_where);
        query.push("source_key = ").push_bind(input.source.clone());
    }

    if input.failure != "all" {
        if let Some(filter) = failure_category_filter_sql(&input.failure) {
            push_static_filter(query, &mut has_where, filter);
        }
    }

    match input.resume.as_str() {
        "resumable" => push_static_filter(query, &mut has_where, "supports_resume = 1"),
        "single_connection" => push_static_filter(query, &mut has_where, "supports_resume = 0"),
        _ => {}
    }

    if let Some(filter) = file_type_filter_sql(&input.file_type) {
        push_static_filter(query, &mut has_where, filter);
    }
    has_where
}

fn append_cursor_filter(
    query: &mut QueryBuilder<'_, Sqlite>,
    has_where: &mut bool,
    input: &TaskListQuery,
) {
    let (Some(value), Some(id)) = (&input.cursor_value, &input.cursor_id) else {
        return;
    };
    let expr = task_sort_sql(&input.sort_key);
    push_filter_prefix(query, has_where);
    query.push("(").push(expr);
    if input.sort_direction == "asc" {
        query.push(" > ");
    } else {
        query.push(" < ");
    }
    if is_numeric_sort(&input.sort_key) {
        query.push_bind(value.parse::<f64>().unwrap_or(0.0));
    } else {
        query.push_bind(value.clone());
    }
    query.push(" OR (").push(expr).push(" = ");
    if is_numeric_sort(&input.sort_key) {
        query.push_bind(value.parse::<f64>().unwrap_or(0.0));
    } else {
        query.push_bind(value.clone());
    }
    query.push(" AND id > ").push_bind(id.clone()).push("))");
}

fn push_filter_prefix(query: &mut QueryBuilder<'_, Sqlite>, has_where: &mut bool) {
    if *has_where {
        query.push(" AND ");
    } else {
        query.push(" WHERE ");
        *has_where = true;
    }
}

fn push_static_filter(query: &mut QueryBuilder<'_, Sqlite>, has_where: &mut bool, filter: &str) {
    push_filter_prefix(query, has_where);
    query.push(filter);
}

fn task_sort_sql(sort_key: &str) -> &'static str {
    match sort_key {
        "created_at" => "created_at",
        "file_size" => "total_size",
        "progress" => "CASE WHEN total_size > 0 THEN CAST(downloaded_bytes AS REAL) / total_size ELSE 0 END",
        "speed" => "speed_bps",
        "priority" => "CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 WHEN 'low' THEN 2 ELSE 1 END",
        "queue_position" => "queue_position",
        "status" => {
            "CASE status WHEN 'downloading' THEN 0 WHEN 'retrying' THEN 1 WHEN 'queued' THEN 2 WHEN 'paused' THEN 3 WHEN 'waiting_network' THEN 4 WHEN 'needs_attention' THEN 5 WHEN 'failed' THEN 6 WHEN 'completed' THEN 7 ELSE 8 END"
        }
        _ => "updated_at",
    }
}

fn is_numeric_sort(sort_key: &str) -> bool {
    matches!(
        sort_key,
        "file_size" | "progress" | "speed" | "status" | "priority" | "queue_position"
    )
}

fn file_type_filter_sql(file_type: &str) -> Option<&'static str> {
    match file_type {
        "archive" => Some(
            "(LOWER(COALESCE(content_type, '')) LIKE '%zip%' OR LOWER(file_name) LIKE '%.zip' OR LOWER(file_name) LIKE '%.rar' OR LOWER(file_name) LIKE '%.7z' OR LOWER(file_name) LIKE '%.tar' OR LOWER(file_name) LIKE '%.gz' OR LOWER(file_name) LIKE '%.bz2' OR LOWER(file_name) LIKE '%.xz')",
        ),
        "image" => Some(
            "(LOWER(COALESCE(content_type, '')) LIKE 'image/%' OR LOWER(file_name) LIKE '%.png' OR LOWER(file_name) LIKE '%.jpg' OR LOWER(file_name) LIKE '%.jpeg' OR LOWER(file_name) LIKE '%.gif' OR LOWER(file_name) LIKE '%.webp' OR LOWER(file_name) LIKE '%.avif' OR LOWER(file_name) LIKE '%.svg')",
        ),
        "video" => Some(
            "(LOWER(COALESCE(content_type, '')) LIKE 'video/%' OR LOWER(file_name) LIKE '%.mp4' OR LOWER(file_name) LIKE '%.mkv' OR LOWER(file_name) LIKE '%.mov' OR LOWER(file_name) LIKE '%.webm' OR LOWER(file_name) LIKE '%.avi')",
        ),
        "document" => Some(
            "(LOWER(file_name) LIKE '%.pdf' OR LOWER(file_name) LIKE '%.doc' OR LOWER(file_name) LIKE '%.docx' OR LOWER(file_name) LIKE '%.xls' OR LOWER(file_name) LIKE '%.xlsx' OR LOWER(file_name) LIKE '%.ppt' OR LOWER(file_name) LIKE '%.pptx' OR LOWER(file_name) LIKE '%.txt' OR LOWER(file_name) LIKE '%.md')",
        ),
        "app" => Some(
            "(LOWER(file_name) LIKE '%.exe' OR LOWER(file_name) LIKE '%.msi' OR LOWER(file_name) LIKE '%.dmg' OR LOWER(file_name) LIKE '%.pkg' OR LOWER(file_name) LIKE '%.deb' OR LOWER(file_name) LIKE '%.rpm' OR LOWER(file_name) LIKE '%.appimage')",
        ),
        "other" => Some(
            "NOT ((LOWER(COALESCE(content_type, '')) LIKE '%zip%' OR LOWER(file_name) LIKE '%.zip' OR LOWER(file_name) LIKE '%.rar' OR LOWER(file_name) LIKE '%.7z' OR LOWER(file_name) LIKE '%.tar' OR LOWER(file_name) LIKE '%.gz' OR LOWER(file_name) LIKE '%.bz2' OR LOWER(file_name) LIKE '%.xz') OR (LOWER(COALESCE(content_type, '')) LIKE 'image/%' OR LOWER(file_name) LIKE '%.png' OR LOWER(file_name) LIKE '%.jpg' OR LOWER(file_name) LIKE '%.jpeg' OR LOWER(file_name) LIKE '%.gif' OR LOWER(file_name) LIKE '%.webp' OR LOWER(file_name) LIKE '%.avif' OR LOWER(file_name) LIKE '%.svg') OR (LOWER(COALESCE(content_type, '')) LIKE 'video/%' OR LOWER(file_name) LIKE '%.mp4' OR LOWER(file_name) LIKE '%.mkv' OR LOWER(file_name) LIKE '%.mov' OR LOWER(file_name) LIKE '%.webm' OR LOWER(file_name) LIKE '%.avi') OR (LOWER(file_name) LIKE '%.pdf' OR LOWER(file_name) LIKE '%.doc' OR LOWER(file_name) LIKE '%.docx' OR LOWER(file_name) LIKE '%.xls' OR LOWER(file_name) LIKE '%.xlsx' OR LOWER(file_name) LIKE '%.ppt' OR LOWER(file_name) LIKE '%.pptx' OR LOWER(file_name) LIKE '%.txt' OR LOWER(file_name) LIKE '%.md') OR (LOWER(file_name) LIKE '%.exe' OR LOWER(file_name) LIKE '%.msi' OR LOWER(file_name) LIKE '%.dmg' OR LOWER(file_name) LIKE '%.pkg' OR LOWER(file_name) LIKE '%.deb' OR LOWER(file_name) LIKE '%.rpm' OR LOWER(file_name) LIKE '%.appimage'))",
        ),
        _ => None,
    }
}

fn failure_category_filter_sql(category: &str) -> Option<&'static str> {
    match category {
        "remote_changed" => Some("error_code = 'remote_changed'"),
        "resume_unavailable" => Some("error_code = 'resume_unavailable'"),
        "temp_file" => Some("error_code IN ('temp_file_missing', 'temp_file_smaller_than_progress')"),
        "disk_write" => Some("error_code = 'disk_write_failed'"),
        "http" => Some("(error_code LIKE 'http_%' OR error_code = 'server_rate_limited')"),
        "auth" => Some("error_code IN ('auth_headers_expired', 'auth_headers_unavailable')"),
        "bt" => Some("error_code LIKE 'bt_%'"),
        "ftp" => Some("error_code LIKE 'ftp_%'"),
        "proxy" => Some("error_code LIKE 'proxy_%'"),
        "schedule" => Some("error_code LIKE 'schedule_%'"),
        "hls" => Some("error_code LIKE 'hls_%'"),
        "dash" => Some("error_code LIKE 'dash_%'"),
        "webdav" => Some("error_code LIKE 'webdav_%'"),
        "other" => Some("(error_code IS NOT NULL AND error_code NOT IN ('remote_changed', 'resume_unavailable', 'temp_file_missing', 'temp_file_smaller_than_progress', 'disk_write_failed', 'auth_headers_expired', 'auth_headers_unavailable', 'server_rate_limited') AND error_code NOT LIKE 'http_%' AND error_code NOT LIKE 'hls_%' AND error_code NOT LIKE 'dash_%' AND error_code NOT LIKE 'bt_%' AND error_code NOT LIKE 'ftp_%' AND error_code NOT LIKE 'webdav_%' AND error_code NOT LIKE 'proxy_%' AND error_code NOT LIKE 'schedule_%')"),
        _ => None,
    }
}

pub async fn get_task_record(pool: &SqlitePool, id: &str) -> Result<Option<TaskRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
               health_summary, error_message, error_code, recovery_actions, retry_after_at,
               expected_hash_sha256, actual_hash_sha256,
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

pub async fn find_duplicate_task_record(
    pool: &SqlitePool,
    url: &str,
    final_url: Option<&str>,
    bt_source_key: Option<&str>,
) -> Result<Option<TaskRecord>, String> {
    let final_url = final_url.unwrap_or("");
    let bt_source_key = bt_source_key.unwrap_or("");
    let row = sqlx::query(
        r#"
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
               health_summary, error_message, error_code, recovery_actions, retry_after_at,
               expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
        FROM tasks
        WHERE url IN (?, ?)
           OR final_url IN (?, ?)
           OR (? != '' AND source_key = ?)
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
        LIMIT 1
        "#,
    )
    .bind(url)
    .bind(final_url)
    .bind(url)
    .bind(final_url)
    .bind(bt_source_key)
    .bind(bt_source_key)
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
        SELECT id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, status, etag, last_modified, content_type,
               supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
               task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
               health_summary, error_message, error_code, recovery_actions, retry_after_at,
               expected_hash_sha256, actual_hash_sha256,
               hash_status, hash_error, hash_verified_at, created_at, updated_at
        FROM tasks
        WHERE status = 'queued' AND (retry_after_at IS NULL OR retry_after_at <= ?)
        ORDER BY queue_position ASC, created_at ASC
        LIMIT ?
        "#,
    )
    .bind(crate::models::task::now_iso())
    .bind(limit.max(0))
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_task).collect()
}

pub async fn next_retry_after_at(pool: &SqlitePool) -> Result<Option<String>, String> {
    let row = sqlx::query(
        r#"
        SELECT retry_after_at FROM tasks
        WHERE status = 'queued' AND retry_after_at IS NOT NULL
        ORDER BY retry_after_at ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|row| row.get("retry_after_at")))
}

pub async fn next_queue_position(pool: &SqlitePool) -> Result<i64, String> {
    let current_max: Option<i64> = sqlx::query_scalar("SELECT MAX(queue_position) FROM tasks")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(current_max.unwrap_or(0).saturating_add(1000))
}

pub async fn update_task_transfer_options(
    pool: &SqlitePool,
    task_id: &str,
    update: TaskTransferOptionsUpdate,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE tasks
        SET task_speed_limit_bps = ?,
            priority = ?,
            queue_position = ?,
            category_key = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&update.task_speed_limit_bps)
    .bind(update.priority.as_str())
    .bind(update.queue_position)
    .bind(&update.category_key)
    .bind(crate::models::task::now_iso())
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
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
        task_speed_limit_bps: row.get("task_speed_limit_bps"),
        priority: TaskPriority::from_db_str(row.get::<String, _>("priority").as_str()),
        queue_position: row.get("queue_position"),
        category_key: row.get("category_key"),
        obey_schedule: row.get::<i64, _>("obey_schedule") != 0,
        health_summary: row.get("health_summary"),
        error_message: row.get("error_message"),
        error_code: task_error_code_from_row(row),
        recovery_actions: task_recovery_actions_from_row(row),
        retry_after_at: row.get("retry_after_at"),
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

fn task_error_code_from_row(row: &sqlx::sqlite::SqliteRow) -> Option<String> {
    row.try_get::<Option<String>, _>("error_code")
        .ok()
        .flatten()
        .or_else(|| {
            row.try_get::<Option<String>, _>("error_message")
                .ok()
                .flatten()
                .and_then(|error| serde_json::from_str::<AppErrorPayload>(&error).ok())
                .map(|payload| payload.code)
        })
}

fn task_recovery_actions_from_row(row: &sqlx::sqlite::SqliteRow) -> Vec<RecoveryAction> {
    if let Some(raw) = row
        .try_get::<Option<String>, _>("recovery_actions")
        .ok()
        .flatten()
    {
        if let Ok(values) = serde_json::from_str::<Vec<String>>(&raw) {
            let actions = values
                .iter()
                .filter_map(|value| value.parse().ok())
                .collect::<Vec<_>>();
            if !actions.is_empty() {
                return actions;
            }
        }
    }

    row.try_get::<Option<String>, _>("error_message")
        .ok()
        .flatten()
        .and_then(|error| serde_json::from_str::<AppErrorPayload>(&error).ok())
        .map(|payload| {
            payload
                .actions
                .iter()
                .filter_map(|value| value.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn recovery_actions_json(actions: &[RecoveryAction]) -> Result<String, String> {
    let values = actions
        .iter()
        .map(|action| action.as_str())
        .collect::<Vec<_>>();
    serde_json::to_string(&values).map_err(|e| e.to_string())
}

pub(super) fn error_state_from_message(
    error_message: Option<&str>,
) -> (Option<String>, Vec<RecoveryAction>) {
    let Some(error) = error_message else {
        return (None, Vec::new());
    };
    if let Ok(payload) = serde_json::from_str::<AppErrorPayload>(error) {
        let actions = payload
            .actions
            .iter()
            .filter_map(|value| value.parse().ok())
            .collect();
        return (Some(payload.code), actions);
    }
    (legacy_error_code(error), Vec::new())
}

fn legacy_error_code(error: &str) -> Option<String> {
    if error.contains("Remote file changed") {
        return Some("remote_changed".to_string());
    }
    if error.contains("Server no longer supports resume") || error.contains("Resume unavailable") {
        return Some("resume_unavailable".to_string());
    }
    if error.contains("Temporary file is missing") {
        return Some("temp_file_missing".to_string());
    }
    if error.contains("Temporary file is smaller") {
        return Some("temp_file_smaller_than_progress".to_string());
    }
    if error.contains("disk")
        || error.contains("Disk")
        || error.contains("write")
        || error.contains("Write")
    {
        return Some("disk_write_failed".to_string());
    }
    if error.contains("403") {
        return Some("http_denied".to_string());
    }
    if error.contains("404") {
        return Some("http_not_found".to_string());
    }
    if error.contains("429") {
        return Some("server_rate_limited".to_string());
    }
    None
}

pub async fn insert_task_record(pool: &SqlitePool, task: &TaskRecord) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, url, final_url, protocol, task_kind, file_name, save_dir, temp_path, final_path,
            total_size, downloaded_bytes, status, etag, last_modified, content_type,
            supports_resume, supports_parallel, supports_multi_file, source_key, connection_count, speed_bps,
            task_speed_limit_bps, priority, queue_position, category_key, obey_schedule,
            health_summary, error_message, error_code, recovery_actions, retry_after_at,
            expected_hash_sha256, actual_hash_sha256,
            hash_status, hash_error, hash_verified_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(&task.task_speed_limit_bps)
    .bind(task.priority.as_str())
    .bind(task.queue_position)
    .bind(&task.category_key)
    .bind(if task.obey_schedule { 1 } else { 0 })
    .bind(&task.health_summary)
    .bind(&task.error_message)
    .bind(&task.error_code)
    .bind(recovery_actions_json(&task.recovery_actions)?)
    .bind(&task.retry_after_at)
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

pub async fn task_filter_options(pool: &SqlitePool) -> Result<TaskFilterOptions, String> {
    let source_rows = sqlx::query(
        r#"
        SELECT DISTINCT source_key FROM tasks
        WHERE source_key IS NOT NULL AND TRIM(source_key) != ''
        ORDER BY source_key ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let sources = source_rows
        .iter()
        .map(|row| row.get::<String, _>("source_key"))
        .collect();

    let error_rows = sqlx::query(
        r#"
        SELECT DISTINCT error_code FROM tasks
        WHERE error_code IS NOT NULL AND TRIM(error_code) != ''
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut failure_categories = error_rows
        .iter()
        .filter_map(|row| {
            crate::models::task::failure_category_for_code(Some(
                row.get::<String, _>("error_code").as_str(),
            ))
        })
        .map(|category| match category {
            crate::models::TaskFailureCategory::RemoteChanged => "remote_changed",
            crate::models::TaskFailureCategory::ResumeUnavailable => "resume_unavailable",
            crate::models::TaskFailureCategory::TempFile => "temp_file",
            crate::models::TaskFailureCategory::DiskWrite => "disk_write",
            crate::models::TaskFailureCategory::Http => "http",
            crate::models::TaskFailureCategory::Auth => "auth",
            crate::models::TaskFailureCategory::Hls => "hls",
            crate::models::TaskFailureCategory::Dash => "dash",
            crate::models::TaskFailureCategory::Metalink => "metalink",
            crate::models::TaskFailureCategory::Bt => "bt",
            crate::models::TaskFailureCategory::Ftp => "ftp",
            crate::models::TaskFailureCategory::Webdav => "webdav",
            crate::models::TaskFailureCategory::Proxy => "proxy",
            crate::models::TaskFailureCategory::Schedule => "schedule",
            crate::models::TaskFailureCategory::Other => "other",
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    failure_categories.sort();
    failure_categories.dedup();

    Ok(TaskFilterOptions {
        sources,
        failure_categories,
    })
}
