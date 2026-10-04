use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    db,
    download::checksum::hash_file,
    events::{emit_queue_changed, emit_queue_changed_with_ids, emit_task_updated_record},
    models::{
        task::now_iso, AppErrorPayload, ChecksumAlgorithm, HashVerificationState,
        HashVerificationStatus, RecoveryAction, Task, TaskPriority, TaskProxyMode,
        TaskRequestHeaderInput, TaskRequestProfileInput, TaskStatus,
    },
    platform,
    state_machine::TransitionError,
    AppState,
};

use super::{
    create::{create_task_with_state, create_task_with_state_and_headers_until, CreateTaskInput},
    delete_path, emit_task_progress_snapshot, queue_task_for_retry_at,
    queue_task_for_retry_with_event, require_task, restart_required_error_code,
    restart_task_from_beginning, task_error_code, task_from_record_with_files, task_payload,
    update_recovery_target, ResolveTaskAttentionInput,
};

#[cfg(debug_assertions)]
use super::create::create_task_headless_with_headers_until;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetalinkMirrorView {
    pub id: String,
    pub url: String,
    pub priority: i32,
    pub location: Option<String>,
    pub status: String,
    pub failure_count: i32,
    pub last_error: Option<String>,
    /// Task file this mirror belongs to (multi-file Metalink manifests).
    pub file_id: Option<String>,
}

// Four workers keep slow recycle-bin/network-volume calls off Tokio without
// flooding the OS shell or storage device during large batch removals.
pub(crate) const MAX_CONCURRENT_FILE_DELETES: usize = 4;

#[derive(Debug, Clone)]
pub(crate) struct FileDeleteRequest {
    pub path: String,
    pub use_trash: bool,
}

/// One completed delete attempt keyed by path, so callers can report per-item
/// outcomes — the Storage Center must never show partial success as full
/// success.
#[derive(Debug)]
pub(crate) struct FileDeleteOutcome {
    pub path: String,
    pub result: Result<(), String>,
}

/// Global pause must leave already-written bytes intact when the source cannot
/// resume; callers report these tasks as skipped instead of changing status.
fn pause_would_discard_progress(task: &crate::models::TaskRecord) -> bool {
    task.downloaded_bytes > 0 && !task.supports_resume
}

pub(crate) async fn delete_paths_off_runtime(
    requests: Vec<FileDeleteRequest>,
) -> Vec<FileDeleteOutcome> {
    let mut seen = HashSet::new();
    let unique = requests
        .into_iter()
        .filter(|request| seen.insert(request.path.clone()))
        .collect::<Vec<_>>();
    let mut pending = stream::iter(unique.into_iter().map(|request| {
        tokio::task::spawn_blocking(move || {
            let result = delete_path(&request.path, request.use_trash);
            (request.path, result)
        })
    }))
    .buffer_unordered(MAX_CONCURRENT_FILE_DELETES);
    let mut outcomes = Vec::new();
    while let Some(outcome) = pending.next().await {
        match outcome {
            Ok((path, result)) => outcomes.push(FileDeleteOutcome { path, result }),
            Err(error) => outcomes.push(FileDeleteOutcome {
                path: String::new(),
                result: Err(format!("File deletion worker failed: {error}")),
            }),
        }
    }
    outcomes
}

