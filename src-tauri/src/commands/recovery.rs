//! Recovery Center commands (feature proposal §3.3).
//!
//! Batch and shared-problem recovery over failed / needs-attention tasks.
//! Every state change re-validates the task behind the per-task runtime lock
//! (R-2.3), and state-changing resolutions record a durable `recovery_history`
//! row so the page can show what was resolved, when, and from which surface.

use tauri::{AppHandle, State};

use crate::commands::tasks::{
    queue_task_for_retry_at, queue_task_for_retry_with_event, restart_required_error_code,
    task_from_record_with_files,
};
use crate::db;
use crate::events;
use crate::models::recovery::{
    BulkRecoveryAction, BulkRecoveryResult, RecoveryHistoryRecord, UpdateTaskCredentialsInput,
};
use crate::models::{task::now_iso, Task, TaskRecord, TaskStatus};
use crate::AppState;

/// Protocols whose stored credentials the Recovery Center can replace. This
/// mirrors the set of engines that consume `task_credentials` at runtime:
/// HTTP Basic Auth covers http/https plus the derived hls/dash/metalink
/// engines, while bt/magnet have no credential channel at all.
const CREDENTIAL_PROTOCOLS: [&str; 10] = [
    "ftp", "ftps", "sftp", "webdav", "webdavs", "http", "https", "hls", "dash", "metalink",
];

/// Upper bound for the free-form recovery-history `source` field (the only
/// caller-controlled string stored in the log), so a caller cannot plant an
/// unbounded value in the database.
const MAX_SOURCE_LEN: usize = 32;

/// Pure gating decision for a bulk resolution: which failed/needs-attention
/// tasks the batch may touch. Exposed for tests — the IPC layer stays thin.
pub fn bulk_resolution_gate(status: &TaskStatus, error_code: Option<&str>) -> BulkGate {
    if !matches!(status, TaskStatus::Failed | TaskStatus::NeedsAttention) {
        return BulkGate::Skip;
    }
    let Some(code) = error_code else {
        return BulkGate::Proceed;
    };
    if restart_required_error_code(code) {
        // Restart-required failures need the destructive per-task playbook
        // (delete temp + re-probe), not a blind re-queue onto changed bytes.
        return BulkGate::Skip;
    }
    if code == "final_path_conflict" {
        // Publish never clobbers or auto-renames, so the conflict persists
        // until the user renames the target or removes the file — a bulk
        // re-queue just re-fails at publish and spams the recovery log.
        // Single-task retry stays available (the user may have cleared the
        // path first).
        return BulkGate::Skip;
    }
    BulkGate::Proceed
}

/// Clamp the caller-supplied history source to a bounded, single-line value.
pub fn normalize_recovery_source(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return "manual".to_string();
    }
    trimmed.chars().take(MAX_SOURCE_LEN).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkGate {
    Proceed,
    Skip,
}

/// Pure gating decision for a credential update. Exposed for tests.
pub fn credentials_update_gate(
    protocol: &str,
    status: &TaskStatus,
) -> Result<(), CredentialsGateError> {
    if !CREDENTIAL_PROTOCOLS.contains(&protocol) {
        return Err(CredentialsGateError::UnsupportedProtocol);
    }
    if matches!(status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err(CredentialsGateError::TaskBusy);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialsGateError {
    UnsupportedProtocol,
    TaskBusy,
}

/// Persist one recovery-history row. Best-effort by design: the state change
/// already succeeded, so a history failure must not fail the resolution
/// outcome — it is logged for diagnostics instead.
pub(crate) async fn record_recovery_history(
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
    action: &str,
    source: &str,
    error_code: Option<&str>,
) {
    let record = RecoveryHistoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        task_file_name: Some(task.file_name.clone()),
        action: action.to_string(),
        source: normalize_recovery_source(source),
        error_code: error_code.map(str::to_string),
        created_at: now_iso(),
    };
    if let Err(error) = db::insert_recovery_record(pool, &record).await {
        tracing::warn!(
            task_id = %task.id,
            error = %error,
            "failed to persist recovery history"
        );
    }
}

#[tauri::command]
#[specta::specta]
pub async fn bulk_resolve_attention(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    action: BulkRecoveryAction,
) -> Result<BulkRecoveryResult, String> {
    let mut result = BulkRecoveryResult {
        succeeded: 0,
        skipped: 0,
        failed: 0,
    };
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            result.skipped += 1;
            continue;
        }
        // R-2.3: serialize against start_task and other user actions on the
        // same task; each write re-validates the current state.
        let guard = state.task_runtime_locks.lock(id).await;
        let outcome = bulk_resolve_one(&app, state.inner(), id, action).await;
        drop(guard);
        match outcome {
            Ok(()) => result.succeeded += 1,
            Err(BulkItemOutcome::Skipped) => result.skipped += 1,
            Err(BulkItemOutcome::Failed(error)) => {
                result.failed += 1;
                tracing::warn!(task_id = id, error = %error, "bulk recovery action failed");
            }
        }
    }
    if result.succeeded > 0 {
        if let BulkRecoveryAction::RetryLater = action {
            // Same deferred dispatch as the single-task retry_later path.
            state.scheduler.clone().spawn_dispatch_after(
                app.clone(),
                state.pool.clone(),
                std::time::Duration::from_secs(300),
            );
        }
    }
    Ok(result)
}

