use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, State};
use tokio::io::AsyncReadExt;

use crate::{
    db,
    events::{emit_queue_changed, emit_task_updated_record},
    models::{
        task::now_iso, HashVerificationState, HashVerificationStatus, RecoveryAction, Task,
        TaskStatus,
    },
    platform, AppState,
};

use super::{
    emit_task_progress_snapshot, is_bt_protocol, queue_task_for_retry, queue_task_for_retry_at,
    require_task, restart_required_error_code, restart_task_from_beginning, schedule_queued_tasks,
    spawn_schedule_queued_tasks_after, task_error_code, task_from_record_with_files, task_payload,
    update_recovery_target, ResolveTaskAttentionInput,
};

#[tauri::command]
#[specta::specta]
pub async fn pause_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Task, String> {
    tracing::info!(task_id = %id, "pausing task");
    let task_for_runtime = db::get_task_record(&state.pool, &id).await?;
    if task_for_runtime
        .as_ref()
        .is_some_and(|task| is_bt_protocol(&task.protocol))
    {
        if let Some(control) = state.downloads.lock().await.remove(&id) {
            control.cancel.store(true, Ordering::SeqCst);
            control.handle.abort();
        }
        if let Some(task) = task_for_runtime.as_ref() {
            state.engine_registry.delete_runtime_task(task, false).await;
        }
        let _ = state.request_headers.lock().await.remove(&id);
    } else if let Some(control) = state.downloads.lock().await.get(&id) {
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
    tracing::info!(task_id = %id, "canceling task");
    let task_for_runtime = db::get_task_record(&state.pool, &id).await?;
    if task_for_runtime
        .as_ref()
        .is_some_and(|task| is_bt_protocol(&task.protocol))
    {
        if let Some(control) = state.downloads.lock().await.remove(&id) {
            control.cancel.store(true, Ordering::SeqCst);
            control.handle.abort();
        }
        if let Some(task) = task_for_runtime.as_ref() {
            state.engine_registry.delete_runtime_task(task, false).await;
        }
        let _ = state.request_headers.lock().await.remove(&id);
    } else if let Some(control) = state.downloads.lock().await.get(&id) {
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
    if std::fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        return match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not delete {path}: {error}")),
        };
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not delete {path}: {error}")),
    }
}
