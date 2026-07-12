use std::{path::PathBuf, sync::atomic::Ordering};

use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use super::task_file_planning::unique_final_path;
pub use super::task_resume::{
    local_resume_error, resume_decision_message, resume_mismatch_message, segment_resume_error,
};

use crate::{
    db,
    download::{EngineRegistry, ProbeRequest},
    events::{
        emit_queue_changed, emit_queue_changed_with_ids, emit_task_progress,
        emit_task_updated_record,
    },
    models::{
        AppErrorPayload, FtpDirectoryProbe, RecoveryAction, SftpDirectoryProbe, Task,
        TaskChecksumRecord, TaskFileRecord, TaskProxySettings, TaskProxySettingsInput, TaskRecord,
        TaskStatus, WebDavDirectoryProbe,
    },
    state_machine::TransitionError,
    AppState, TaskRequestHeaders,
};

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
pub struct UpdateTorrentFileSelectionInput {
    pub task_id: String,
    pub selected_file_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTorrentSeedingInput {
    pub task_id: String,
    pub enabled: bool,
    pub ratio_limit: Option<f64>,
    pub time_limit_seconds: Option<String>,
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

#[tauri::command]
#[specta::specta]
pub async fn update_torrent_file_selection(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    input: UpdateTorrentFileSelectionInput,
) -> Result<Task, String> {
    let task = require_task(&state.pool, &input.task_id).await?;
    if task.protocol != "bt" {
        return Err("File selection is only available for BitTorrent tasks.".to_string());
    }
    if matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err("Pause the torrent before changing file selection.".to_string());
    }
    let selected = input
        .selected_file_paths
        .iter()
        .map(|value| value.trim().replace('\\', "/"))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(crate::models::AppErrorPayload {
            code: "bt_file_selection_required".to_string(),
            message: "Choose at least one torrent file before downloading.".to_string(),
            recoverable: true,
            actions: vec!["check_url".to_string()],
        }
        .command_error());
    }
    let mut tx = state.pool.begin().await.map_err(|e| e.to_string())?;
    db::update_task_file_selection_in_tx(&mut tx, &task.id, &selected).await?;
    db::insert_task_event_in_tx(&mut tx, &task.id, "bt_file_selection_updated", None).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    crate::state_machine::transition_task(
        &app,
        &state.pool,
        &task.id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await
    .map_err(String::from)?;
    emit_queue_changed_with_ids(&app, Some(vec![task.id.clone()]));
    state
        .scheduler
        .clone()
        .dispatch(app.clone(), state.pool.clone())
        .await;
    task_payload(&state.pool, &task.id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_torrent_seeding(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    input: UpdateTorrentSeedingInput,
) -> Result<Task, String> {
    let task = require_task(&state.pool, &input.task_id).await?;
    if task.protocol != "bt" {
        return Err("Seeding is only available for BitTorrent tasks.".to_string());
    }
    let time_limit_seconds = input
        .time_limit_seconds
        .as_deref()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0);
    db::update_torrent_seeding(
        &state.pool,
        &task.id,
        input.enabled,
        input.ratio_limit.filter(|value| *value > 0.0),
        time_limit_seconds,
    )
    .await?;
    db::insert_task_event(
        &state.pool,
        &task.id,
        if input.enabled {
            "bt_seeding_enabled"
        } else {
            "bt_seeding_disabled"
        },
        None,
    )
    .await?;
    if !input.enabled {
        state
            .engine_registry
            .delete_runtime_task(&task, false)
            .await;
    }
    if let Some(updated) = db::get_task_record(&state.pool, &task.id).await? {
        emit_task_updated_record(&app, &state.pool, &updated).await;
    }
    task_payload(&state.pool, &task.id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_task_proxy_settings(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<TaskProxySettings, String> {
    db::get_task_proxy_settings(&state.pool, &task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_proxy_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    input: TaskProxySettingsInput,
) -> Result<TaskProxySettings, String> {
    let task = require_task(&state.pool, &input.task_id).await?;
    if matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err("Pause the task before changing its proxy settings.".to_string());
    }
    if input.mode == crate::models::TaskProxyMode::Custom {
        if let Some(url) = input
            .proxy_url
            .as_deref()
            .and_then(crate::proxy::normalize_proxy_url)
        {
            db::validate_task_proxy_protocol(&task.protocol, &url)?;
        }
    }
    let settings = db::upsert_task_proxy_settings(&state.pool, input).await?;
    db::insert_task_event(&state.pool, &task.id, "task_proxy_updated", None).await?;
    if let Some(updated) = db::get_task_record(&state.pool, &task.id).await? {
        emit_task_updated_record(&app, &state.pool, &updated).await;
    }
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub async fn probe_ftp_directory(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<FtpDirectoryProbe, String> {
    let proxy_config = state.engine_registry.proxy_config().await;
    crate::download::ftp::probe_ftp_directory_url(&url, proxy_config).await
}

#[tauri::command]
#[specta::specta]
pub async fn probe_sftp_directory(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<SftpDirectoryProbe, String> {
    let proxy_config = state.engine_registry.proxy_config().await;
    crate::download::sftp::probe_sftp_directory_url(&state.pool, &url, proxy_config).await
}

#[tauri::command]
#[specta::specta]
pub async fn probe_webdav_directory(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<WebDavDirectoryProbe, String> {
    let proxy_config = state.engine_registry.proxy_config().await;
    crate::download::webdav::probe_webdav_directory_url(&url, proxy_config).await
}

// --- migrated scheduler functions removed (see crate::scheduler) ---

/// Enforces the configured download-window schedule by pausing active tasks
/// when the window is closed and resuming previously schedule-paused tasks
/// when it opens.  Called periodically by the background monitor and
/// immediately after schedule-related settings change.
pub(crate) async fn check_schedule_preemption(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
    let settings = db::get_settings(&state.pool, default_dir).await?;
    if !settings.schedule_download_window_enabled {
        return Ok(());
    }

    let window_active = db::local_time_window_active(
        &settings.schedule_download_window_start,
        &settings.schedule_download_window_end,
    );

    if !window_active {
        // Window just closed — pause downloading tasks that obey the schedule.
        let active_ids: Vec<String> = {
            let downloads = state.downloads.lock().await;
            downloads.keys().cloned().collect()
        };
        for task_id in &active_ids {
            let record = match db::get_task_record(&state.pool, task_id).await? {
                Some(r) => r,
                None => continue,
            };
            if !record.obey_schedule {
                continue;
            }
            if record.status != TaskStatus::Downloading {
                continue;
            }
            tracing::info!(task_id = %task_id, "pausing task: schedule window closed");
            if let Err(err) = pause_task(app.clone(), state.clone(), task_id.clone()).await {
                tracing::warn!(task_id = %task_id, error = %err, "schedule auto-pause failed");
                continue;
            }
            // Tag as schedule-paused AFTER the normal "paused" event so this
            // row has the highest ID and is seen as the latest pause reason.
            let _ = db::insert_task_event(&state.pool, task_id, "paused_by_schedule", None).await;
        }
    } else {
        // Window just opened — resume tasks that were paused by schedule.
        let paused_ids = db::list_paused_schedulable_tasks(&state.pool)
            .await
            .map_err(|e| e.to_string())?;

        let mut resumed_any = false;
        for task_id in &paused_ids {
            let latest_pause = db::get_latest_pause_event_type(&state.pool, task_id).await?;
            if latest_pause.as_deref() != Some("paused_by_schedule") {
                continue;
            }
            tracing::info!(task_id = %task_id, "resuming task: schedule window opened");
            db::insert_task_event(&state.pool, task_id, "resumed", None).await?;
            if let Err(err) = queue_task_for_retry(&app, state.inner(), task_id).await {
                tracing::warn!(task_id = %task_id, error = %err, "schedule auto-resume failed");
            }
            resumed_any = true;
        }
        if resumed_any {
            emit_queue_changed(&app);
        }
    }

    Ok(())
}

/// Spawns a background task that checks the schedule window at boundary
/// crossings and preempts running tasks or resumes paused tasks as needed.
///
/// E-5: Instead of polling every 60s, the monitor sleeps until the next
/// window boundary (start or end, whichever comes first). This eliminates
/// up to 60s latency at boundary crossings. When the schedule is disabled,
/// it re-checks every 5 minutes for settings changes.
pub(crate) fn spawn_schedule_window_monitor(app: AppHandle, _state: &AppState) {
    tauri::async_runtime::spawn(async move {
        // The initial check runs synchronously in `lib.rs` setup() before
        // this spawns, so we start with a sleep.
        loop {
            // Calculate sleep duration based on current schedule settings.
            let sleep = {
                let state_ref = app.state::<AppState>();
                if state_ref.quit_requested.load(Ordering::SeqCst) {
                    tracing::debug!("schedule window monitor exiting (shutdown requested)");
                    return;
                }
                let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
                match db::get_settings(&state_ref.pool, default_dir).await {
                    Ok(settings) if settings.schedule_download_window_enabled => {
                        db::duration_until_next_window_boundary(
                            &settings.schedule_download_window_start,
                            &settings.schedule_download_window_end,
                        )
                    }
                    Ok(_) => {
                        // Schedule disabled — re-check periodically for changes.
                        std::time::Duration::from_secs(300)
                    }
                    Err(_) => {
                        // Settings read failed — retry in 1 minute.
                        std::time::Duration::from_secs(60)
                    }
                }
            };
            tokio::time::sleep(sleep).await;

            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("schedule window monitor exiting (shutdown requested)");
                break;
            }
            if let Err(error) = check_schedule_preemption(app.clone(), state_ref).await {
                tracing::warn!(error = %error, "schedule preemption check failed");
            }
        }
    });
}

/// Interval between background `task_requests` cleanup passes. Long enough
/// that it never contends with hot download paths, short enough that the
/// table stays bounded for long-running sessions (HLS/live, high-retry).
const REQUEST_DIAGNOSTICS_CLEANUP_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// Spawns a background task that periodically prunes the `task_requests`
/// diagnostic table. The first pass also runs shortly after startup so a
/// long-closed app gets cleaned before any new traffic arrives.
pub(crate) fn spawn_request_diagnostics_cleanup(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            REQUEST_DIAGNOSTICS_CLEANUP_INTERVAL_SECS,
        ));
        // First tick fires immediately; skip it — startup cleanup runs
        // synchronously in `lib.rs` setup() before this spawns.
        interval.tick().await;
        loop {
            interval.tick().await;
            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("request diagnostics cleanup exiting (shutdown requested)");
                break;
            }
            match db::prune_request_diagnostics(&state_ref.pool).await {
                Ok(0) => {}
                Ok(removed) => {
                    tracing::info!(removed, "pruned stale request diagnostics");
                }
                Err(error) => {
                    tracing::warn!(error = %error, "request diagnostics prune failed");
                }
            }
        }
    });
}

/// Interval between background WAL checkpoint passes when downloads are
/// active. Long enough that it never contends with hot download paths.
const WAL_CHECKPOINT_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// E-5: Shorter WAL checkpoint interval when no downloads are active.
/// Allows WAL to be checkpointed sooner during idle periods, bounding
/// `-wal` file growth without waiting up to 6 hours.
const WAL_CHECKPOINT_IDLE_INTERVAL_SECS: u64 = 30 * 60;

/// Spawns a background task that periodically runs `PRAGMA wal_checkpoint(TRUNCATE)`
/// to bound `-wal` file growth for long-running sessions.
///
/// E-5: Uses a shorter interval (30 min) when there are no active downloads,
/// so WAL gets checkpointed sooner during idle periods. When downloads are
/// active, keeps the conservative 6-hour interval to avoid interfering with I/O.
pub(crate) fn spawn_wal_checkpoint_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // First pass: wait a short grace period after startup so the initial
        // download burst doesn't trigger an immediate checkpoint.
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        loop {
            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("wal checkpoint monitor exiting (shutdown requested)");
                break;
            }
            // E-5: Choose interval based on whether downloads are active.
            let is_idle = {
                let downloads = state_ref.downloads.lock().await;
                downloads.is_empty()
            };
            let interval_secs = if is_idle {
                WAL_CHECKPOINT_IDLE_INTERVAL_SECS
            } else {
                WAL_CHECKPOINT_INTERVAL_SECS
            };
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("wal checkpoint monitor exiting (shutdown requested)");
                break;
            }
            if let Err(error) = db::wal_checkpoint(&state_ref.pool).await {
                tracing::warn!(error = %error, "periodic WAL checkpoint failed");
            }
        }
    });
}

