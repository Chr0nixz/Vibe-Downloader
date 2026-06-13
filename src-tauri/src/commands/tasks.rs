use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use reqwest::Url;
use serde::Deserialize;
use specta::Type;
use tauri::AppHandle;

use super::task_file_planning::unique_final_path;
pub use super::task_resume::{
    local_resume_error, resume_decision_message, resume_mismatch_message, segment_resume_error,
};

use crate::{
    db,
    download::{DownloadContext, EngineRegistry, ProbeRequest},
    events::{emit_queue_changed, emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        AppErrorPayload, HashVerificationStatus, RecoveryAction, Task, TaskFileRecord, TaskRecord,
        TaskStatus,
    },
    AppState, DownloadControl, TaskRequestHeaders,
};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTaskAttentionInput {
    pub id: String,
    pub action: RecoveryAction,
    pub file_name: Option<String>,
    pub save_dir: Option<String>,
}

mod create;

pub use create::*;

mod query;

pub use query::*;

mod actions;

pub use actions::*;

#[cfg(debug_assertions)]
mod mock_seed;

#[cfg(debug_assertions)]
pub use mock_seed::*;

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

        let queued_limit = i64::from(settings.max_active_tasks)
            .saturating_mul(i64::from(settings.max_connections_per_host).max(1))
            .clamp(
                i64::from(settings.max_active_tasks).max(1),
                db::DEFAULT_TASK_PAGE_SIZE,
            );
        let queued = match db::list_queued_task_records(&pool, queued_limit).await {
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
                DownloadStartContext {
                    app: app.clone(),
                    pool: pool.clone(),
                    downloads: downloads.clone(),
                    request_headers: request_headers.clone(),
                    scheduler: scheduler.clone(),
                    speed_limiter: speed_limiter.clone(),
                    engine_registry: engine_registry.clone(),
                },
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

struct DownloadStartContext {
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
}

async fn start_task_download(
    context: DownloadStartContext,
    task: TaskRecord,
    connection_limit: usize,
) -> Result<(), String> {
    let DownloadStartContext {
        app,
        pool,
        downloads,
        request_headers,
        scheduler,
        speed_limiter,
        engine_registry,
    } = context;

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
        db::TaskRemoteMetadataUpdate {
            final_url: &probe.resolved_uri,
            total_size: probe.total_size,
            etag: probe.etag.as_deref(),
            last_modified: probe.last_modified.as_deref(),
            content_type: probe.content_type.as_deref(),
            supports_resume: probe.capabilities.supports_resume,
            supports_parallel: probe.capabilities.supports_parallel,
            supports_multi_file: probe.capabilities.supports_multi_file,
            source_key: &probe.source_key,
        },
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

fn is_bt_protocol(protocol: &str) -> bool {
    matches!(protocol, "bt" | "magnet")
}

fn is_torrent_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
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

async fn tasks_from_records_with_files(
    pool: &sqlx::SqlitePool,
    records: Vec<TaskRecord>,
) -> Result<Vec<Task>, String> {
    let task_ids = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let mut files_by_task_id = db::list_task_file_records_for_tasks(pool, &task_ids).await?;
    Ok(records
        .into_iter()
        .map(|record| {
            let files = files_by_task_id.remove(&record.id).unwrap_or_default();
            task_from_record_and_files(record, files)
        })
        .collect())
}

async fn task_from_record_with_files(
    pool: &sqlx::SqlitePool,
    record: TaskRecord,
) -> Result<Task, String> {
    let files = db::list_task_file_records(pool, &record.id).await?;
    Ok(task_from_record_and_files(record, files))
}

fn task_from_record_and_files(record: TaskRecord, files: Vec<TaskFileRecord>) -> Task {
    let mut task = Task::from(record);
    task.files = files.into_iter().map(Into::into).collect();
    task
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
