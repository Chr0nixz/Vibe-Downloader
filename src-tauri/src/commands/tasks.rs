use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use tauri::{AppHandle, State};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::{
    db,
    download::{DownloadContext, EngineRegistry, ProbeOutput, ProbeRequest},
    events::{emit_queue_changed, emit_task_progress, emit_task_updated, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        task::now_iso, AppErrorPayload, BatchImportItem, BatchImportResult, BrowserKind,
        HashVerificationState, HashVerificationStatus, ProbeTaskPayload, RecoveryAction,
        RequestDiagnostic, SegmentSummary, Task, TaskEvent, TaskFileRecord, TaskRecord,
        TaskSegment, TaskSegmentRecord, TaskStatus,
    },
    platform, AppState, DownloadControl, TaskRequestHeaders,
};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub url: String,
    pub save_dir: Option<String>,
    pub file_name: Option<String>,
    pub expected_hash_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTaskInput {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTaskAttentionInput {
    pub id: String,
    pub action: RecoveryAction,
    pub file_name: Option<String>,
    pub save_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListSegmentsInput {
    pub task_id: String,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CursorPageInput {
    pub task_id: String,
    pub cursor: Option<String>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventsPageResult {
    pub items: Vec<TaskEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequestsPageResult {
    pub items: Vec<RequestDiagnostic>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskSegmentsPageResult {
    pub items: Vec<TaskSegment>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportUrlsInput {
    pub input: String,
    pub save_dir: Option<String>,
    pub probe: Option<bool>,
    pub create: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksInput {
    pub nav: Option<String>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_direction: Option<String>,
    pub file_type: Option<String>,
    pub source: Option<String>,
    pub failure: Option<String>,
    pub resume: Option<String>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResult {
    pub items: Vec<Task>,
    pub total: String,
    pub page: i32,
    pub page_size: i32,
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    let records = db::list_task_records(&state.pool).await?;
    let mut tasks = Vec::with_capacity(records.len());
    for record in records {
        tasks.push(task_from_record_with_files(&state.pool, record).await?);
    }
    Ok(tasks)
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks_page(
    state: State<'_, AppState>,
    input: ListTasksInput,
) -> Result<ListTasksResult, String> {
    let query = db::TaskListQuery {
        nav: input.nav.unwrap_or_else(|| "all".to_string()),
        search: input.search.unwrap_or_default(),
        sort_key: input.sort_key.unwrap_or_else(|| "updated_at".to_string()),
        sort_direction: match input.sort_direction.as_deref() {
            Some("asc") => "asc".to_string(),
            _ => "desc".to_string(),
        },
        file_type: input.file_type.unwrap_or_else(|| "all".to_string()),
        source: input.source.unwrap_or_else(|| "all".to_string()),
        failure: input.failure.unwrap_or_else(|| "all".to_string()),
        resume: input.resume.unwrap_or_else(|| "all".to_string()),
        page: i64::from(input.page.unwrap_or(0)),
        page_size: i64::from(
            input
                .page_size
                .unwrap_or(i32::try_from(db::DEFAULT_TASK_PAGE_SIZE).unwrap_or(100)),
        ),
        cursor_value: None,
        cursor_id: None,
    };
    let page = db::list_task_records_page(&state.pool, &query).await?;
    let mut tasks = Vec::with_capacity(page.items.len());
    for record in page.items {
        tasks.push(task_from_record_with_files(&state.pool, record).await?);
    }
    Ok(ListTasksResult {
        items: tasks,
        total: page.total.to_string(),
        page: i32::try_from(page.page).unwrap_or(0),
        page_size: i32::try_from(page.page_size).unwrap_or(100),
    })
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksCursorInput {
    pub nav: Option<String>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_direction: Option<String>,
    pub file_type: Option<String>,
    pub source: Option<String>,
    pub failure_category: Option<String>,
    pub resume: Option<String>,
    pub cursor: Option<String>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskFilterOptions {
    pub sources: Vec<String>,
    pub failure_categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksCursorResult {
    pub items: Vec<Task>,
    pub next_cursor: Option<String>,
    pub total_estimate: String,
    pub filter_options: TaskFilterOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskCursor {
    sort_value: String,
    id: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_tasks_cursor(
    state: State<'_, AppState>,
    input: ListTasksCursorInput,
) -> Result<ListTasksCursorResult, String> {
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| serde_json::from_str::<TaskCursor>(value).ok());
    let sort_key = input.sort_key.unwrap_or_else(|| "updated_at".to_string());
    let query = db::TaskListQuery {
        nav: input.nav.unwrap_or_else(|| "all".to_string()),
        search: input.search.unwrap_or_default(),
        sort_key: sort_key.clone(),
        sort_direction: match input.sort_direction.as_deref() {
            Some("asc") => "asc".to_string(),
            _ => "desc".to_string(),
        },
        file_type: input.file_type.unwrap_or_else(|| "all".to_string()),
        source: input.source.unwrap_or_else(|| "all".to_string()),
        failure: input.failure_category.unwrap_or_else(|| "all".to_string()),
        resume: input.resume.unwrap_or_else(|| "all".to_string()),
        page: 0,
        page_size: i64::from(
            input
                .page_size
                .unwrap_or(i32::try_from(db::DEFAULT_TASK_PAGE_SIZE).unwrap_or(100)),
        ),
        cursor_value: cursor.as_ref().map(|cursor| cursor.sort_value.clone()),
        cursor_id: cursor.as_ref().map(|cursor| cursor.id.clone()),
    };
    let page = db::list_task_records_cursor(&state.pool, &query).await?;
    let next_cursor = page
        .has_more
        .then(|| {
            let record = page.items.last()?;
            serde_json::to_string(&TaskCursor {
                sort_value: task_cursor_value(record, &sort_key),
                id: record.id.clone(),
            })
            .unwrap_or_default()
            .into()
        })
        .flatten();
    let mut tasks = Vec::with_capacity(page.items.len());
    for record in page.items {
        tasks.push(task_from_record_with_files(&state.pool, record).await?);
    }
    let options = db::task_filter_options(&state.pool).await?;
    Ok(ListTasksCursorResult {
        items: tasks,
        next_cursor,
        total_estimate: page.total.to_string(),
        filter_options: TaskFilterOptions {
            sources: options.sources,
            failure_categories: options.failure_categories,
        },
    })
}

fn task_cursor_value(task: &TaskRecord, sort_key: &str) -> String {
    match sort_key {
        "created_at" => task.created_at.clone(),
        "file_size" => task.total_size.to_string(),
        "progress" => {
            if task.total_size > 0 {
                ((task.downloaded_bytes as f64) / (task.total_size as f64)).to_string()
            } else {
                "0".to_string()
            }
        }
        "speed" => task.speed_bps.to_string(),
        "status" => status_rank(task.status).to_string(),
        _ => task.updated_at.clone(),
    }
}

fn status_rank(status: TaskStatus) -> i32 {
    match status {
        TaskStatus::Downloading => 0,
        TaskStatus::Retrying => 1,
        TaskStatus::Queued => 2,
        TaskStatus::Paused => 3,
        TaskStatus::WaitingNetwork => 4,
        TaskStatus::NeedsAttention => 5,
        TaskStatus::Failed => 6,
        TaskStatus::Completed => 7,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_task(state: State<'_, AppState>, id: String) -> Result<Option<Task>, String> {
    let Some(record) = db::get_task_record(&state.pool, &id).await? else {
        return Ok(None);
    };
    Ok(Some(
        task_from_record_with_files(&state.pool, record).await?,
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_segments(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<TaskSegment>, String> {
    db::list_segment_records(&state.pool, &task_id)
        .await
        .map(|records| records.into_iter().map(TaskSegment::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn list_segments(
    state: State<'_, AppState>,
    input: ListSegmentsInput,
) -> Result<Vec<TaskSegment>, String> {
    db::list_segment_records_paged(
        &state.pool,
        &input.task_id,
        i64::from(input.page.unwrap_or(0)),
        i64::from(input.page_size.unwrap_or(100)),
    )
    .await
    .map(|records| records.into_iter().map(TaskSegment::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn list_segments_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<TaskSegmentsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    let mut records =
        db::list_segment_records_cursor(&state.pool, &input.task_id, cursor, i64::from(page_size))
            .await?;
    let next_cursor = if records.len() > usize::try_from(page_size).unwrap_or(100) {
        records.pop().map(|segment| segment.range_start.to_string())
    } else {
        None
    };
    Ok(TaskSegmentsPageResult {
        items: records.into_iter().map(TaskSegment::from).collect(),
        next_cursor,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_segment_summary(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<SegmentSummary, String> {
    db::segment_summary(&state.pool, &task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_events(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<TaskEvent>, String> {
    db::list_task_events(&state.pool, &task_id, 100).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_events_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<TaskEventsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    let mut items =
        db::list_task_events_page(&state.pool, &input.task_id, cursor, i64::from(page_size))
            .await?;
    let next_cursor = if items.len() > usize::try_from(page_size).unwrap_or(100) {
        items.pop().map(|event| event.id)
    } else {
        None
    };
    Ok(TaskEventsPageResult { items, next_cursor })
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_requests(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<RequestDiagnostic>, String> {
    db::list_request_diagnostics(&state.pool, &task_id, 100).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_requests_page(
    state: State<'_, AppState>,
    input: CursorPageInput,
) -> Result<TaskRequestsPageResult, String> {
    let page_size = input.page_size.unwrap_or(100).clamp(1, 500);
    let cursor = input
        .cursor
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    let mut items = db::list_request_diagnostics_page(
        &state.pool,
        &input.task_id,
        cursor,
        i64::from(page_size),
    )
    .await?;
    let next_cursor = if items.len() > usize::try_from(page_size).unwrap_or(100) {
        items.pop().map(|request| request.id)
    } else {
        None
    };
    Ok(TaskRequestsPageResult { items, next_cursor })
}

#[tauri::command]
#[specta::specta]
pub async fn probe_task(
    state: State<'_, AppState>,
    input: ProbeTaskInput,
) -> Result<ProbeTaskPayload, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a download URL.".to_string());
    }

    tracing::debug!(url = %sanitize_url(url), "probing download url");
    let engine = state.engine_registry.engine_for_uri(url)?;
    let probe = engine
        .probe(ProbeRequest {
            uri: url.to_string(),
            source: None,
            request_headers: Vec::new(),
        })
        .await?;
    tracing::debug!(
        url = %sanitize_url(url),
        total_size = probe.total_size,
        supports_parallel = probe.capabilities.supports_parallel,
        source_key = %probe.source_key,
        "probe completed"
    );
    Ok(ProbeTaskPayload {
        final_url: probe.resolved_uri,
        file_name: probe.display_name,
        protocol: probe.protocol,
        task_kind: probe.task_kind,
        capabilities: probe.capabilities,
        files: probe.files,
        total_size: probe.total_size.to_string(),
        source_key: probe.source_key,
        content_type: probe.content_type,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn create_task(
    app: AppHandle,
    state: State<'_, AppState>,
    input: CreateTaskInput,
) -> Result<Task, String> {
    create_task_with_state(app, state.inner(), input).await
}

#[tauri::command]
#[specta::specta]
pub async fn import_urls(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ImportUrlsInput,
) -> Result<BatchImportResult, String> {
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    let mut created_count = 0_i32;
    let mut failed_count = 0_i32;
    let mut duplicate_count = 0_i32;
    let should_probe = input.probe.unwrap_or(true);
    let should_create = input.create.unwrap_or(false);

    for raw_url in input
        .input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let parsed = match Url::parse(raw_url) {
            Ok(url)
                if matches!(url.scheme(), "http" | "https" | "ftp" | "ftps" | "magnet")
                    || is_torrent_url(&url) =>
            {
                url
            }
            Ok(_) => {
                failed_count += 1;
                items.push(BatchImportItem {
                    input_url: raw_url.to_string(),
                    normalized_url: None,
                    duplicate: false,
                    valid: false,
                    file_name: None,
                    total_size: None,
                    content_type: None,
                    supports_resume: false,
                    error_message: Some(
                        "Only HTTP, HTTPS, FTP, FTPS, magnet links, and .torrent URLs are supported."
                            .to_string(),
                    ),
                    task: None,
                });
                continue;
            }
            Err(_) => {
                failed_count += 1;
                items.push(BatchImportItem {
                    input_url: raw_url.to_string(),
                    normalized_url: None,
                    duplicate: false,
                    valid: false,
                    file_name: None,
                    total_size: None,
                    content_type: None,
                    supports_resume: false,
                    error_message: Some("URL is invalid.".to_string()),
                    task: None,
                });
                continue;
            }
        };
        let normalized_url = parsed.to_string();
        if !seen.insert(normalized_url.clone()) {
            duplicate_count += 1;
            items.push(BatchImportItem {
                input_url: raw_url.to_string(),
                normalized_url: Some(normalized_url),
                duplicate: true,
                valid: true,
                file_name: None,
                total_size: None,
                content_type: None,
                supports_resume: false,
                error_message: Some("Duplicate URL in this import.".to_string()),
                task: None,
            });
            continue;
        }

        let mut item = BatchImportItem {
            input_url: raw_url.to_string(),
            normalized_url: Some(normalized_url.clone()),
            duplicate: false,
            valid: true,
            file_name: None,
            total_size: None,
            content_type: None,
            supports_resume: false,
            error_message: None,
            task: None,
        };

        if should_probe {
            let probe_result = match state.engine_registry.engine_for_uri(&normalized_url) {
                Ok(engine) => {
                    engine
                        .probe(ProbeRequest {
                            uri: normalized_url.clone(),
                            source: None,
                            request_headers: Vec::new(),
                        })
                        .await
                }
                Err(error) => Err(error),
            };
            match probe_result {
                Ok(probe) => {
                    item.file_name = Some(probe.display_name);
                    item.total_size = Some(probe.total_size.to_string());
                    item.content_type = probe.content_type;
                    item.supports_resume = probe.capabilities.supports_resume;
                }
                Err(error) => {
                    item.valid = false;
                    item.error_message = Some(error);
                    failed_count += 1;
                    items.push(item);
                    continue;
                }
            }
        }

        if should_create {
            match create_task_with_state(
                app.clone(),
                state.inner(),
                CreateTaskInput {
                    url: normalized_url,
                    save_dir: input.save_dir.clone(),
                    file_name: item.file_name.clone(),
                    expected_hash_sha256: None,
                },
            )
            .await
            {
                Ok(task) => {
                    created_count += 1;
                    item.task = Some(task);
                }
                Err(error) => {
                    failed_count += 1;
                    item.valid = false;
                    item.error_message = Some(error);
                }
            }
        }

        items.push(item);
    }

    Ok(BatchImportResult {
        items,
        created_count,
        failed_count,
        duplicate_count,
    })
}

pub(crate) async fn create_task_with_state(
    app: AppHandle,
    state: &AppState,
    input: CreateTaskInput,
) -> Result<Task, String> {
    create_task_with_state_and_headers(app, state, input, Vec::new(), None).await
}

pub(crate) async fn create_task_with_state_and_headers(
    app: AppHandle,
    state: &AppState,
    input: CreateTaskInput,
    request_headers: Vec<(String, String)>,
    source_browser: Option<BrowserKind>,
) -> Result<Task, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a download URL.".to_string());
    }

    let engine = state.engine_registry.engine_for_uri(url)?;
    let probe = engine
        .probe(ProbeRequest {
            uri: url.to_string(),
            source: None,
            request_headers: request_headers.clone(),
        })
        .await?;
    let settings =
        db::get_settings(&state.pool, super::settings::default_download_dir(&app)?).await?;
    let save_dir = match input
        .save_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(&settings.default_save_dir),
    };
    std::fs::create_dir_all(&save_dir)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;

    let probe_files = normalized_probe_files(&probe);
    let uses_single_output_file = probe_files.len() == 1;
    let requested_file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| uses_single_output_file)
        .unwrap_or(&probe.display_name);
    let final_path = unique_final_path(&save_dir, requested_file_name);
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested_file_name)
        .to_string();
    let temp_path = PathBuf::from(format!("{}.vibe-downloading", final_path.display()));
    let now = now_iso();
    let expected_hash_sha256 = normalize_sha256(input.expected_hash_sha256.as_deref())?;
    let hash_status = if expected_hash_sha256.is_some() {
        HashVerificationStatus::Pending
    } else {
        HashVerificationStatus::NotRequested
    };

    let record = TaskRecord {
        id: Uuid::new_v4().to_string(),
        url: url.to_string(),
        final_url: Some(probe.resolved_uri.clone()),
        protocol: probe.protocol.clone(),
        task_kind: probe.task_kind,
        file_name: file_name.clone(),
        save_dir: save_dir.to_string_lossy().to_string(),
        temp_path: Some(temp_path.to_string_lossy().to_string()),
        final_path: Some(final_path.to_string_lossy().to_string()),
        total_size: probe.total_size,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
        content_type: probe.content_type.clone(),
        supports_resume: probe.capabilities.supports_resume,
        supports_parallel: probe.capabilities.supports_parallel,
        supports_multi_file: probe.capabilities.supports_multi_file,
        source_key: probe.source_key.clone(),
        connection_count: 0,
        speed_bps: 0,
        health_summary: Some("Queued".to_string()),
        error_message: None,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256,
        actual_hash_sha256: None,
        hash_status,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
    };

    db::insert_task_record(&state.pool, &record).await?;
    db::insert_task_event(&state.pool, &record.id, "created", None).await?;
    for file_record in task_file_records_from_probe(
        &record,
        &probe_files,
        &save_dir,
        &final_path,
        &temp_path,
        &file_name,
    )? {
        db::insert_task_file_record(&state.pool, &file_record).await?;
    }
    db::ensure_task_segments_with_settings(&state.pool, &record, &settings).await?;
    if is_bt_protocol(&record.protocol) {
        if let Some(info_hash) = record.source_key.strip_prefix("bt:") {
            db::upsert_torrent_task(
                &state.pool,
                &record.id,
                info_hash,
                &record.file_name,
                record
                    .url
                    .starts_with("magnet:")
                    .then_some(record.url.as_str()),
                None,
                0,
                0,
                false,
                None,
            )
            .await?;
        }
    }
    if !request_headers.is_empty() {
        state
            .request_headers
            .lock()
            .await
            .insert(record.id.clone(), request_headers.clone());
        if let Err(error) = db::upsert_task_request_headers(
            &state.pool,
            &record.id,
            &request_headers,
            source_browser,
        )
        .await
        {
            tracing::warn!(
                task_id = %record.id,
                error = %error,
                "browser request headers kept in memory only"
            );
            db::insert_task_event(
                &state.pool,
                &record.id,
                "headers_not_persisted",
                Some(&error),
            )
            .await?;
        }
    }
    tracing::info!(
        task_id = %record.id,
        url = %sanitize_url(url),
        file_name = %record.file_name,
        total_size = record.total_size,
        "task created"
    );
    emit_queue_changed(&app);
    schedule_queued_tasks(app.clone(), state).await;

    let task = task_payload(&state.pool, &record.id).await?;
    emit_task_updated(&app, &task);
    Ok(task)
}

#[tauri::command]
#[specta::specta]
pub async fn pause_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    tracing::info!(task_id = %id, "pausing task");
    if let Some(control) = state.downloads.lock().await.get(&id) {
        control.cancel.store(true, Ordering::SeqCst);
    }
    db::update_task_status(
        &state.pool,
        &id,
        TaskStatus::Paused,
        0,
        0,
        Some("Paused"),
        None,
    )
    .await?;
    db::insert_task_event(&state.pool, &id, "paused", None).await?;
    db::update_segments_status_for_task(
        &state.pool,
        &id,
        crate::models::SegmentStatus::Pending,
        None,
    )
    .await?;
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_task_updated_record(&app, &state.pool, &task).await;
    emit_queue_changed(&app);
    schedule_queued_tasks(app, state.inner()).await;
    task_payload(&state.pool, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    tracing::info!(task_id = %id, "resuming task");
    let task = require_task(&state.pool, &id).await?;
    if matches!(task.status, TaskStatus::Completed) {
        return Err("This download is already completed.".to_string());
    }
    if matches!(task.status, TaskStatus::NeedsAttention) {
        return Err("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    db::insert_task_event(&state.pool, &id, "resumed", None).await?;
    let task = queue_task_for_retry(&app, state.inner(), &id).await?;
    task_from_record_with_files(&state.pool, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    tracing::info!(task_id = %id, "retrying task");
    let task = require_task(&state.pool, &id).await?;
    if task.status == TaskStatus::NeedsAttention
        && task_error_code(&task)
            .as_deref()
            .is_some_and(restart_required_error_code)
    {
        return Err("This task must be restarted before it can continue safely.".to_string());
    }
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }

    db::insert_task_event(&state.pool, &id, "retrying", None).await?;
    let task = queue_task_for_retry(&app, state.inner(), &id).await?;
    task_from_record_with_files(&state.pool, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    tracing::info!(task_id = %id, "canceling task");
    if let Some(control) = state.downloads.lock().await.get(&id) {
        control.cancel.store(true, Ordering::SeqCst);
    }
    db::update_task_status(
        &state.pool,
        &id,
        TaskStatus::Failed,
        0,
        0,
        Some("Canceled"),
        Some("Canceled by user."),
    )
    .await?;
    db::insert_task_event(&state.pool, &id, "failed", Some("Canceled by user.")).await?;
    db::update_task_retry_after(&state.pool, &id, None).await?;
    db::update_segments_status_for_task(
        &state.pool,
        &id,
        crate::models::SegmentStatus::Failed,
        Some("Canceled by user."),
    )
    .await?;
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_task_updated_record(&app, &state.pool, &task).await;
    emit_queue_changed(&app);
    schedule_queued_tasks(app, state.inner()).await;
    task_payload(&state.pool, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    delete_file: bool,
) -> Result<(), String> {
    tracing::info!(task_id = %id, delete_file, "deleting task");
    let task_for_runtime = db::get_task_record(&state.pool, &id).await?;
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel.store(true, Ordering::SeqCst);
        if task_for_runtime
            .as_ref()
            .is_none_or(|task| !is_bt_protocol(&task.protocol))
        {
            control.handle.abort();
        }
    }
    if let Some(task) = task_for_runtime.as_ref() {
        state
            .engine_registry
            .delete_runtime_task(task, delete_file)
            .await;
    }

    if delete_file {
        if let Some(task) = task_for_runtime {
            for file in db::list_task_file_records(&state.pool, &id).await? {
                for path in [file.temp_path, file.final_path].into_iter().flatten() {
                    remove_task_file_path(&path)?;
                }
            }
            for path in [task.temp_path, task.final_path].into_iter().flatten() {
                remove_task_file_path(&path)?;
            }
        }
    }

    db::delete_task_record(&state.pool, &id).await?;
    emit_queue_changed(&app);
    schedule_queued_tasks(app, state.inner()).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_task_attention(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ResolveTaskAttentionInput,
) -> Result<Task, String> {
    let id = input.id.trim();
    if id.is_empty() {
        return Err("Task id is required.".to_string());
    }
    let task = require_task(&state.pool, id).await?;
    let error_code = task_error_code(&task);

    match input.action {
        RecoveryAction::Retry => {
            if task.status == TaskStatus::NeedsAttention
                && error_code
                    .as_deref()
                    .is_some_and(restart_required_error_code)
            {
                return Err(
                    "This task must be restarted before it can continue safely.".to_string()
                );
            }
            db::insert_task_event(&state.pool, id, "retrying", None).await?;
            let task = queue_task_for_retry(&app, state.inner(), id).await?;
            task_from_record_with_files(&state.pool, task).await
        }
        RecoveryAction::RetryLater => {
            if task.status == TaskStatus::NeedsAttention
                && error_code
                    .as_deref()
                    .is_some_and(restart_required_error_code)
            {
                return Err(
                    "This task must be restarted before it can continue safely.".to_string()
                );
            }
            let retry_after_at = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
            db::insert_task_event(
                &state.pool,
                id,
                "retry_later",
                Some(&format!("Retry scheduled for {retry_after_at}.")),
            )
            .await?;
            let task =
                queue_task_for_retry_at(&app, state.inner(), id, Some(&retry_after_at)).await?;
            spawn_schedule_queued_tasks_after(
                app.clone(),
                state.inner(),
                std::time::Duration::from_secs(300),
            );
            task_from_record_with_files(&state.pool, task).await
        }
        RecoveryAction::ChooseAnotherName | RecoveryAction::ChooseAnotherFolder => {
            update_recovery_target(&app, state.inner(), &task, &input).await?;
            db::insert_task_event(
                &state.pool,
                id,
                "retrying",
                Some("Recovery target changed."),
            )
            .await?;
            let task = queue_task_for_retry(&app, state.inner(), id).await?;
            task_from_record_with_files(&state.pool, task).await
        }
        RecoveryAction::Restart => {
            db::insert_task_event(
                &state.pool,
                id,
                "retrying",
                Some("Restarted from beginning."),
            )
            .await?;
            let task = restart_task_from_beginning(&app, state.inner(), &task).await?;
            task_from_record_with_files(&state.pool, task).await
        }
        RecoveryAction::OpenFolder | RecoveryAction::CheckUrl | RecoveryAction::FreeDiskSpace => {
            task_from_record_with_files(&state.pool, task).await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn open_task_file(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let task = require_task(&state.pool, &id).await?;
    let final_path = task
        .final_path
        .ok_or_else(|| "This task does not have a file path yet.".to_string())?;
    let path = PathBuf::from(final_path);
    if !path.exists() {
        return Err("The downloaded file was not found on disk.".to_string());
    }
    platform::open_path(&path)
}

#[tauri::command]
#[specta::specta]
pub async fn open_task_folder(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let task = require_task(&state.pool, &id).await?;
    let path = task
        .final_path
        .as_deref()
        .and_then(|value| Path::new(value).parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(task.save_dir));
    if !path.exists() {
        return Err("The download folder was not found on disk.".to_string());
    }
    platform::open_path(&path)
}

#[tauri::command]
#[specta::specta]
pub async fn verify_task_hash(
    state: State<'_, AppState>,
    id: String,
) -> Result<HashVerificationState, String> {
    verify_task_hash_with_pool(&state.pool, &id).await
}

pub(crate) async fn verify_task_hash_with_pool(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<HashVerificationState, String> {
    let task = require_task(pool, id).await?;
    let Some(expected) = task.expected_hash_sha256.clone() else {
        return Ok(HashVerificationState {
            task_id: task.id,
            expected_sha256: None,
            actual_sha256: task.actual_hash_sha256,
            status: HashVerificationStatus::NotRequested,
            error_message: None,
            verified_at: task.hash_verified_at,
        });
    };
    let Some(final_path) = task.final_path.clone() else {
        let message = "Downloaded file path is not available.".to_string();
        db::update_hash_verification(
            pool,
            &task.id,
            None,
            HashVerificationStatus::Failed,
            Some(&message),
        )
        .await?;
        return Ok(HashVerificationState {
            task_id: task.id,
            expected_sha256: Some(expected),
            actual_sha256: None,
            status: HashVerificationStatus::Failed,
            error_message: Some(message),
            verified_at: Some(now_iso()),
        });
    };

    db::update_hash_verification(pool, &task.id, None, HashVerificationStatus::Pending, None)
        .await?;
    let actual = sha256_file(&PathBuf::from(final_path)).await?;
    let status = if actual.eq_ignore_ascii_case(&expected) {
        HashVerificationStatus::Verified
    } else {
        HashVerificationStatus::Failed
    };
    let error_message = if status == HashVerificationStatus::Failed {
        Some("SHA-256 checksum does not match.".to_string())
    } else {
        None
    };
    db::update_hash_verification(
        pool,
        &task.id,
        Some(&actual),
        status,
        error_message.as_deref(),
    )
    .await?;
    db::insert_task_event(
        pool,
        &task.id,
        if status == HashVerificationStatus::Verified {
            "hash_verified"
        } else {
            "hash_failed"
        },
        error_message.as_deref(),
    )
    .await?;
    let updated = require_task(pool, &task.id).await?;
    Ok(HashVerificationState {
        task_id: updated.id,
        expected_sha256: updated.expected_hash_sha256,
        actual_sha256: updated.actual_hash_sha256,
        status: updated.hash_status,
        error_message: updated.hash_error,
        verified_at: updated.hash_verified_at,
    })
}

#[cfg(debug_assertions)]
pub async fn seed_mock_data(pool: &sqlx::SqlitePool) -> Result<Vec<Task>, String> {
    db::clear_tasks(pool).await?;
    let now = now_iso();
    let mocks = build_mock_tasks(&now);

    for task in &mocks {
        db::insert_task_record(pool, task).await?;
        db::insert_task_file_record(
            pool,
            &TaskFileRecord {
                id: Uuid::new_v4().to_string(),
                task_id: task.id.clone(),
                relative_path: task.file_name.clone(),
                file_name: task.file_name.clone(),
                save_dir: task.save_dir.clone(),
                temp_path: task.temp_path.clone(),
                final_path: task.final_path.clone(),
                total_size: task.total_size,
                downloaded_bytes: task.downloaded_bytes,
                selected: true,
                status: task.status,
                content_type: task.content_type.clone(),
            },
        )
        .await?;
    }

    let records = db::list_task_records(pool).await?;
    let mut tasks = Vec::with_capacity(records.len());
    for record in records {
        tasks.push(task_from_record_with_files(pool, record).await?);
    }
    Ok(tasks)
}

#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn seed_mock_tasks(state: State<'_, AppState>) -> Result<Vec<Task>, String> {
    seed_mock_data(&state.pool).await
}

pub(crate) async fn schedule_queued_tasks(app: AppHandle, state: &AppState) {
    schedule_queued_tasks_inner(
        app,
        state.pool.clone(),
        state.downloads.clone(),
        state.request_headers.clone(),
        state.scheduler.clone(),
        state.speed_limiter.clone(),
        state.engine_registry.clone(),
    )
    .await;
}

pub(crate) async fn schedule_retry_after_wakeup(app: AppHandle, state: &AppState) {
    let Some(next) = (match db::next_retry_after_at(&state.pool).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(error = %error, "failed to inspect retry-after queue");
            None
        }
    }) else {
        return;
    };
    let when = chrono::DateTime::parse_from_rfc3339(&next)
        .map(|value| value.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let now = chrono::Utc::now();
    let delay = when
        .signed_duration_since(now)
        .to_std()
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    if delay.is_zero() {
        schedule_queued_tasks(app, state).await;
    } else {
        spawn_schedule_queued_tasks_after(app, state, delay);
    }
}

async fn schedule_queued_tasks_inner(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
) {
    let _guard = scheduler.lock().await;

    loop {
        let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
        let settings = match db::get_settings(&pool, default_dir).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::error!(error = %error, "failed to load settings for scheduler");
                break;
            }
        };
        let active_count = downloads.lock().await.len() as i32;
        let available = settings.max_active_tasks.saturating_sub(active_count);
        if available <= 0 {
            tracing::debug!(
                active_count,
                max_active_tasks = settings.max_active_tasks,
                "scheduler has no available slots"
            );
            break;
        }

        let queued =
            match db::list_queued_task_records(&pool, i64::from(settings.max_active_tasks)).await {
                Ok(tasks) => tasks,
                Err(error) => {
                    tracing::error!(error = %error, "failed to load queued tasks");
                    break;
                }
            };
        if queued.is_empty() {
            break;
        }

        tracing::debug!(
            available,
            queued_count = queued.len(),
            "scheduler dispatching queued tasks"
        );

        let mut made_progress = false;
        for task in queued {
            if downloads.lock().await.len() as i32 >= settings.max_active_tasks {
                break;
            }
            let host_used = host_connection_slots(&downloads, &task.source_key).await;
            let host_limit = usize::try_from(settings.max_connections_per_host)
                .unwrap_or(usize::try_from(db::DEFAULT_MAX_CONNECTIONS_PER_HOST).unwrap_or(8))
                .max(1);
            if host_used >= host_limit {
                tracing::debug!(
                    task_id = %task.id,
                    source_key = %task.source_key,
                    host_used,
                    host_limit,
                    "scheduler deferred task because host connection limit is full"
                );
                continue;
            }
            let planned_slots = db::planned_segment_count_with_plan(
                &task,
                db::parse_multi_connection_threshold_bytes(
                    &settings.multi_connection_threshold_bytes,
                ),
                settings.segment_count,
            )
            .min(host_limit.saturating_sub(host_used))
            .max(1);
            let task_id = task.id.clone();
            if let Err(error) = start_task_download(
                app.clone(),
                pool.clone(),
                downloads.clone(),
                request_headers.clone(),
                scheduler.clone(),
                speed_limiter.clone(),
                engine_registry.clone(),
                task,
                planned_slots,
            )
            .await
            {
                match db::get_task_record(&pool, &task_id).await {
                    Ok(Some(current)) if current.status == TaskStatus::Queued => {
                        mark_download_failed(&app, &pool, &task_id, error).await;
                    }
                    Ok(Some(current)) => {
                        emit_task_progress_snapshot(&app, &current);
                        emit_task_updated_record(&app, &pool, &current).await;
                    }
                    _ => {}
                }
            }
            made_progress = true;
        }

        if !made_progress {
            break;
        }
    }

    emit_queue_changed(&app);
}

#[allow(clippy::too_many_arguments)]
async fn start_task_download(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
    task: TaskRecord,
    connection_limit: usize,
) -> Result<(), String> {
    let task_request_headers =
        resolve_task_request_headers(&pool, request_headers.clone(), &task.id).await?;
    let task =
        prepare_task_for_download(&pool, &engine_registry, task, &task_request_headers).await?;
    if downloads.lock().await.contains_key(&task.id) {
        tracing::debug!(task_id = %task.id, "download already active, skipping start");
        return Ok(());
    }

    tracing::info!(
        task_id = %task.id,
        url = %sanitize_url(&task.url),
        total_size = task.total_size,
        connection_limit,
        "starting task download"
    );

    let connection_count = i32::try_from(connection_limit.max(1)).unwrap_or(1);
    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        connection_count,
        Some("Downloading"),
        None,
    )
    .await?;
    db::insert_task_event(&pool, &task.id, "started", None).await?;
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let downloads_map = downloads.clone();
    let task_id = task.id.clone();
    let map_task_id = task.id.clone();
    let source_key = task.source_key.clone();
    let task_app = app.clone();
    let task_cancel = cancel.clone();
    let task_pool = pool.clone();
    let task_scheduler = scheduler.clone();
    let state_speed_limiter = speed_limiter.clone();
    let task_engine_registry = engine_registry.clone();

    let handle = tokio::spawn(async move {
        let engine = match task_engine_registry.engine_for_uri(&task.url) {
            Ok(engine) => engine,
            Err(error) => {
                mark_download_failed(&task_app, &task_pool, &task_id, error).await;
                let _ = downloads_map.lock().await.remove(&task_id);
                spawn_schedule_queued_tasks(
                    task_app.clone(),
                    task_pool.clone(),
                    downloads_map.clone(),
                    request_headers.clone(),
                    task_scheduler.clone(),
                    state_speed_limiter.clone(),
                    task_engine_registry.clone(),
                );
                return;
            }
        };

        let result = engine
            .download(DownloadContext {
                app: task_app.clone(),
                pool: task_pool.clone(),
                task,
                cancel: task_cancel.clone(),
                speed_limiter: state_speed_limiter.clone(),
                connection_limit,
                request_headers: task_request_headers.clone(),
            })
            .await;
        let canceled = task_cancel.load(Ordering::SeqCst);
        let _ = downloads_map.lock().await.remove(&task_id);
        let _ = request_headers.lock().await.remove(&task_id);

        if let Err(error) = result {
            if !canceled {
                mark_download_failed(&task_app, &task_pool, &task_id, error).await;
            }
        } else if !canceled {
            match verify_task_hash_with_pool(&task_pool, &task_id).await {
                Ok(state) if state.status != HashVerificationStatus::NotRequested => {
                    tracing::info!(
                        task_id = %task_id,
                        status = ?state.status,
                        "hash verification completed"
                    );
                    if let Ok(Some(current)) = db::get_task_record(&task_pool, &task_id).await {
                        emit_task_updated_record(&task_app, &task_pool, &current).await;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        task_id = %task_id,
                        error = %error,
                        "hash verification failed to run"
                    );
                }
            }
        }

        spawn_schedule_queued_tasks(
            task_app,
            task_pool,
            downloads_map,
            request_headers,
            task_scheduler,
            state_speed_limiter,
            task_engine_registry,
        );
    });

    downloads.lock().await.insert(
        map_task_id,
        DownloadControl {
            cancel,
            handle,
            source_key,
            connection_slots: connection_limit.max(1),
        },
    );

    emit_queue_changed(&app);
    Ok(())
}

async fn host_connection_slots(
    downloads: &Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    source_key: &str,
) -> usize {
    downloads
        .lock()
        .await
        .values()
        .filter(|control| control.source_key == source_key)
        .map(|control| control.connection_slots)
        .sum()
}

fn spawn_schedule_queued_tasks(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
) {
    tokio::spawn(async move {
        schedule_queued_tasks_inner(
            app,
            pool,
            downloads,
            request_headers,
            scheduler,
            speed_limiter,
            engine_registry,
        )
        .await;
    });
}

fn spawn_schedule_queued_tasks_after(app: AppHandle, state: &AppState, delay: std::time::Duration) {
    let pool = state.pool.clone();
    let downloads = state.downloads.clone();
    let request_headers = state.request_headers.clone();
    let scheduler = state.scheduler.clone();
    let speed_limiter = state.speed_limiter.clone();
    let engine_registry = state.engine_registry.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        schedule_queued_tasks_inner(
            app,
            pool,
            downloads,
            request_headers,
            scheduler,
            speed_limiter,
            engine_registry,
        )
        .await;
    });
}

async fn resolve_task_request_headers(
    pool: &sqlx::SqlitePool,
    request_headers: TaskRequestHeaders,
    task_id: &str,
) -> Result<Vec<(String, String)>, String> {
    if let Some(headers) = request_headers.lock().await.get(task_id).cloned() {
        return Ok(headers);
    }
    let persisted = db::resolve_task_request_headers(pool, task_id).await?;
    if !persisted.is_empty() {
        request_headers
            .lock()
            .await
            .insert(task_id.to_string(), persisted.clone());
    }
    Ok(persisted)
}

async fn queue_task_for_retry(
    app: &AppHandle,
    state: &AppState,
    id: &str,
) -> Result<TaskRecord, String> {
    queue_task_for_retry_at(app, state, id, None).await
}

async fn queue_task_for_retry_at(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    retry_after_at: Option<&str>,
) -> Result<TaskRecord, String> {
    db::update_task_status(
        &state.pool,
        id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await?;
    db::update_task_retry_after(&state.pool, id, retry_after_at).await?;
    db::update_segments_status_for_task(
        &state.pool,
        id,
        crate::models::SegmentStatus::Pending,
        None,
    )
    .await?;
    let task = require_task(&state.pool, id).await?;
    emit_task_progress_snapshot(app, &task);
    emit_task_updated_record(app, &state.pool, &task).await;
    emit_queue_changed(app);
    if retry_after_at.is_none() {
        schedule_queued_tasks(app.clone(), state).await;
    }
    require_task(&state.pool, id).await
}

async fn update_recovery_target(
    app: &AppHandle,
    state: &AppState,
    task: &TaskRecord,
    input: &ResolveTaskAttentionInput,
) -> Result<(), String> {
    let save_dir = input
        .save_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&task.save_dir));
    std::fs::create_dir_all(&save_dir)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;

    let requested_file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&task.file_name);
    let final_path = unique_final_path(&save_dir, requested_file_name);
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested_file_name)
        .to_string();

    db::update_task_save_target(
        &state.pool,
        &task.id,
        &file_name,
        &save_dir.to_string_lossy(),
        &final_path.to_string_lossy(),
    )
    .await?;
    if let Some(updated) = db::get_task_record(&state.pool, &task.id).await? {
        emit_task_updated_record(app, &state.pool, &updated).await;
    }
    Ok(())
}

async fn restart_task_from_beginning(
    app: &AppHandle,
    state: &AppState,
    task: &TaskRecord,
) -> Result<TaskRecord, String> {
    if let Some(control) = state.downloads.lock().await.remove(&task.id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }
    if let Some(temp_path) = task.temp_path.as_deref() {
        match std::fs::remove_file(temp_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("Could not delete the temporary file: {error}"));
            }
        }
    }

    let engine = state.engine_registry.engine_for_uri(&task.url)?;
    let request_headers =
        resolve_task_request_headers(&state.pool, state.request_headers.clone(), &task.id).await?;
    let probe = engine
        .probe(ProbeRequest {
            uri: task.url.clone(),
            source: None,
            request_headers,
        })
        .await?;
    db::update_task_remote_metadata(
        &state.pool,
        &task.id,
        &probe.resolved_uri,
        probe.total_size,
        probe.etag.as_deref(),
        probe.last_modified.as_deref(),
        probe.content_type.as_deref(),
        probe.capabilities.supports_resume,
        probe.capabilities.supports_parallel,
        probe.capabilities.supports_multi_file,
        &probe.source_key,
    )
    .await?;
    db::delete_segments_for_task(&state.pool, &task.id).await?;
    db::reset_task_download_state(&state.pool, &task.id).await?;
    let settings = db::get_settings(
        &state.pool,
        super::settings::default_download_dir(app).unwrap_or_default(),
    )
    .await?;
    let task = require_task(&state.pool, &task.id).await?;
    db::ensure_task_segments_with_settings(&state.pool, &task, &settings).await?;
    emit_task_progress_snapshot(app, &task);
    emit_task_updated_record(app, &state.pool, &task).await;
    emit_queue_changed(app);
    schedule_queued_tasks(app.clone(), state).await;
    require_task(&state.pool, &task.id).await
}

fn restart_required_error_code(code: &str) -> bool {
    matches!(
        code,
        "remote_changed"
            | "resume_unavailable"
            | "temp_file_missing"
            | "temp_file_smaller_than_progress"
    )
}

fn task_error_code(task: &TaskRecord) -> Option<String> {
    if let Some(code) = task.error_code.clone() {
        return Some(code);
    }
    let error = task.error_message.as_deref()?;
    if let Ok(payload) = serde_json::from_str::<AppErrorPayload>(error) {
        return Some(payload.code);
    }
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
    None
}

async fn prepare_task_for_download(
    pool: &sqlx::SqlitePool,
    engine_registry: &EngineRegistry,
    task: TaskRecord,
    request_headers: &[(String, String)],
) -> Result<TaskRecord, String> {
    if task.status == TaskStatus::NeedsAttention {
        return Err("Remote file changed. Restart download to avoid corruption.".to_string());
    }

    if is_bt_protocol(&task.protocol) {
        db::ensure_task_segments(pool, &task).await?;
        return require_task(pool, &task.id).await;
    }

    let segments = db::ensure_task_segments(pool, &task).await?;
    let temp_path = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Task is missing a temporary path.".to_string())?;
    let temp_exists = temp_path.exists();
    let temp_size = std::fs::metadata(&temp_path)
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    if let Some(message) = segment_resume_error(
        &segments,
        task.downloaded_bytes,
        temp_exists,
        temp_size,
        task.total_size,
        task.supports_resume,
    ) {
        fail_task_and_segments(pool, &task.id, message).await?;
        db::insert_task_event(pool, &task.id, "resume_blocked", Some(message)).await?;
        return Err(message.to_string());
    }

    if temp_size > 0 {
        let uri = task.final_url.as_deref().unwrap_or(&task.url).to_string();
        let engine = engine_registry.engine_for_uri(&uri)?;
        let probe = engine
            .probe(ProbeRequest {
                uri,
                source: None,
                request_headers: request_headers.to_vec(),
            })
            .await?;
        if let Some(message) = resume_mismatch_message(&task, &probe) {
            db::update_task_status(
                pool,
                &task.id,
                TaskStatus::NeedsAttention,
                0,
                0,
                Some(&message),
                Some(&message),
            )
            .await?;
            db::update_segments_status_for_task(
                pool,
                &task.id,
                crate::models::SegmentStatus::Failed,
                Some(&message),
            )
            .await?;
            db::insert_task_event(pool, &task.id, "resume_blocked", Some(&message)).await?;
            return Err(message);
        }
        if let Some(message) = resume_decision_message(&task, &probe) {
            db::insert_task_event(pool, &task.id, "resume_checked", Some(&message)).await?;
        }
    }

    if segments.len() == 1 && temp_size > segments[0].downloaded_until {
        db::update_task_and_segment_progress(
            pool,
            &task.id,
            &segments[0].id,
            temp_size,
            0,
            0,
            task.status,
        )
        .await?;
    }

    require_task(pool, &task.id).await
}

pub fn local_resume_error(
    recorded_progress: i64,
    temp_exists: bool,
    temp_size: i64,
    total_size: i64,
    supports_parallel: bool,
) -> Option<&'static str> {
    if temp_size > total_size && total_size > 0 {
        return Some("Temporary file is larger than the remote file.");
    }
    if recorded_progress > 0 && !temp_exists {
        return Some("Temporary file is missing. Restart this download.");
    }
    if recorded_progress > temp_size {
        return Some("Temporary file is smaller than the recorded progress.");
    }
    if temp_size > 0 && !supports_parallel {
        return Some("Resume unavailable. Restart this download from the beginning.");
    }
    None
}

pub fn segment_resume_error(
    segments: &[TaskSegmentRecord],
    task_downloaded_bytes: i64,
    temp_exists: bool,
    temp_size: i64,
    total_size: i64,
    supports_parallel: bool,
) -> Option<&'static str> {
    if segments.is_empty() {
        return Some("Task has no segment records. Restart this download.");
    }
    if temp_size > total_size && total_size > 0 {
        return Some("Temporary file is larger than the remote file.");
    }

    let mut expected_start = 0_i64;
    let mut highest_recorded_offset = 0_i64;
    let mut downloaded_bytes = 0_i64;

    for segment in segments {
        if segment.range_start != expected_start || segment.range_end < segment.range_start {
            return Some("Segment records are inconsistent. Restart this download.");
        }
        if segment.downloaded_until < segment.range_start
            || segment.downloaded_until > segment.range_end.saturating_add(1)
        {
            return Some("Segment progress is outside its byte range. Restart this download.");
        }

        let clamped_until = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if clamped_until > segment.range_start {
            highest_recorded_offset = highest_recorded_offset.max(clamped_until);
        }
        downloaded_bytes += clamped_until.saturating_sub(segment.range_start);
        expected_start = segment.range_end.saturating_add(1);
    }

    if total_size > 0 && expected_start != total_size {
        return Some("Segment records do not match the remote file size. Restart this download.");
    }

    let recorded_progress = task_downloaded_bytes
        .max(downloaded_bytes)
        .max(highest_recorded_offset);
    if recorded_progress > 0 && !temp_exists {
        return Some("Temporary file is missing. Restart this download.");
    }
    if highest_recorded_offset > temp_size {
        return Some("Temporary file is smaller than the recorded progress.");
    }
    if temp_size > 0 && !supports_parallel {
        return Some("Resume unavailable. Restart this download from the beginning.");
    }
    None
}

pub trait ResumeProbe {
    fn total_size(&self) -> i64;
    fn supports_resume(&self) -> bool;
    fn etag(&self) -> Option<&String>;
    fn last_modified(&self) -> Option<&String>;
}

impl ResumeProbe for crate::download::ProbeOutput {
    fn total_size(&self) -> i64 {
        self.total_size
    }

    fn supports_resume(&self) -> bool {
        self.capabilities.supports_resume
    }

    fn etag(&self) -> Option<&String> {
        self.etag.as_ref()
    }

    fn last_modified(&self) -> Option<&String> {
        self.last_modified.as_ref()
    }
}

impl ResumeProbe for crate::download::ProbeResult {
    fn total_size(&self) -> i64 {
        self.total_size
    }

    fn supports_resume(&self) -> bool {
        self.supports_resume
    }

    fn etag(&self) -> Option<&String> {
        self.etag.as_ref()
    }

    fn last_modified(&self) -> Option<&String> {
        self.last_modified.as_ref()
    }
}

pub fn resume_mismatch_message<P: ResumeProbe>(task: &TaskRecord, probe: &P) -> Option<String> {
    if task.total_size != probe.total_size() {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    if !probe.supports_resume() {
        return Some("Server no longer supports resume. Restart this download.".to_string());
    }
    if strong_etag(task.etag.as_deref())
        && strong_etag(probe.etag().map(String::as_str))
        && task.etag.as_ref() != probe.etag()
    {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    if task.last_modified.is_some()
        && probe.last_modified().is_some()
        && task.last_modified.as_ref() != probe.last_modified()
    {
        return Some("Remote file changed. Restart download to avoid corruption.".to_string());
    }
    None
}

pub fn resume_decision_message<P: ResumeProbe>(task: &TaskRecord, probe: &P) -> Option<String> {
    if weak_etag(task.etag.as_deref()) || weak_etag(probe.etag().map(String::as_str)) {
        return Some(
            "Resume allowed with weak ETag metadata. Verify the file if the source is unstable."
                .to_string(),
        );
    }
    if task.etag.is_none() && task.last_modified.is_none() {
        return Some("Resume allowed without remote validators. Range metadata matched, but integrity depends on the server.".to_string());
    }
    Some("Resume metadata matched. Continuing from the temporary file.".to_string())
}

fn weak_etag(value: Option<&str>) -> bool {
    value
        .map(str::trim_start)
        .is_some_and(|value| value.starts_with("W/") || value.starts_with("w/"))
}

fn strong_etag(value: Option<&str>) -> bool {
    value.is_some_and(|value| !weak_etag(Some(value)))
}

fn is_bt_protocol(protocol: &str) -> bool {
    matches!(protocol, "bt" | "magnet")
}

fn is_torrent_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".torrent"))
}

async fn fail_task_and_segments(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    message: &str,
) -> Result<(), String> {
    db::update_task_status(
        pool,
        task_id,
        TaskStatus::Failed,
        0,
        0,
        Some(message),
        Some(message),
    )
    .await?;
    db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(message),
    )
    .await
}

async fn mark_download_failed(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    task_id: &str,
    error: String,
) {
    tracing::error!(task_id = %task_id, error = %error, "download failed");
    let payload = serde_json::from_str::<AppErrorPayload>(&error).ok();
    let status = match payload.as_ref().map(|payload| payload.code.as_str()) {
        Some("final_path_conflict" | "auth_headers_expired" | "auth_headers_unavailable") => {
            TaskStatus::NeedsAttention
        }
        _ => TaskStatus::Failed,
    };
    if let Err(db_error) =
        db::update_task_status(pool, task_id, status, 0, 0, Some(&error), Some(&error)).await
    {
        tracing::warn!(
            task_id = %task_id,
            error = %db_error,
            "failed to persist task failure status"
        );
    }
    if let Err(db_error) = db::insert_task_event(pool, task_id, "failed", Some(&error)).await {
        tracing::warn!(
            task_id = %task_id,
            error = %db_error,
            "failed to persist task failure event"
        );
    }
    if let Err(db_error) = db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(&error),
    )
    .await
    {
        tracing::warn!(
            task_id = %task_id,
            error = %db_error,
            "failed to persist segment failure status"
        );
    }
    if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
        emit_task_progress_snapshot(app, &task);
        emit_task_updated_record(app, pool, &task).await;
    }
    emit_queue_changed(app);
}

async fn require_task(pool: &sqlx::SqlitePool, id: &str) -> Result<TaskRecord, String> {
    db::get_task_record(pool, id)
        .await?
        .ok_or_else(|| "Task not found.".to_string())
}

async fn task_payload(pool: &sqlx::SqlitePool, id: &str) -> Result<Task, String> {
    let record = require_task(pool, id).await?;
    task_from_record_with_files(pool, record).await
}

async fn task_from_record_with_files(
    pool: &sqlx::SqlitePool,
    record: TaskRecord,
) -> Result<Task, String> {
    let files = db::list_task_file_records(pool, &record.id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let mut task = Task::from(record);
    task.files = files;
    Ok(task)
}

fn normalized_probe_files(probe: &ProbeOutput) -> Vec<crate::models::ProbedFile> {
    if probe.files.is_empty() {
        return vec![crate::models::ProbedFile {
            relative_path: probe.display_name.clone(),
            size: probe.total_size.to_string(),
            content_type: probe.content_type.clone(),
        }];
    }
    probe.files.clone()
}

fn task_file_records_from_probe(
    task: &TaskRecord,
    files: &[crate::models::ProbedFile],
    save_dir: &Path,
    single_final_path: &Path,
    single_temp_path: &Path,
    single_file_name: &str,
) -> Result<Vec<TaskFileRecord>, String> {
    let single_file = files.len() == 1;
    let mut records = Vec::with_capacity(files.len());
    for file in files {
        let (relative_path, file_name, final_path, temp_path) = if single_file {
            (
                single_file_name.to_string(),
                single_file_name.to_string(),
                single_final_path.to_path_buf(),
                single_temp_path.to_path_buf(),
            )
        } else {
            let relative_path = sanitize_relative_path(&file.relative_path);
            let parent = save_dir.join(relative_path.parent().unwrap_or_else(|| Path::new("")));
            std::fs::create_dir_all(&parent)
                .map_err(|e| format!("Could not create the download directory: {e}"))?;
            let requested_name = relative_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("download");
            let final_path = unique_final_path(&parent, requested_name);
            let relative_path = final_path
                .strip_prefix(save_dir)
                .unwrap_or(&final_path)
                .to_string_lossy()
                .replace('\\', "/");
            let file_name = final_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(requested_name)
                .to_string();
            let temp_path = PathBuf::from(format!("{}.vibe-downloading", final_path.display()));
            (relative_path, file_name, final_path, temp_path)
        };

        records.push(TaskFileRecord {
            id: Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            relative_path,
            file_name,
            save_dir: task.save_dir.clone(),
            temp_path: Some(temp_path.to_string_lossy().to_string()),
            final_path: Some(final_path.to_string_lossy().to_string()),
            total_size: parse_probed_file_size(&file.size),
            downloaded_bytes: 0,
            selected: true,
            status: TaskStatus::Queued,
            content_type: file.content_type.clone(),
        });
    }
    Ok(records)
}

fn sanitize_relative_path(value: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for component in value
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|component| !component.is_empty() && *component != "." && *component != "..")
    {
        path.push(sanitize_file_name(component));
    }
    if path.as_os_str().is_empty() {
        path.push(format!("download-{}", chrono::Utc::now().timestamp()));
    }
    path
}

fn parse_probed_file_size(value: &str) -> i64 {
    value.parse::<i64>().unwrap_or(0)
}

fn normalize_sha256(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = value.to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("SHA-256 must be 64 hexadecimal characters.".to_string());
    }
    Ok(Some(normalized))
}

async fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Could not open file for checksum verification: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Could not read file for checksum verification: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn remove_task_file_path(path: &str) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not delete {path}: {error}")),
    }
}

fn emit_task_progress_snapshot(app: &AppHandle, task: &TaskRecord) {
    let payload = crate::models::TaskProgressPayload {
        task_id: task.id.clone(),
        downloaded_bytes: task.downloaded_bytes.to_string(),
        total_size: task.total_size.to_string(),
        speed_bps: task.speed_bps.to_string(),
        connection_count: task.connection_count,
        status: task.status,
    };
    emit_task_progress(app, &payload);
}

fn unique_final_path(save_dir: &Path, requested_file_name: &str) -> PathBuf {
    let sanitized = sanitize_file_name(requested_file_name);
    let candidate = save_dir.join(&sanitized);
    if !candidate.exists()
        && !PathBuf::from(format!("{}.vibe-downloading", candidate.display())).exists()
    {
        return candidate;
    }

    let path = Path::new(&sanitized);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());

    for index in 1..10_000 {
        let name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem} ({index}).{extension}"),
            _ => format!("{stem} ({index})"),
        };
        let candidate = save_dir.join(name);
        if !candidate.exists()
            && !PathBuf::from(format!("{}.vibe-downloading", candidate.display())).exists()
        {
            return candidate;
        }
    }

    save_dir.join(format!("download-{}", chrono::Utc::now().timestamp()))
}

fn sanitize_file_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        format!("download-{}", chrono::Utc::now().timestamp())
    } else {
        trimmed
    }
}

#[cfg(debug_assertions)]
fn build_mock_tasks(now: &str) -> Vec<TaskRecord> {
    vec![
        mock_task(
            "ubuntu-24.04.iso",
            "https://releases.ubuntu.com/noble/ubuntu-24.04-desktop-amd64.iso",
            "releases.ubuntu.com",
            TaskStatus::Downloading,
            4_200_000_000,
            1_680_000_000,
            8,
            48_500_000,
            Some("Downloading steadily".into()),
            now,
        ),
        mock_task(
            "node-v22.pkg",
            "https://nodejs.org/dist/v22.0.0/node-v22.0.0.pkg",
            "nodejs.org",
            TaskStatus::Downloading,
            80_000_000,
            52_000_000,
            4,
            12_400_000,
            Some("Server limit detected".into()),
            now,
        ),
        mock_task(
            "rust-docs.pdf",
            "https://doc.rust-lang.org/book.pdf",
            "doc.rust-lang.org",
            TaskStatus::Paused,
            12_000_000,
            4_800_000,
            0,
            0,
            None,
            now,
        ),
        mock_task(
            "game-patch.zip",
            "https://cdn.example.com/patches/season-12.zip",
            "cdn.example.com",
            TaskStatus::Queued,
            2_400_000_000,
            0,
            0,
            0,
            None,
            now,
        ),
        mock_task(
            "dataset.tar.gz",
            "https://data.example.org/ml/dataset.tar.gz",
            "data.example.org",
            TaskStatus::Retrying,
            900_000_000,
            120_000_000,
            2,
            3_200_000,
            Some("Network fluctuation, retrying".into()),
            now,
        ),
        mock_task(
            "driver-setup.exe",
            "https://vendor.example.net/drivers/setup.exe",
            "vendor.example.net",
            TaskStatus::Failed,
            350_000_000,
            89_000_000,
            0,
            0,
            Some("Resume unavailable".into()),
            now,
        ),
        mock_task(
            "llm-weights.safetensors",
            "https://models.example.ai/weights/v3.safetensors",
            "models.example.ai",
            TaskStatus::NeedsAttention,
            8_000_000_000,
            2_100_000_000,
            0,
            0,
            Some("Remote file changed. Restart download to avoid corruption.".into()),
            now,
        ),
        mock_task(
            "archlinux.iso",
            "https://mirror.archlinux.org/iso/latest/archlinux-x86_64.iso",
            "mirror.archlinux.org",
            TaskStatus::Completed,
            1_300_000_000,
            1_300_000_000,
            0,
            0,
            Some("Completed".into()),
            now,
        ),
        mock_task(
            "fonts-bundle.zip",
            "https://github.com/google/fonts/archive/refs/heads/main.zip",
            "github.com",
            TaskStatus::WaitingNetwork,
            220_000_000,
            45_000_000,
            0,
            0,
            Some("Waiting for network".into()),
            now,
        ),
        mock_task(
            "vscode.deb",
            "https://code.visualstudio.com/sha/download?build=stable&os=linux-deb-x64",
            "code.visualstudio.com",
            TaskStatus::Downloading,
            95_000_000,
            71_000_000,
            2,
            8_900_000,
            Some("Disk write slower than network".into()),
            now,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
#[cfg(debug_assertions)]
fn mock_task(
    file_name: &str,
    url: &str,
    host: &str,
    status: TaskStatus,
    total_size: i64,
    downloaded_bytes: i64,
    connection_count: i32,
    speed_bps: i64,
    health_summary: Option<String>,
    now: &str,
) -> TaskRecord {
    let error_message = if matches!(status, TaskStatus::Failed | TaskStatus::NeedsAttention) {
        health_summary.clone()
    } else {
        None
    };

    TaskRecord {
        id: Uuid::new_v4().to_string(),
        url: url.to_string(),
        final_url: Some(url.to_string()),
        protocol: "https".to_string(),
        task_kind: crate::models::TaskKind::SingleFile,
        file_name: file_name.to_string(),
        save_dir: "~/Downloads".to_string(),
        temp_path: None,
        final_path: None,
        total_size,
        downloaded_bytes,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: host.to_string(),
        connection_count,
        speed_bps,
        health_summary,
        error_message,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}
