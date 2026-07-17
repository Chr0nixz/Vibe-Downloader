use sqlx::SqlitePool;

use crate::{
    db,
    events::{emit_task_updated_record, DownloadEventTarget},
    models::{SegmentStatus, TaskRecord, TaskStatus},
};

/// State transition error.
#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    /// The current state does not allow transitioning to the target state.
    #[error("illegal transition: {from:?} → {to:?}")]
    Illegal { from: TaskStatus, to: TaskStatus },
    /// Database error.
    #[error("database error: {0}")]
    Database(String),
    /// Task not found.
    #[error("task not found: {0}")]
    NotFound(String),
    /// Concurrency conflict: task state was modified by another path; the conditional update matched no rows.
    /// The caller should treat this as recoverable (non-fatal) and re-decide based on the actual current state.
    #[error("state transition conflict for task {task_id}: current is {current:?}, attempted {attempted:?}")]
    Conflict {
        task_id: String,
        current: TaskStatus,
        attempted: TaskStatus,
    },
}

impl From<sqlx::Error> for TransitionError {
    fn from(error: sqlx::Error) -> Self {
        TransitionError::Database(error.to_string())
    }
}

impl From<String> for TransitionError {
    fn from(error: String) -> Self {
        TransitionError::Database(error)
    }
}

impl From<TransitionError> for String {
    fn from(error: TransitionError) -> String {
        error.to_string()
    }
}

/// Unified state transition entry point.
///
/// - Validates transition legality (returns `TransitionError::Illegal` if invalid, no DB write)
/// - Writes `db::update_task_status` (conditional update: `WHERE id = ? AND status = ?`)
/// - Inserts a `task_event` (if `event_type` is not `None`)
/// - Emits `emit_task_updated_record`
///
/// **R-1 concurrency protection**: reading the old state, validation, and update all happen in the same transaction.
/// The conditional update `WHERE id = ? AND status = ?` ensures that if another path has already modified the state,
/// this update matches no rows and returns `TransitionError::Conflict` (caller may treat as recoverable).
///
/// **Note**: Progress value updates (Downloading → Downloading) do not go through this function;
/// they write directly to the DB to avoid hot-path overhead. This function is for **semantic state changes** only.
#[allow(clippy::too_many_arguments)]
pub async fn transition_task<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    target: TaskStatus,
    downloaded_bytes: i64,
    connection_count: i32,
    message: Option<&str>,
    event_type: Option<&str>,
) -> Result<TaskRecord, TransitionError> {
    transition_task_inner(
        app,
        pool,
        task_id,
        target,
        downloaded_bytes,
        connection_count,
        message,
        event_type,
        None,
        None,
    )
    .await
}

/// Transition a task while atomically resetting its work units and retry schedule.
///
/// Pause/resume/retry use this path so the task row, task files, work units,
/// event, and retry timestamp cannot describe different lifecycle states.
#[allow(clippy::too_many_arguments)]
pub async fn transition_task_with_runtime_state<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    target: TaskStatus,
    downloaded_bytes: i64,
    connection_count: i32,
    message: Option<&str>,
    event_type: Option<&str>,
    event_message: Option<&str>,
    work_unit_status: SegmentStatus,
    work_unit_error: Option<&str>,
    retry_after_at: Option<&str>,
) -> Result<TaskRecord, TransitionError> {
    transition_task_inner(
        app,
        pool,
        task_id,
        target,
        downloaded_bytes,
        connection_count,
        message,
        event_type,
        Some(event_message),
        Some((work_unit_status, work_unit_error, retry_after_at)),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn transition_task_inner<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    target: TaskStatus,
    downloaded_bytes: i64,
    connection_count: i32,
    message: Option<&str>,
    event_type: Option<&str>,
    event_message_override: Option<Option<&str>>,
    runtime_state: Option<(SegmentStatus, Option<&str>, Option<&str>)>,
) -> Result<TaskRecord, TransitionError> {
    // R-1: begin transaction first so read + validate + update are atomic.
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| TransitionError::Database(e.to_string()))?;

    let current = db::get_task_record_in_tx(&mut tx, task_id)
        .await?
        .ok_or_else(|| TransitionError::NotFound(task_id.to_string()))?;

    if !current.status.can_transition_to(target) {
        tracing::warn!(
            task_id = %task_id,
            from = ?current.status,
            to = ?target,
            "rejecting illegal state transition"
        );
        return Err(TransitionError::Illegal {
            from: current.status,
            to: target,
        });
    }

    // R-1: conditional update — only matches if status is still `current.status`.
    // If another path changed the status concurrently, this returns None and
    // we surface a Conflict error so the caller can re-evaluate.
    let expected = current.status;
    let updated = db::update_task_status_in_tx(
        &mut tx,
        task_id,
        target,
        Some(expected),
        downloaded_bytes,
        connection_count,
        message,
        message,
    )
    .await?;

    let updated = match updated {
        Some(record) => record,
        None => {
            // Condition did not match — re-query actual current status.
            let actual = db::get_task_record_in_tx(&mut tx, task_id).await?;
            let actual_status = actual.map(|r| r.status).unwrap_or(TaskStatus::Failed);
            tracing::warn!(
                task_id = %task_id,
                expected = ?expected,
                actual = ?actual_status,
                attempted = ?target,
                "state transition conflict: status changed concurrently"
            );
            return Err(TransitionError::Conflict {
                task_id: task_id.to_string(),
                current: actual_status,
                attempted: target,
            });
        }
    };

    if let Some(event_type) = event_type {
        let event_message = event_message_override.unwrap_or(message);
        db::insert_task_event_in_tx(&mut tx, task_id, event_type, event_message).await?;
    }

    if let Some((work_unit_status, work_unit_error, retry_after_at)) = runtime_state {
        db::update_segments_status_for_task_in_tx(
            &mut tx,
            task_id,
            work_unit_status,
            work_unit_error,
        )
        .await?;
        db::update_task_retry_after_in_tx(&mut tx, task_id, retry_after_at).await?;
    }

    tx.commit()
        .await
        .map_err(|e| TransitionError::Database(e.to_string()))?;

    emit_task_updated_record(app, pool, &updated).await;

    Ok(updated)
}
