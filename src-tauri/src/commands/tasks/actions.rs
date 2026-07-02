use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    db,
    download::checksum::hash_file,
    events::{emit_queue_changed, emit_queue_changed_with_ids, emit_task_updated_record},
    models::{
        task::now_iso, ChecksumAlgorithm, HashVerificationState, HashVerificationStatus,
        RecoveryAction, Task, TaskPriority, TaskStatus,
    },
    platform, state_machine::TransitionError, AppState,
};

use super::{
    delete_path, emit_task_progress_snapshot, is_bt_protocol, queue_task_for_retry,
    queue_task_for_retry_at, require_task, restart_required_error_code,
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

    db::update_task_transfer_options(
        &state.pool,
        &input.id,
        db::TaskTransferOptionsUpdate {
            task_speed_limit_bps,
            priority,
            queue_position,
            category_key,
        },
    )
    .await?;
    let updated = require_task(&state.pool, &input.id).await?;
    emit_task_updated_record(&app, &state.pool, &updated).await;
    emit_queue_changed_with_ids(&app, Some(vec![updated.id.clone()]));
    if matches!(updated.status, TaskStatus::Queued) {
        state.scheduler.clone().dispatch(app.clone(), state.pool.clone()).await;
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
    state.scheduler.clone().dispatch(app, state.pool.clone()).await;
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
    } else if let Some(control) = state.downloads.lock().await.get(&id) {
        control.cancel_token.cancel();
    }
    match crate::state_machine::transition_task(
        &app,
        &state.pool,
        &id,
        TaskStatus::Paused,
        0,
        0,
        Some("Paused"),
        Some("paused"),
    )
    .await
    {
        Ok(_) => {}
        Err(TransitionError::Conflict { .. }) => {
            return Err("Task state changed concurrently, please refresh.".to_string());
        }
        Err(error) => return Err(error.into()),
    }
    db::update_segments_status_for_task(
        &state.pool,
        &id,
        crate::models::SegmentStatus::Pending,
        None,
    )
    .await?;
    let task = require_task(&state.pool, &id).await?;
    emit_task_progress_snapshot(&app, &task);
    emit_queue_changed_with_ids(&app, Some(vec![id.clone()]));
    state.scheduler.clone().dispatch(app, state.pool.clone()).await;
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
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel_token.cancel();
        if let Some(h) = control.handle.as_ref() {
                h.abort();
            }
    }

    db::insert_task_event(&state.pool, &id, "retrying", None).await?;
    let task = queue_task_for_retry(&app, state.inner(), &id).await?;
    task_from_record_with_files(&state.pool, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_metalink_mirrors(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<MetalinkMirrorView>, String> {
    let records = db::list_metalink_resources_for_task(&state.pool, &id).await?;
    Ok(records.into_iter().map(MetalinkMirrorView::from_record).collect())
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
    if let Some(control) = state.downloads.lock().await.remove(&id) {
        control.cancel_token.cancel();
        if let Some(h) = control.handle.as_ref() {
                h.abort();
            }
    }

    db::reset_metalink_resource_statuses(&state.pool, &id).await?;
    // Boost the chosen mirror's priority so the Metalink engine tries it first.
    db::promote_metalink_resource_for_retry(&state.pool, &id, &mirror_url).await?;
    db::insert_task_event(
        &state.pool,
        &id,
        "retrying_with_mirror",
        Some(&mirror_url),
    )
    .await?;
    let task = queue_task_for_retry(&app, state.inner(), &id).await?;
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
    } else if let Some(control) = state.downloads.lock().await.get(&id) {
        control.cancel_token.cancel();
    }
    match crate::state_machine::transition_task(
        &app,
        &state.pool,
        &id,
        TaskStatus::Failed,
        0,
        0,
        Some("Canceled by user."),
        Some("failed"),
    )
    .await
    {
        Ok(_) => {}
        Err(TransitionError::Conflict { .. }) => {
            return Err("Task state changed concurrently, please refresh.".to_string());
        }
        Err(error) => return Err(error.into()),
    }
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
    emit_queue_changed_with_ids(&app, Some(vec![id.clone()]));
    state.scheduler.clone().dispatch(app, state.pool.clone()).await;
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
        let use_trash = db::delete_to_trash_enabled(&state.pool).await.unwrap_or(true);
        let mut file_warnings: Vec<String> = Vec::new();
        if let Some(task) = task_for_runtime {
            for file in db::list_task_file_records(&state.pool, &id).await? {
                // temp files always permanent; final files respect trash setting
                if let Some(ref p) = file.temp_path {
                    if let Err(e) = delete_path(p, false) {
                        file_warnings.push(e);
                    }
                }
                if let Some(ref p) = file.final_path {
                    if let Err(e) = delete_path(p, use_trash) {
                        file_warnings.push(e);
                    }
                }
            }
            if let Some(ref p) = task.temp_path {
                if let Err(e) = delete_path(p, false) {
                    file_warnings.push(e);
                }
            }
            if let Some(ref p) = task.final_path {
                if let Err(e) = delete_path(p, use_trash) {
                    file_warnings.push(e);
                }
            }
        }
        for warning in &file_warnings {
            tracing::warn!(task_id = %id, warning, "file deletion warning during task removal");
        }
    }

    db::delete_task_record(&state.pool, &id).await?;
    emit_queue_changed(&app);
    state.scheduler.clone().dispatch(app, state.pool.clone()).await;
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
        db::delete_to_trash_enabled(&state.pool).await.unwrap_or(true)
    } else {
        false
    };

    // Phase 1: Cancel active downloads and clean up runtime state.
    let mut file_warnings: Vec<String> = Vec::new();
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
            if let Some(task) = task_for_runtime {
                for file in db::list_task_file_records(&state.pool, id).await? {
                    if let Some(ref p) = file.temp_path {
                        if let Err(e) = delete_path(p, false) {
                            file_warnings.push(e);
                        }
                    }
                    if let Some(ref p) = file.final_path {
                        if let Err(e) = delete_path(p, use_trash) {
                            file_warnings.push(e);
                        }
                    }
                }
                if let Some(ref p) = task.temp_path {
                    if let Err(e) = delete_path(p, false) {
                        file_warnings.push(e);
                    }
                }
                if let Some(ref p) = task.final_path {
                    if let Err(e) = delete_path(p, use_trash) {
                        file_warnings.push(e);
                    }
                }
            }
        }
        // R-2.5: Evict the lock entry for this deleted task.
        drop(_guard);
        state.task_runtime_locks.evict(id).await;
    }
    for warning in &file_warnings {
        tracing::warn!(warning, "file deletion warning during bulk delete");
    }

    // Phase 3: Delete all DB records in a single transaction.
    db::delete_task_records_batch(&state.pool, &ids).await?;

    emit_queue_changed(&app);
    state.scheduler.clone().dispatch(app, state.pool.clone()).await;
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
            state.scheduler.clone().spawn_dispatch_after(
                app.clone(),
                state.pool.clone(),
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
        RecoveryAction::OpenFolder
        | RecoveryAction::CheckUrl
        | RecoveryAction::FreeDiskSpace
        | RecoveryAction::ConfigureFfmpeg => {
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

