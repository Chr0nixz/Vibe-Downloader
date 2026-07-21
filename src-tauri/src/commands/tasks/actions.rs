use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    db,
    download::checksum::hash_file,
    events::{
        emit_queue_changed, emit_queue_changed_with_ids, emit_task_updated_record,
        evict_task_files_version, evict_task_files_versions,
    },
    models::{
        task::now_iso, ChecksumAlgorithm, HashVerificationState, HashVerificationStatus,
        RecoveryAction, Task, TaskPriority, TaskStatus,
    },
    platform,
    state_machine::TransitionError,
    AppState,
};

use super::{
    delete_path, emit_task_progress_snapshot, is_bt_protocol, queue_task_for_retry_at,
    queue_task_for_retry_with_event, require_task, restart_required_error_code,
    restart_task_from_beginning, task_error_code, task_from_record_with_files, task_payload,
    update_recovery_target, ResolveTaskAttentionInput,
};

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
const MAX_CONCURRENT_FILE_DELETES: usize = 4;

#[derive(Debug)]
struct FileDeleteRequest {
    path: String,
    use_trash: bool,
}

async fn delete_paths_off_runtime(requests: Vec<FileDeleteRequest>) -> Vec<String> {
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
    let mut warnings = Vec::new();
    while let Some(outcome) = pending.next().await {
        match outcome {
            Ok((_, Ok(()))) => {}
            Ok((_, Err(error))) => warnings.push(error),
            Err(error) => warnings.push(format!("File deletion worker failed: {error}")),
        }
    }
    warnings
}