pub(crate) async fn resolve_task_request_headers(
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
    match crate::state_machine::transition_task(
        app,
        &state.pool,
        id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await
    {
        Ok(_) => {}
        Err(TransitionError::Conflict { .. }) => {
            return Err("Task state changed concurrently, please refresh.".to_string());
        }
        Err(error) => return Err(error.into()),
    }
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
    emit_queue_changed_with_ids(app, Some(vec![id.to_string()]));
    if retry_after_at.is_none() {
        // R-2.3: Spawn dispatch in background to avoid deadlock with the
        // per-task runtime lock held by the caller (retry_task/resume_task/
        // resolve_task_attention). dispatch → start_task will acquire the
        // per-task lock, which is still held by the caller until it returns.
        // Spawning lets the caller unwind and release the lock first.
        let dispatch_app = app.clone();
        let dispatch_pool = state.pool.clone();
        let dispatch_scheduler = state.scheduler.clone();
        tauri::async_runtime::spawn(async move {
            dispatch_scheduler
                .dispatch(dispatch_app, dispatch_pool)
                .await;
        });
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
    // R-2.3: Caller (resolve_task_attention) must already hold the per-task
    // runtime lock. tokio::sync::Mutex is not re-entrant, so we do not
    // re-acquire here.
    if let Some(control) = state.downloads.lock().await.remove(&task.id) {
        control.cancel_token.cancel();
        if let Some(h) = control.handle.as_ref() {
            h.abort();
        }
    }
    if let Some(temp_path) = task.temp_path.as_deref() {
        remove_task_path(temp_path)?;
    }

    let engine = state.engine_registry.engine_for_uri(&task.url)?;
    let request_headers =
        resolve_task_request_headers(&state.pool, state.request_headers.clone(), &task.id).await?;
    let probe = engine
        .probe(ProbeRequest {
            uri: task.url.clone(),
            source: None,
            request_headers,
            pool: Some(state.pool.clone()),
            task_id: Some(task.id.clone()),
            credentials: None,
            app: None,
            request_id: None,
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
    if task.protocol == "hls" {
        db::reset_hls_segments_for_task(&state.pool, &task.id).await?;
    }
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
    emit_queue_changed_with_ids(app, Some(vec![task.id.clone()]));
    state
        .scheduler
        .clone()
        .dispatch(app.clone(), state.pool.clone())
        .await;
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

pub(crate) async fn prepare_task_for_download(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    engine_registry: &EngineRegistry,
    task: TaskRecord,
    request_headers: &[(String, String)],
) -> Result<TaskRecord, String> {
    if task.status == TaskStatus::NeedsAttention {
        return Err("Remote file changed. Restart download to avoid corruption.".to_string());
    }

    if is_bt_protocol(&task.protocol)
        || is_hls_protocol(&task.protocol)
        || is_dash_protocol(&task.protocol)
        || is_metalink_protocol(&task.protocol)
        || is_sftp_protocol(&task.protocol)
    {
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
        fail_task_and_segments(app, pool, &task.id, message).await?;
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
                pool: Some(pool.clone()),
                task_id: Some(task.id.clone()),
                credentials: None,
                app: None,
                request_id: None,
            })
            .await?;
        if let Some(message) = resume_mismatch_message(&task, &probe) {
            crate::state_machine::transition_task(
                app,
                pool,
                &task.id,
                TaskStatus::NeedsAttention,
                0,
                0,
                Some(&message),
                Some("resume_blocked"),
            )
            .await
            .map_err(String::from)?;
            db::update_segments_status_for_task(
                pool,
                &task.id,
                crate::models::SegmentStatus::Failed,
                Some(&message),
            )
            .await?;
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

fn is_hls_protocol(protocol: &str) -> bool {
    protocol == "hls"
}

fn is_dash_protocol(protocol: &str) -> bool {
    protocol == "dash"
}

fn is_metalink_protocol(protocol: &str) -> bool {
    protocol == "metalink"
}

fn is_sftp_protocol(protocol: &str) -> bool {
    protocol == "sftp"
}

// URL 分类函数已统一至 `crate::download::url_classify`，此处 re-export 以保持
// `super::is_torrent_url` 等调用路径不变。
pub(crate) use crate::download::url_classify::{is_dash_url, is_metalink_url, is_torrent_url};

async fn fail_task_and_segments(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    task_id: &str,
    message: &str,
) -> Result<(), String> {
    crate::state_machine::transition_task(
        app,
        pool,
        task_id,
        TaskStatus::Failed,
        0,
        0,
        Some(message),
        None,
    )
    .await
    .map_err(String::from)?;
    db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(message),
    )
    .await
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
    let mut checksums_by_task_id =
        db::list_task_checksum_records_for_tasks(pool, &task_ids).await?;
    Ok(records
        .into_iter()
        .map(|record| {
            let files = files_by_task_id.remove(&record.id).unwrap_or_default();
            let checksums = checksums_by_task_id.remove(&record.id).unwrap_or_default();
            task_from_record_and_files(record, files, checksums)
        })
        .collect())
}

async fn task_from_record_with_files(
    pool: &sqlx::SqlitePool,
    record: TaskRecord,
) -> Result<Task, String> {
    let files = db::list_task_file_records(pool, &record.id).await?;
    let checksums = db::list_task_checksum_records(pool, &record.id).await?;
    Ok(task_from_record_and_files(record, files, checksums))
}

fn task_from_record_and_files(
    record: TaskRecord,
    files: Vec<TaskFileRecord>,
    checksums: Vec<TaskChecksumRecord>,
) -> Task {
    let mut task = Task::from(record);
    task.files = files.into_iter().map(Into::into).collect();
    task.checksums = checksums.into_iter().map(Into::into).collect();
    task
}

pub(crate) fn emit_task_progress_snapshot(app: &AppHandle, task: &TaskRecord) {
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

/// Delete a file or directory, optionally sending to the OS trash/recycle bin.
///
/// When `use_trash` is true, the `trash` crate attempts to move the path to
/// the system's recycle bin. If that fails, an error is returned — the file
/// is **not** permanently deleted, giving the caller a chance to warn the user.
/// When `use_trash` is false, deletes permanently. `NotFound` errors are
/// silently ignored in both modes.
pub(super) fn delete_path(path: &str, use_trash: bool) -> Result<(), String> {
    let path_obj = std::path::Path::new(path);
    if !path_obj.exists() {
        return Ok(());
    }

    if use_trash {
        if let Err(error) = trash::delete(path_obj) {
            // Return the error — do NOT fall through to permanent deletion.
            // Silent permanent deletion when the user expected trash is a
            // data-loss footgun (the user checks the recycle bin and finds nothing).
            return Err(format!(
                "Could not move {path} to the recycle bin: {error}. The file was not deleted."
            ));
        }
        return Ok(());
    }

    // Permanent deletion
    if path_obj.is_dir() {
        match std::fs::remove_dir_all(path_obj) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not delete {path}: {error}")),
        }
    } else {
        match std::fs::remove_file(path_obj) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not delete {path}: {error}")),
        }
    }
}

/// Delete a task's temporary file/folder. Always permanent (temp files
/// should not clutter the recycle bin).
fn remove_task_path(path: &str) -> Result<(), String> {
    delete_path(path, false)
}