impl MetalinkMirrorView {
    fn from_record(r: db::MetalinkResourceRecord) -> Self {
        Self {
            id: r.id,
            url: r.url,
            priority: r.priority as i32,
            location: r.location,
            status: r.status,
            failure_count: r.failure_count as i32,
            last_error: r.last_error,
            file_id: Some(r.file_id).filter(|id| !id.is_empty()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskTransferOptionsInput {
    pub id: String,
    pub task_speed_limit_bps: Option<String>,
    pub priority: Option<TaskPriority>,
    pub queue_position: Option<String>,
    pub category_key: Option<String>,
    pub obey_schedule: Option<bool>,
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_transfer_options(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateTaskTransferOptionsInput,
) -> Result<Task, String> {
    let current = require_task(&state.pool, &input.id).await?;
    let task_speed_limit_bps = input
        .task_speed_limit_bps
        .as_deref()
        .and_then(db::normalize_speed_limit_bps);
    let priority = input.priority.unwrap_or(current.priority);
    let queue_position = input
        .queue_position
        .as_deref()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(current.queue_position);
    let category_key = input
        .category_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let obey_schedule = input.obey_schedule.unwrap_or(current.obey_schedule);

    db::update_task_transfer_options(
        &state.pool,
        &input.id,
        db::TaskTransferOptionsUpdate {
            task_speed_limit_bps,
            priority,
            queue_position,
            category_key,
            obey_schedule,
        },
    )
    .await?;
    let updated = require_task(&state.pool, &input.id).await?;
    let default_dir = super::super::settings::default_download_dir(&app).unwrap_or_default();
    if let Err(error) = state
        .scheduler
        .refresh_speed_limit_policies(&state.pool, default_dir)
        .await
    {
        tracing::warn!(task_id = %input.id, error = %error, "active task speed policy refresh failed");
    }
    emit_task_updated_record(&app, &state.pool, &updated).await;
    emit_queue_changed_with_ids(&app, Some(vec![updated.id.clone()]));
    if matches!(updated.status, TaskStatus::Queued) {
        state
            .scheduler
            .clone()
            .dispatch(app.clone(), state.pool.clone())
            .await;
    }
    task_payload(&state.pool, &input.id).await
}

/// Reassign `queue_position` for the given task ids based on their order in
/// the input slice. Only `Queued` tasks should be included; the caller (UI)
/// is responsible for filtering. The full set is rebalanced with a step of
/// 1000 so future insertions still have room. Emits `task-updated` for each
/// affected task and a single `queue-changed` event so the frontend refreshes.
#[tauri::command]
#[specta::specta]
pub async fn reorder_queued_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> Result<(), String> {
    if task_ids.is_empty() {
        return Ok(());
    }
    db::reorder_queued_tasks(&state.pool, &task_ids).await?;
    for id in &task_ids {
        if let Ok(Some(record)) = db::get_task_record(&state.pool, id).await {
            emit_task_updated_record(&app, &state.pool, &record).await;
        }
    }
    emit_queue_changed(&app);
    state
        .scheduler
        .clone()
        .dispatch(app, state.pool.clone())
        .await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn pause_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(&id).await;
    tracing::info!(task_id = %id, "pausing task");
    crate::remove_and_drain_control(
        &state.downloads,
        &state.request_headers,
        &id,
        crate::USER_ACTION_DRAIN_GRACE,
    )
    .await?;
    match crate::state_machine::transition_task_with_runtime_state(
        &app,
        &state.pool,
        &id,
        TaskStatus::Paused,
        0,
        0,
        Some("Paused"),
        Some("paused"),
        Some("Paused"),
        crate::models::SegmentStatus::Pending,
        None,
        None,
    )
    .await
    {
        Ok(_) => {}
        Err(TransitionError::Conflict { .. }) => {
            // ARC-30: a stable code, not free text — bulk statistics and the
            // frontend dispatch on the code, so message rewording is inert.
            return Err(AppErrorPayload::new(
                "task_state_changed",
                "Task state changed concurrently, please refresh.",
                false,
                Vec::new(),
            )
            .command_error());
        }
        Err(error) => return Err(error.into()),
    }
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_queue_changed_with_ids(&app, Some(vec![id.clone()]));
    // ARC-32: the per-task runtime lock is still held here; dispatch must not
    // be awaited under it (start_task re-acquires task runtime locks).
    state.scheduler.dispatch_detached(app, state.pool.clone());
    task_payload(&state.pool, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(&id).await;
    tracing::info!(task_id = %id, "resuming task");
    if state
        .downloads
        .lock()
        .await
        .get(&id)
        .is_some_and(|control| control.cancel_token.is_cancelled())
    {
        return Err(crate::task_stop_pending_error());
    }
    let task = require_task(&state.pool, &id).await?;
    if matches!(task.status, TaskStatus::Completed) {
        // ARC-30: stable code instead of free text (see pause_task).
        return Err(AppErrorPayload::new(
            "task_already_completed",
            "This download is already completed.",
            false,
            Vec::new(),
        )
        .command_error());
    }
    if matches!(task.status, TaskStatus::Failed | TaskStatus::NeedsAttention) {
        // Resume is only for paused/waiting tasks. Failed tasks must go through
        // retry or the explicit recovery playbook so bulk and keyboard paths
        // cannot silently bypass the row's safety decision.
        return Err(AppErrorPayload::new(
            "task_state_changed",
            "Task requires recovery before it can resume.",
            false,
            Vec::new(),
        )
        .command_error());
    }
    if !task.supports_resume
        || task_error_code(&task)
            .as_deref()
            .is_some_and(restart_required_error_code)
    {
        return Err(AppErrorPayload::new(
            "resume_unavailable",
            "The server no longer supports resuming.",
            true,
            vec!["restart", "open_folder"],
        )
        .command_error());
    }
    let task = queue_task_for_retry_with_event(&app, state.inner(), &id, "resumed", None).await?;
    task_from_record_with_files(&state.pool, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(&id).await;
    tracing::info!(task_id = %id, "retrying task");
    let task = require_task(&state.pool, &id).await?;
    if matches!(task.status, TaskStatus::Failed | TaskStatus::NeedsAttention)
        && task_error_code(&task)
            .as_deref()
            .is_some_and(restart_required_error_code)
    {
        return Err("This task must be restarted before it can continue safely.".to_string());
    }
    // ARC-57: drain (not detach) the worker so any in-flight checkpoint
    // commits before the retry transition, and no detached worker can write a
    // late `downloading` checkpoint over the new state or race the re-dispatch
    // as a second writer. Same pattern as pause/cancel and restart (ARC-45).
    crate::remove_and_drain_control(
        &state.downloads,
        &state.request_headers,
        &id,
        crate::USER_ACTION_DRAIN_GRACE,
    )
    .await?;

    let task = queue_task_for_retry_with_event(&app, state.inner(), &id, "retrying", None).await?;
    task_from_record_with_files(&state.pool, task).await
}

/// Create a fresh task from a completed task's persisted configuration. The
/// original task remains immutable so its output and completion evidence stay
/// available in history.
#[tauri::command]
#[specta::specta]
pub async fn redownload_task(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    redownload_task_inner(Some(app), state.inner(), &id).await
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub async fn redownload_task_headless(state: &AppState, id: &str) -> Result<Task, String> {
    redownload_task_inner(None, state, id).await
}

async fn redownload_task_inner(
    app: Option<AppHandle>,
    state: &AppState,
    id: &str,
) -> Result<Task, String> {
    let task = require_task(&state.pool, id).await?;
    if task.status != TaskStatus::Completed {
        return Err(AppErrorPayload::new(
            "task_not_completed",
            "Only completed tasks can be downloaded again.",
            false,
            Vec::new(),
        )
        .command_error());
    }

    let (request_profile, request_profile_sensitive_expires_at) =
        request_profile_for_redownload(&state.pool, &task.id).await?;
    let (browser_request_headers, source_browser, browser_header_expires_at) =
        db::resolve_task_request_headers_with_source(&state.pool, &task.id).await?;
    let credentials = db::resolve_task_credentials(&state.pool, &task.id).await?;
    let task_files = db::list_task_file_records(&state.pool, &task.id).await?;
    let proxy = db::get_task_proxy_settings(&state.pool, &task.id).await?;
    let (proxy_url, proxy_username, proxy_password, proxy_no_proxy) = if proxy.mode
        == TaskProxyMode::Custom
    {
        let global = state.engine_registry.proxy_config().await;
        let resolved =
            db::resolve_task_proxy_config(&state.pool, &task.id, &task.protocol, &global).await?;
        (
            resolved.url,
            resolved.username,
            resolved.password,
            resolved.no_proxy,
        )
    } else {
        (
            None,
            None,
            None,
            (!proxy.no_proxy.is_empty()).then_some(proxy.no_proxy.clone()),
        )
    };

    let policy = db::task_network_policy(&state.pool, &task.id).await?;
    // Legacy tasks may have no persisted policy (or the explicit public
    // placeholder from a revoked grant). Re-download must not manufacture an
    // empty authorization token: creation can derive the public policy from
    // the URL, while private targets still require their original bound grant.
    let authorization_id = if policy.root_authority.is_some() && !policy.grants.is_empty() {
        Some(db::create_network_authorization_for_policy(&state.pool, &policy).await?)
    } else {
        None
    };

    let checksums = db::list_task_checksum_records(&state.pool, &task.id).await?;
    let primary_checksum = checksums
        .iter()
        .filter(|checksum| checksum.file_id.is_none())
        .find(|checksum| checksum.is_primary)
        .or_else(|| checksums.iter().find(|checksum| checksum.file_id.is_none()));
    let expected_hash = primary_checksum
        .map(|checksum| checksum.expected_hash.clone())
        .or_else(|| task.expected_hash_sha256.clone());
    let expected_hash_algorithm = primary_checksum.map(|checksum| checksum.algorithm);

    let hls = if task.protocol == "hls" {
        db::get_hls_task(&state.pool, &task.id).await?
    } else {
        None
    };
    let selected_hls_audio_track_uris = hls
        .as_ref()
        .and_then(|record| record.selected_audio_track_uris.as_deref())
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok());
    let selected_hls_subtitle_track_uris = hls
        .as_ref()
        .and_then(|record| record.selected_subtitle_track_uris.as_deref())
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok());
    let selected_file_paths = (!task_files.is_empty()).then(|| {
        task_files
            .iter()
            .filter(|file| file.selected)
            .map(|file| file.relative_path.clone())
            .collect::<Vec<_>>()
    });

    let input = CreateTaskInput {
        url: task.url.clone(),
        request_profile,
        network_authorization_id: authorization_id,
        source_kind: Some(policy.source),
        save_dir: Some(task.save_dir.clone()),
        file_name: Some(task.file_name.clone()),
        start_paused: Some(false),
        obey_schedule: Some(task.obey_schedule),
        expected_hash_sha256: if expected_hash_algorithm.is_none() {
            expected_hash.clone()
        } else {
            None
        },
        expected_hash,
        expected_hash_algorithm,
        task_speed_limit_bps: task.task_speed_limit_bps.clone(),
        priority: Some(task.priority),
        category_key: task.category_key.clone(),
        probe_snapshot: None,
        selected_file_paths,
        allow_duplicate: Some(true),
        username: credentials.as_ref().map(|value| value.username.clone()),
        password: credentials.as_ref().map(|value| value.password.clone()),
        private_key_data: credentials
            .as_ref()
            .and_then(|value| value.private_key_data.clone()),
        private_key_passphrase: credentials
            .as_ref()
            .and_then(|value| value.private_key_passphrase.clone()),
        selected_hls_variant_uri: hls.as_ref().map(|record| record.media_url.clone()),
        selected_hls_audio_track_uris,
        selected_hls_subtitle_track_uris,
        proxy_mode: Some(proxy.mode),
        proxy_url,
        proxy_username,
        proxy_password,
        proxy_no_proxy,
    };

    match app {
        Some(app) => {
            if browser_request_headers.is_empty() && request_profile_sensitive_expires_at.is_none()
            {
                create_task_with_state(app, state, input).await
            } else {
                create_task_with_state_and_headers_until(
                    app,
                    state,
                    input,
                    browser_request_headers,
                    source_browser,
                    request_profile_sensitive_expires_at,
                    browser_header_expires_at,
                )
                .await
            }
        }
        None => {
            create_task_headless_with_headers_until(
                state,
                input,
                browser_request_headers,
                source_browser,
                request_profile_sensitive_expires_at,
                browser_header_expires_at,
            )
            .await
        }
    }
}

async fn request_profile_for_redownload(
    pool: &sqlx::SqlitePool,
    task_id: &str,
) -> Result<(Option<TaskRequestProfileInput>, Option<String>), String> {
    let headers = db::resolve_task_request_profile_headers(pool, task_id).await?;
    let profile_view = db::get_task_request_profile(pool, task_id).await?;
    if headers.is_empty() {
        return Ok((None, profile_view.sensitive_expires_at));
    }
    let mut profile = TaskRequestProfileInput {
        user_agent: None,
        referer: None,
        custom_headers: Vec::new(),
    };
    for (name, value) in headers {
        match name.to_ascii_lowercase().as_str() {
            "user-agent" => profile.user_agent = Some(value),
            "referer" => profile.referer = Some(value),
            _ => profile
                .custom_headers
                .push(TaskRequestHeaderInput { name, value }),
        }
    }
    Ok((Some(profile), profile_view.sensitive_expires_at))
}

#[tauri::command]
#[specta::specta]
pub async fn list_metalink_mirrors(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<MetalinkMirrorView>, String> {
    let records = db::list_metalink_resources_for_task(&state.pool, &id).await?;
    Ok(records
        .into_iter()
        .map(MetalinkMirrorView::from_record)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn retry_task_with_mirror(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    mirror_url: String,
) -> Result<Task, String> {
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(&id).await;
    tracing::info!(task_id = %id, mirror_url = %mirror_url, "retrying task with specific mirror");
    let task = require_task(&state.pool, &id).await?;
    if task.protocol != "metalink" {
        return Err("Mirror retry is only supported for Metalink tasks.".to_string());
    }
    // ARC-57: same drain (not detach) pattern as pause_task / retry_task.
    crate::remove_and_drain_control(
        &state.downloads,
        &state.request_headers,
        &id,
        crate::USER_ACTION_DRAIN_GRACE,
    )
    .await?;

    db::reset_metalink_resource_statuses(&state.pool, &id).await?;
    // Boost the chosen mirror's priority so the Metalink engine tries it first.
    db::promote_metalink_resource_for_retry(&state.pool, &id, &mirror_url).await?;
    let task = queue_task_for_retry_with_event(
        &app,
        state.inner(),
        &id,
        "retrying_with_mirror",
        Some(&mirror_url),
    )
    .await?;
    task_from_record_with_files(&state.pool, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn finish_live_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    tracing::info!(task_id = %id, "finishing HLS live recording");
    let task = require_task(&state.pool, &id).await?;
    if task.protocol != "hls" {
        return Err("Only HLS live recordings can be finished.".to_string());
    }
    let Some(hls_task) = db::get_hls_task(&state.pool, &id).await? else {
        return Err("HLS recording state is not ready yet.".to_string());
    };
    if hls_task.playlist_kind == "vod" {
        return Err("VOD HLS tasks finish automatically.".to_string());
    }
    if !matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err("The HLS recording must be downloading before it can be finished.".to_string());
    }
    db::request_hls_finish(&state.pool, &id).await?;
    if let Some(control) = state.downloads.lock().await.get(&id) {
        control.finish.store(true, Ordering::SeqCst);
        // PERF-15: wake the waiting HLS loop immediately — the DB flag is its
        // 2 s fallback, not the prompt path.
        control.finish_notify.notify_waiters();
    }
    db::update_task_health_summary(&state.pool, &id, Some("Finishing HLS recording")).await?;
    db::insert_task_event(&state.pool, &id, "hls_finish_requested", None).await?;
    let task = require_task(&state.pool, &id).await?;
    emit_task_updated_record(&app, &state.pool, &task).await;
    task_payload(&state.pool, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(&id).await;
    tracing::info!(task_id = %id, "canceling task");
    crate::remove_and_drain_control(
        &state.downloads,
        &state.request_headers,
        &id,
        crate::USER_ACTION_DRAIN_GRACE,
    )
    .await?;
    match crate::state_machine::transition_task_with_runtime_state(
        &app,
        &state.pool,
        &id,
        TaskStatus::Failed,
        0,
        0,
        Some("Canceled by user."),
        Some("failed"),
        Some("Canceled by user."),
        crate::models::SegmentStatus::Failed,
        Some("Canceled by user."),
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
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_queue_changed_with_ids(&app, Some(vec![id.clone()]));
    // ARC-32: the per-task runtime lock is still held here; dispatch must not
    // be awaited under it (start_task re-acquires task runtime locks).
    state.scheduler.dispatch_detached(app, state.pool.clone());
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
    let result = super::TaskDeletion {
        pool: &state.pool,
        downloads: &state.downloads,
        request_headers: &state.request_headers,
        runtime_locks: &state.task_runtime_locks,
        engines: &state.engine_registry,
        drain_grace: crate::USER_ACTION_DRAIN_GRACE,
    }
    .delete(&id, delete_file)
    .await;
    // Refresh even on partial cleanup failure: the task may now be paused.
    emit_queue_changed(&app);
    state.scheduler.dispatch_detached(app, state.pool.clone());
    result
}

#[tauri::command]
#[specta::specta]
pub async fn bulk_delete_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    delete_file: bool,
) -> Result<u32, String> {
    let result = super::TaskDeletion {
        pool: &state.pool,
        downloads: &state.downloads,
        request_headers: &state.request_headers,
        runtime_locks: &state.task_runtime_locks,
        engines: &state.engine_registry,
        drain_grace: crate::USER_ACTION_DRAIN_GRACE,
    }
    .delete_many(&ids, delete_file)
    .await;
    // Successful rows are already deleted even when another item failed.
    emit_queue_changed(&app);
    state.scheduler.dispatch_detached(app, state.pool.clone());
    result
}

/// Bulk apply a transfer action (pause/resume/retry) to multiple tasks in a
/// single IPC call. Returns the number of tasks successfully processed.
/// Individual task failures are logged and skipped; the call only fails if a
/// fatal error occurs before the loop.
#[tauri::command]
#[specta::specta]
pub async fn bulk_task_action(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    action: String,
) -> Result<u32, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    let normalized = action.trim().to_ascii_lowercase();
    tracing::info!(count = ids.len(), action = %normalized, "bulk task action");
    let mut succeeded: u32 = 0;
    for id in &ids {
        let result = match normalized.as_str() {
            "pause" => pause_task(app.clone(), state.clone(), id.clone()).await,
            "resume" => resume_task(app.clone(), state.clone(), id.clone()).await,
            "retry" => retry_task(app.clone(), state.clone(), id.clone()).await,
            other => {
                return Err(format!("Unknown bulk action: {other}"));
            }
        };
        match result {
            Ok(_) => succeeded += 1,
            Err(err) => {
                tracing::warn!(task_id = %id, error = %err, "bulk action failed for task");
            }
        }
    }
    Ok(succeeded)
}

/// UX-05: Pause or resume every matching task in the database, ignoring the
/// frontend's loaded page / search / filter. Returns succeeded/skipped/failed.
#[tauri::command]
#[specta::specta]
pub async fn bulk_task_action_global(
    app: AppHandle,
    state: State<'_, AppState>,
    action: String,
) -> Result<crate::models::BulkTaskActionResult, String> {
    let normalized = action.trim().to_ascii_lowercase();
    let statuses: &[&str] = match normalized.as_str() {
        "pause" => &["downloading", "retrying", "queued"],
        "resume" => &["paused", "waiting_network"],
        other => return Err(format!("Unknown global bulk action: {other}")),
    };
    let ids = db::list_task_ids_by_statuses(&state.pool, statuses).await?;
    tracing::info!(
        count = ids.len(),
        action = %normalized,
        "bulk task action global"
    );
    let mut succeeded: u32 = 0;
    let mut skipped: u32 = 0;
    let mut failed: u32 = 0;
    for id in &ids {
        // Re-check status so a raced transition counts as skipped, not failed.
        let current = db::get_task_record(&state.pool, id).await?;
        let Some(task) = current else {
            skipped += 1;
            continue;
        };
        let status = task.status.as_str();
        if !statuses.contains(&status) {
            skipped += 1;
            continue;
        }
        // Pausing would discard bytes when the source cannot resume; leave the
        // task running and report it as skipped so the global action is safe.
        if normalized == "pause" && pause_would_discard_progress(&task) {
            skipped += 1;
            continue;
        }
        let result = match normalized.as_str() {
            "pause" => pause_task(app.clone(), state.clone(), id.clone()).await,
            "resume" => resume_task(app.clone(), state.clone(), id.clone()).await,
            _ => unreachable!(),
        };
        match result {
            Ok(_) => succeeded += 1,
            Err(err) => {
                // ARC-30: skipped-vs-failed keys off the stable error code,
                // never message wording (ARC-16 established the same rule
                // for resume errors).
                if bulk_action_is_skippable(&err) {
                    tracing::info!(task_id = %id, error = %err, "bulk global action skipped");
                    skipped += 1;
                } else {
                    tracing::warn!(task_id = %id, error = %err, "bulk global action failed");
                    failed += 1;
                }
            }
        }
    }
    Ok(crate::models::BulkTaskActionResult {
        succeeded,
        skipped,
        failed,
    })
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
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(id).await;
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
            let task =
                queue_task_for_retry_with_event(&app, state.inner(), id, "retrying", None).await?;
            crate::commands::recovery::record_recovery_history(
                &state.pool,
                &task,
                input.action.as_str(),
                input.origin.as_deref().unwrap_or("manual"),
                error_code.as_deref(),
            )
            .await;
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
            let event_message = format!("Retry scheduled for {retry_after_at}.");
            let task = queue_task_for_retry_at(
                &app,
                state.inner(),
                id,
                Some(&retry_after_at),
                Some("retry_later"),
                Some(&event_message),
            )
            .await?;
            crate::commands::recovery::record_recovery_history(
                &state.pool,
                &task,
                input.action.as_str(),
                input.origin.as_deref().unwrap_or("manual"),
                error_code.as_deref(),
            )
            .await;
            state.scheduler.clone().spawn_dispatch_after(
                app.clone(),
                state.pool.clone(),
                std::time::Duration::from_secs(300),
            );
            task_from_record_with_files(&state.pool, task).await
        }
        RecoveryAction::ChooseAnotherName | RecoveryAction::ChooseAnotherFolder => {
            update_recovery_target(&app, state.inner(), &task, &input).await?;
            let task = queue_task_for_retry_with_event(
                &app,
                state.inner(),
                id,
                "retrying",
                Some("Recovery target changed."),
            )
            .await?;
            crate::commands::recovery::record_recovery_history(
                &state.pool,
                &task,
                input.action.as_str(),
                input.origin.as_deref().unwrap_or("manual"),
                error_code.as_deref(),
            )
            .await;
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
            crate::commands::recovery::record_recovery_history(
                &state.pool,
                &task,
                input.action.as_str(),
                input.origin.as_deref().unwrap_or("manual"),
                error_code.as_deref(),
            )
            .await;
            task_from_record_with_files(&state.pool, task).await
        }
        RecoveryAction::OpenFolder
        | RecoveryAction::CheckUrl
        | RecoveryAction::FreeDiskSpace
        | RecoveryAction::ConfigureFfmpeg
        | RecoveryAction::ManageSftpHostKeys => {
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

/// U09: menu-level integrity recheck reuses the existing checksum job and
/// returns the refreshed task so every task surface updates immediately.
#[tauri::command]
#[specta::specta]
pub async fn recheck_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    verify_task_hash_with_pool(&state.pool, &id).await?;
    let task = require_task(&state.pool, &id).await?;
    emit_task_updated_record(&app, &state.pool, &task).await;
    emit_queue_changed_with_ids(&app, Some(vec![id.clone()]));
    task_payload(&state.pool, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn compute_file_hash(
    state: State<'_, AppState>,
    id: String,
    algorithm: ChecksumAlgorithm,
) -> Result<String, String> {
    let task = require_task(&state.pool, &id).await?;
    let final_path = task
        .final_path
        .ok_or_else(|| "Downloaded file path is not available.".to_string())?;
    hash_file(&PathBuf::from(final_path), algorithm).await
}

pub(crate) async fn verify_task_hash_with_pool(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<HashVerificationState, String> {
    let task = require_task(pool, id).await?;
    let task_checksums = db::list_task_checksum_records(pool, id)
        .await?
        .into_iter()
        .filter(|checksum| checksum.file_id.is_none())
        .collect::<Vec<_>>();
    if !task_checksums.is_empty() {
        return verify_task_checksum_records(pool, task, task_checksums).await;
    }

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
    let actual = hash_file(&PathBuf::from(final_path), ChecksumAlgorithm::Sha256).await?;
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

async fn verify_task_checksum_records(
    pool: &sqlx::SqlitePool,
    task: crate::models::TaskRecord,
    checksums: Vec<crate::models::TaskChecksumRecord>,
) -> Result<HashVerificationState, String> {
    let Some(final_path) = task.final_path.as_deref() else {
        let message = "Downloaded file path is not available.";
        for checksum in &checksums {
            db::update_task_checksum_record(
                pool,
                &checksum.id,
                None,
                HashVerificationStatus::Failed,
                Some(message),
            )
            .await?;
        }
        db::update_hash_verification(
            pool,
            &task.id,
            None,
            HashVerificationStatus::Failed,
            Some(message),
        )
        .await?;
        db::insert_task_event(pool, &task.id, "hash_failed", Some(message)).await?;
        let updated = require_task(pool, &task.id).await?;
        return Ok(HashVerificationState {
            task_id: updated.id,
            expected_sha256: updated.expected_hash_sha256,
            actual_sha256: updated.actual_hash_sha256,
            status: updated.hash_status,
            error_message: updated.hash_error,
            verified_at: updated.hash_verified_at,
        });
    };

    db::update_hash_verification(pool, &task.id, None, HashVerificationStatus::Pending, None)
        .await?;

    let path = PathBuf::from(final_path);
    let mut computed = Vec::<(ChecksumAlgorithm, Result<String, String>)>::new();
    let mut actual_sha256 = None;
    let mut failures = Vec::new();

    for checksum in &checksums {
        let actual = if let Some((_, result)) = computed
            .iter()
            .find(|(algorithm, _)| *algorithm == checksum.algorithm)
        {
            result.clone()
        } else {
            let result = hash_file(&path, checksum.algorithm).await;
            computed.push((checksum.algorithm, result.clone()));
            result
        };

        match actual {
            Ok(actual) => {
                if checksum.algorithm == ChecksumAlgorithm::Sha256 {
                    actual_sha256 = Some(actual.clone());
                }
                let verified = actual.eq_ignore_ascii_case(&checksum.expected_hash);
                let status = if verified {
                    HashVerificationStatus::Verified
                } else {
                    HashVerificationStatus::Failed
                };
                let error = (!verified).then(|| {
                    format!(
                        "{} checksum does not match.",
                        checksum_algorithm_label(checksum.algorithm)
                    )
                });
                if let Some(error) = error.as_ref() {
                    failures.push(error.clone());
                }
                db::update_task_checksum_record(
                    pool,
                    &checksum.id,
                    Some(&actual),
                    status,
                    error.as_deref(),
                )
                .await?;
            }
            Err(error) => {
                failures.push(error.clone());
                db::update_task_checksum_record(
                    pool,
                    &checksum.id,
                    None,
                    HashVerificationStatus::Failed,
                    Some(&error),
                )
                .await?;
            }
        }
    }

    let status = if failures.is_empty() {
        HashVerificationStatus::Verified
    } else {
        HashVerificationStatus::Failed
    };
    let error_message = (!failures.is_empty()).then(|| failures.join(" "));
    db::update_hash_verification(
        pool,
        &task.id,
        actual_sha256.as_deref(),
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

fn checksum_algorithm_label(algorithm: ChecksumAlgorithm) -> &'static str {
    match algorithm {
        ChecksumAlgorithm::Sha256 => "SHA-256",
        ChecksumAlgorithm::Sha512 => "SHA-512",
        ChecksumAlgorithm::Sha1 => "SHA-1",
        ChecksumAlgorithm::Md5 => "MD5",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn checksum_test_pool(label: &str) -> (sqlx::SqlitePool, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("vibe-checksum-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create checksum fixture directory");
        let payload_path = root.join("payload.bin");
        std::fs::write(&payload_path, b"vibe checksum protocol fixture")
            .expect("write checksum fixture");
        let pool = db::connect(&root.join("test.sqlite"))
            .await
            .expect("connect checksum database")
            .pool;
        (pool, root, payload_path)
    }

    fn checksum_task(id: &str, protocol: &str, final_path: &Path) -> crate::models::TaskRecord {
        let now = now_iso();
        crate::models::TaskRecord {
            id: id.to_string(),
            url: format!("{protocol}://example.com/payload.bin"),
            final_url: Some(format!("{protocol}://example.com/payload.bin")),
            protocol: protocol.to_string(),
            task_kind: crate::models::TaskKind::SingleFile,
            file_name: "payload.bin".to_string(),
            save_dir: final_path
                .parent()
                .expect("fixture parent")
                .to_string_lossy()
                .to_string(),
            temp_path: None,
            final_path: Some(final_path.to_string_lossy().to_string()),
            total_size: i64::try_from(
                std::fs::metadata(final_path)
                    .expect("fixture metadata")
                    .len(),
            )
            .expect("fixture size"),
            downloaded_bytes: 0,
            status: TaskStatus::Completed,
            etag: None,
            last_modified: None,
            content_type: Some("application/octet-stream".to_string()),
            supports_resume: true,
            supports_parallel: false,
            supports_multi_file: false,
            source_key: format!("{protocol}://example.com"),
            connection_count: 0,
            speed_bps: 0,
            task_speed_limit_bps: None,
            priority: TaskPriority::Normal,
            queue_position: 0,
            category_key: None,
            obey_schedule: true,
            health_summary: Some("Completed".to_string()),
            error_message: None,
            error_code: None,
            recovery_actions: Vec::new(),
            retry_after_at: None,
            expected_hash_sha256: None,
            actual_hash_sha256: None,
            hash_status: HashVerificationStatus::Pending,
            hash_error: None,
            hash_verified_at: None,
            created_at: now.clone(),
            updated_at: now,
            files_version: 0,
        }
    }

    fn checksum_record(
        task: &crate::models::TaskRecord,
        algorithm: ChecksumAlgorithm,
        expected_hash: String,
    ) -> crate::models::TaskChecksumRecord {
        crate::models::TaskChecksumRecord {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            file_id: None,
            algorithm,
            expected_hash,
            actual_hash: None,
            status: HashVerificationStatus::Pending,
            source_kind: "manual".to_string(),
            source_url: None,
            source_label: None,
            is_primary: true,
            weak: algorithm.is_weak(),
            error_message: None,
            discovered_at: None,
            verified_at: None,
            created_at: task.created_at.clone(),
            updated_at: task.updated_at.clone(),
        }
    }

    #[test]
    fn global_pause_marks_only_written_non_resumable_tasks_as_skipped() {
        let root = std::env::temp_dir().join(format!("vibe-pause-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        let path = root.join("payload.bin");
        std::fs::write(&path, b"fixture").expect("write fixture");
        let mut task = checksum_task("pause-check", "http", &path);

        task.downloaded_bytes = 1;
        task.supports_resume = false;
        assert!(pause_would_discard_progress(&task));

        task.supports_resume = true;
        assert!(!pause_would_discard_progress(&task));
        task.supports_resume = false;
        task.downloaded_bytes = 0;
        assert!(!pause_would_discard_progress(&task));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn file_deletes_run_off_runtime_and_deduplicate_paths() {
        let root = std::env::temp_dir().join(format!("vibe-delete-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        let first = root.join("first.part");
        let second = root.join("second.part");
        std::fs::write(&first, b"first").expect("write first fixture");
        std::fs::write(&second, b"second").expect("write second fixture");

        let requests = vec![
            FileDeleteRequest {
                path: first.to_string_lossy().to_string(),
                use_trash: false,
            },
            FileDeleteRequest {
                path: first.to_string_lossy().to_string(),
                use_trash: false,
            },
            FileDeleteRequest {
                path: second.to_string_lossy().to_string(),
                use_trash: false,
            },
        ];
        let outcomes = delete_paths_off_runtime(requests).await;

        assert!(
            outcomes.iter().all(|outcome| outcome.result.is_ok()),
            "unexpected delete failures: {outcomes:?}"
        );
        assert!(!first.exists());
        assert!(!second.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn non_http_single_file_protocols_verify_non_sha256_checksums() {
        let (pool, root, payload_path) = checksum_test_pool("protocols").await;
        let protocols = [
            ("ftp", ChecksumAlgorithm::Sha512),
            ("sftp", ChecksumAlgorithm::Sha1),
            ("webdav", ChecksumAlgorithm::Md5),
        ];

        for (protocol, algorithm) in protocols {
            let task = checksum_task(&format!("{protocol}-checksum"), protocol, &payload_path);
            db::insert_task_record(&pool, &task)
                .await
                .expect("insert checksum task");
            let expected = hash_file(&payload_path, algorithm)
                .await
                .expect("compute expected checksum");
            db::insert_task_checksum_record(
                &pool,
                &checksum_record(&task, algorithm, expected.clone()),
            )
            .await
            .expect("insert checksum record");

            let state = verify_task_hash_with_pool(&pool, &task.id)
                .await
                .expect("verify task checksums");
            assert_eq!(
                state.status,
                HashVerificationStatus::Verified,
                "unexpected verification state for {protocol}"
            );
            assert!(state.expected_sha256.is_none());

            let records = db::list_task_checksum_records(&pool, &task.id)
                .await
                .expect("list checksum records");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].status, HashVerificationStatus::Verified);
            assert_eq!(records[0].actual_hash.as_deref(), Some(expected.as_str()));
            assert!(records[0].verified_at.is_some());
        }

        pool.close().await;
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn checksum_mismatch_updates_record_and_task_summary() {
        let (pool, root, payload_path) = checksum_test_pool("mismatch").await;
        let task = checksum_task("ftp-checksum-mismatch", "ftp", &payload_path);
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert checksum task");
        db::insert_task_checksum_record(
            &pool,
            &checksum_record(&task, ChecksumAlgorithm::Md5, "0".repeat(32)),
        )
        .await
        .expect("insert checksum record");

        let state = verify_task_hash_with_pool(&pool, &task.id)
            .await
            .expect("verify mismatched checksum");
        assert_eq!(state.status, HashVerificationStatus::Failed);
        assert_eq!(
            state.error_message.as_deref(),
            Some("MD5 checksum does not match.")
        );

        let records = db::list_task_checksum_records(&pool, &task.id)
            .await
            .expect("list checksum records");
        assert_eq!(records[0].status, HashVerificationStatus::Failed);
        assert!(records[0].actual_hash.is_some());
        assert_eq!(
            records[0].error_message.as_deref(),
            Some("MD5 checksum does not match.")
        );

        pool.close().await;
        let _ = std::fs::remove_dir_all(root);
    }
}

/// ARC-30: a raced transition or an already-completed resume is a *skip*
/// (the task was not in an actionable state), not a failure of the bulk
/// operation. Only structured `AppErrorPayload` codes decide this; plain
/// legacy strings are failures.
fn bulk_action_is_skippable(error: &str) -> bool {
    let payload = serde_json::from_str::<AppErrorPayload>(error).ok();
    matches!(
        payload.as_ref().map(|p| p.code.as_str()),
        Some("task_state_changed") | Some("task_already_completed")
    )
}

#[cfg(test)]
mod arc30_tests {
    use super::bulk_action_is_skippable;
    use crate::models::AppErrorPayload;

    fn payload(code: &str, message: &str) -> String {
        AppErrorPayload::new(code, message, false, Vec::new()).command_error()
    }

    /// ARC-30: changing any error wording must not affect bulk statistics.
    #[test]
    fn skippable_codes_dispatch_on_code_not_message_text() {
        assert!(bulk_action_is_skippable(&payload(
            "task_state_changed",
            "totally reworded phrasing"
        )));
        assert!(bulk_action_is_skippable(&payload(
            "task_already_completed",
            "another arbitrary wording"
        )));
    }

    #[test]
    fn unknown_codes_and_plain_strings_count_as_failures() {
        assert!(!bulk_action_is_skippable(&payload(
            "hls_segment_failed",
            "unrelated failure"
        )));
        // Pre-ARC-30 plain-text forms of the same situations are no longer
        // classified — the structured payload is the single contract.
        assert!(!bulk_action_is_skippable(
            "Task state changed concurrently, please refresh."
        ));
        assert!(!bulk_action_is_skippable(
            "This download is already completed."
        ));
        assert!(!bulk_action_is_skippable("not json at all"));
    }
}