fn push_delete_request(requests: &mut Vec<FileDeleteRequest>, path: Option<&str>, use_trash: bool) {
    if let Some(path) = path.filter(|path| !path.trim().is_empty()) {
        requests.push(FileDeleteRequest {
            path: path.to_string(),
            use_trash,
        });
    }
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
    let task_for_runtime = db::get_task_record(&state.pool, &id).await?;
    if task_for_runtime
        .as_ref()
        .is_some_and(|task| is_bt_protocol(&task.protocol))
    {
        if let Some(control) = state.downloads.lock().await.remove(&id) {
            control.cancel_token.cancel();
            if let Some(h) = control.handle.as_ref() {
                h.abort();
            }
        }
        if let Some(task) = task_for_runtime.as_ref() {
            state.engine_registry.delete_runtime_task(task, false).await;
        }
        let _ = state.request_headers.lock().await.remove(&id);
    } else {
        // Cancel the download and wait for the coordinator to finish its
        // checkpoint flush before transitioning the task state.
        //
        // Without this wait, the coordinator's checkpoint write and
        // transition_task can race. ARC-06 uses BEGIN IMMEDIATE + BUSY
        // retries on transitions; draining the JoinHandle remains defense
        // in depth so the checkpoint commit finishes first.
        //
        // The 5s timeout bounds the wait for slow disks; if it expires,
        // transition_task proceeds anyway and relies on IMMEDIATE/retry.
        let handle = {
            let mut downloads = state.downloads.lock().await;
            if let Some(control) = downloads.remove(&id) {
                control.cancel_token.cancel();
                control.handle
            } else {
                None
            }
        };
        if let Some(handle) = handle {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
    }
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
            return Err("Task state changed concurrently, please refresh.".to_string());
        }
        Err(error) => return Err(error.into()),
    }
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_queue_changed_with_ids(&app, Some(vec![id.clone()]));
    state
        .scheduler
        .clone()
        .dispatch(app, state.pool.clone())
        .await;
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
    let task = require_task(&state.pool, &id).await?;
    if matches!(task.status, TaskStatus::Completed) {
        return Err("This download is already completed.".to_string());
    }
    if matches!(task.status, TaskStatus::NeedsAttention) {
        return Err("Remote file changed. Restart download to avoid corruption.".to_string());
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
    if task.status == TaskStatus::NeedsAttention
        && task_error_code(&task)
            .as_deref()
            .is_some_and(restart_required_error_code)
    {
        return Err("This task must be restarted before it can continue safely.".to_string());
    }
    // ARC-06: Align with pause/cancel — drain the worker JoinHandle so any
    // in-flight checkpoint commits before the retry transition (defense in
    // depth alongside BEGIN IMMEDIATE).
    let handle = {
        let mut downloads = state.downloads.lock().await;
        if let Some(control) = downloads.remove(&id) {
            control.cancel_token.cancel();
            control.handle
        } else {
            None
        }
    };
    if let Some(handle) = handle {
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }

    let task = queue_task_for_retry_with_event(&app, state.inner(), &id, "retrying", None).await?;
    task_from_record_with_files(&state.pool, task).await
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
    // ARC-06: Same checkpoint-drain pattern as pause_task / retry_task.
    let handle = {
        let mut downloads = state.downloads.lock().await;
        if let Some(control) = downloads.remove(&id) {
            control.cancel_token.cancel();
            control.handle
        } else {
            None
        }
    };
    if let Some(handle) = handle {
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }

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
    let task_for_runtime = db::get_task_record(&state.pool, &id).await?;
    if task_for_runtime
        .as_ref()
        .is_some_and(|task| is_bt_protocol(&task.protocol))
    {
        if let Some(control) = state.downloads.lock().await.remove(&id) {
            control.cancel_token.cancel();
            if let Some(h) = control.handle.as_ref() {
                h.abort();
            }
        }
        if let Some(task) = task_for_runtime.as_ref() {
            state.engine_registry.delete_runtime_task(task, false).await;
        }
        let _ = state.request_headers.lock().await.remove(&id);
    } else {
        // Same checkpoint-drain pattern as pause_task: cancel and wait for
        // the coordinator's checkpoint flush to commit before transition_task
        // begins. ARC-06 IMMEDIATE + retry covers residual BUSY; drain remains
        // defense in depth.
        let handle = {
            let mut downloads = state.downloads.lock().await;
            if let Some(control) = downloads.remove(&id) {
                control.cancel_token.cancel();
                control.handle
            } else {
                None
            }
        };
        if let Some(handle) = handle {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
    }
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
    state
        .scheduler
        .clone()
        .dispatch(app, state.pool.clone())
        .await;
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
    // R-2.3: Serialize against start_task and other user actions on the same task.
    let _guard = state.task_runtime_locks.lock(&id).await;
    tracing::info!(task_id = %id, delete_file, "deleting task");
    let task_for_runtime = db::get_task_record(&state.pool, &id).await?;
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel_token.cancel();
        if task_for_runtime
            .as_ref()
            .is_none_or(|task| !is_bt_protocol(&task.protocol))
        {
            if let Some(h) = control.handle.as_ref() {
                h.abort();
            }
        }
    }
    if let Some(task) = task_for_runtime.as_ref() {
        state
            .engine_registry
            .delete_runtime_task(task, delete_file)
            .await;
    }

    if delete_file {
        let use_trash = db::delete_to_trash_enabled(&state.pool)
            .await
            .unwrap_or(true);
        let mut delete_requests = Vec::new();
        if let Some(task) = task_for_runtime.as_ref() {
            for file in db::list_task_file_records(&state.pool, &id).await? {
                push_delete_request(&mut delete_requests, file.temp_path.as_deref(), false);
                push_delete_request(&mut delete_requests, file.final_path.as_deref(), use_trash);
            }
            push_delete_request(&mut delete_requests, task.temp_path.as_deref(), false);
            push_delete_request(&mut delete_requests, task.final_path.as_deref(), use_trash);
        }
        let file_warnings = delete_paths_off_runtime(delete_requests).await;
        for warning in &file_warnings {
            tracing::warn!(task_id = %id, warning, "file deletion warning during task removal");
        }
    }

    db::delete_task_record(&state.pool, &id).await?;
    evict_task_files_version(&id);
    emit_queue_changed(&app);
    state
        .scheduler
        .clone()
        .dispatch(app, state.pool.clone())
        .await;
    // R-2.5: Evict the lock entry now that the task is deleted and the guard
    // is about to drop. drop(_guard) first so evict sees Arc strong_count == 1.
    drop(_guard);
    state.task_runtime_locks.evict(&id).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn bulk_delete_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    delete_file: bool,
) -> Result<u32, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    tracing::info!(count = ids.len(), delete_file, "bulk deleting tasks");
    let use_trash = if delete_file {
        db::delete_to_trash_enabled(&state.pool)
            .await
            .unwrap_or(true)
    } else {
        false
    };

    // Phase 1: Cancel active downloads and clean up runtime state.
    let mut delete_requests = Vec::new();
    for id in &ids {
        // R-2.3: Per-task lock for the duration of this iteration.
        let _guard = state.task_runtime_locks.lock(id).await;
        let task_for_runtime = db::get_task_record(&state.pool, id).await?;
        if let Some(control) = state.downloads.lock().await.remove(id) {
            control.cancel_token.cancel();
            if task_for_runtime
                .as_ref()
                .is_none_or(|task| !is_bt_protocol(&task.protocol))
            {
                if let Some(h) = control.handle.as_ref() {
                    h.abort();
                }
            }
        }
        if let Some(task) = task_for_runtime.as_ref() {
            state
                .engine_registry
                .delete_runtime_task(task, delete_file)
                .await;
        }

        // Phase 2: Delete files (best-effort, filesystem ops cannot be transactional).
        if delete_file {
            if let Some(task) = task_for_runtime.as_ref() {
                for file in db::list_task_file_records(&state.pool, id).await? {
                    push_delete_request(&mut delete_requests, file.temp_path.as_deref(), false);
                    push_delete_request(
                        &mut delete_requests,
                        file.final_path.as_deref(),
                        use_trash,
                    );
                }
                push_delete_request(&mut delete_requests, task.temp_path.as_deref(), false);
                push_delete_request(&mut delete_requests, task.final_path.as_deref(), use_trash);
            }
        }
        // R-2.5: Evict the lock entry for this deleted task.
        drop(_guard);
        state.task_runtime_locks.evict(id).await;
    }
    let file_warnings = delete_paths_off_runtime(delete_requests).await;
    for warning in &file_warnings {
        tracing::warn!(warning, "file deletion warning during bulk delete");
    }

    // Phase 3: Delete all DB records in a single transaction.
    db::delete_task_records_batch(&state.pool, &ids).await?;
    evict_task_files_versions(ids.iter().map(String::as_str));

    emit_queue_changed(&app);
    state
        .scheduler
        .clone()
        .dispatch(app, state.pool.clone())
        .await;
    Ok(ids.len() as u32)
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
        "resume" => &["paused", "failed", "waiting_network"],
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
        let result = match normalized.as_str() {
            "pause" => pause_task(app.clone(), state.clone(), id.clone()).await,
            "resume" => resume_task(app.clone(), state.clone(), id.clone()).await,
            _ => unreachable!(),
        };
        match result {
            Ok(_) => succeeded += 1,
            Err(err) => {
                let lower = err.to_ascii_lowercase();
                if lower.contains("concurrently") || lower.contains("already") {
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
        let warnings = delete_paths_off_runtime(requests).await;

        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
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