enum BulkItemOutcome {
    Skipped,
    Failed(String),
}

async fn bulk_resolve_one(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    action: BulkRecoveryAction,
) -> Result<(), BulkItemOutcome> {
    let task = db::get_task_record(&state.pool, id)
        .await
        .map_err(BulkItemOutcome::Failed)?
        // Deleted concurrently while the batch was running.
        .ok_or(BulkItemOutcome::Skipped)?;
    let code = crate::models::AppErrorPayload::code_from_stored(
        task.error_code.as_deref(),
        task.error_message.as_deref(),
    );
    if bulk_resolution_gate(&task.status, code.as_deref()) == BulkGate::Skip {
        // Restart-required failures need the destructive per-task playbook
        // (delete temp + re-probe), not a blind re-queue onto changed bytes.
        return Err(BulkItemOutcome::Skipped);
    }
    let task = match action {
        BulkRecoveryAction::Retry => {
            queue_task_for_retry_with_event(app, state, id, "retrying", None)
                .await
                .map_err(BulkItemOutcome::Failed)?
        }
        BulkRecoveryAction::RetryLater => {
            let retry_after_at = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
            let event_message = format!("Retry scheduled for {retry_after_at}.");
            queue_task_for_retry_at(
                app,
                state,
                id,
                Some(&retry_after_at),
                Some("retry_later"),
                Some(&event_message),
            )
            .await
            .map_err(BulkItemOutcome::Failed)?
        }
    };
    record_recovery_history(
        &state.pool,
        &task,
        action.as_str(),
        "recovery_center",
        code.as_deref(),
    )
    .await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_credentials(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateTaskCredentialsInput,
) -> Result<Task, String> {
    let id = input.task_id.trim();
    if id.is_empty() {
        return Err("Task id is required.".to_string());
    }
    // R-2.3: hold the lock across read-validate-write so a download cannot
    // start with half-updated credentials.
    let _guard = state.task_runtime_locks.lock(id).await;
    let task = db::get_task_record(&state.pool, id)
        .await?
        .ok_or_else(|| "Task not found.".to_string())?;
    match credentials_update_gate(&task.protocol, &task.status) {
        Ok(()) => {}
        Err(CredentialsGateError::UnsupportedProtocol) => {
            return Err(format!(
                "Credential updates are not supported for {} tasks.",
                task.protocol
            ));
        }
        Err(CredentialsGateError::TaskBusy) => {
            return Err("Pause the task before changing its credentials.".to_string());
        }
    }
    db::upsert_task_credentials(
        &state.pool,
        id,
        &task.protocol,
        &input.username,
        &input.password,
        input.private_key_data.as_deref(),
        input.private_key_passphrase.as_deref(),
    )
    .await?;
    db::insert_task_event(&state.pool, id, "task_credentials_updated", None).await?;
    record_recovery_history(
        &state.pool,
        &task,
        "update_credentials",
        "recovery_center",
        task.error_code.as_deref(),
    )
    .await;
    events::emit_task_updated_record(&app, &state.pool, &task).await;
    task_from_record_with_files(&state.pool, task).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_recovery_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<RecoveryHistoryRecord>, String> {
    let limit = limit.unwrap_or(30).clamp(1, 100);
    db::list_recovery_history(&state.pool, limit).await
}
