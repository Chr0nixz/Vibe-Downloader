use sqlx::SqlitePool;

use crate::{
    db,
    events::{emit_task_updated_record, DownloadEventTarget},
    models::{SegmentStatus, TaskRecord, TaskStatus},
};

/// ARC-21: the retry budget and the BUSY predicate now live in `db` so the
/// create path can reuse exactly the same policy. Kept as a local alias to
/// avoid churn in the call sites below.
use db::SQLITE_BUSY_MAX_ATTEMPTS as TRANSITION_BUSY_MAX_ATTEMPTS;

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

fn is_retryable_sqlite_busy(error: &TransitionError) -> bool {
    let TransitionError::Database(message) = error else {
        return false;
    };
    db::is_sqlite_busy_message(message)
}

/// Values written by a semantic task transition. Keeping the health summary,
/// error text, byte offset and instantaneous speed separate prevents a
/// lifecycle caller from accidentally publishing one field in another
/// column (ARC-66).
#[derive(Debug, Clone, Copy)]
pub struct TaskTransitionInput<'a> {
    pub target: TaskStatus,
    pub downloaded_bytes: Option<i64>,
    pub speed_bps: i64,
    pub connection_count: i32,
    pub health_summary: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub event_type: Option<&'a str>,
    pub event_message: Option<&'a str>,
}

impl<'a> TaskTransitionInput<'a> {
    pub fn legacy(
        target: TaskStatus,
        downloaded_bytes: i64,
        connection_count: i32,
        message: Option<&'a str>,
        event_type: Option<&'a str>,
    ) -> Self {
        Self {
            target,
            downloaded_bytes: None,
            speed_bps: downloaded_bytes,
            connection_count,
            health_summary: message,
            error_message: matches!(target, TaskStatus::Failed | TaskStatus::NeedsAttention)
                .then_some(message)
                .flatten(),
            event_type,
            event_message: message,
        }
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
/// **ARC-06**: Uses `BEGIN IMMEDIATE` so the write lock is taken before the status
/// read, avoiding `SQLITE_BUSY_SNAPSHOT` when checkpoints commit between a deferred
/// snapshot and the conditional UPDATE. Transient BUSY errors are retried with
/// bounded exponential backoff.
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
    transition_task_with_input(
        app,
        pool,
        task_id,
        TaskTransitionInput::legacy(
            target,
            downloaded_bytes,
            connection_count,
            message,
            event_type,
        ),
    )
    .await
}

/// Explicit transition entry point for new lifecycle code. The compatibility
/// wrapper above remains for older integration callers while production paths
/// migrate to named fields.
pub async fn transition_task_with_input<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    input: TaskTransitionInput<'_>,
) -> Result<TaskRecord, TransitionError> {
    transition_task_inner(app, pool, task_id, input, None).await
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
    transition_task_with_runtime_input(
        app,
        pool,
        task_id,
        TaskTransitionInput {
            event_message,
            ..TaskTransitionInput::legacy(
                target,
                downloaded_bytes,
                connection_count,
                message,
                event_type,
            )
        },
        Some((work_unit_status, work_unit_error, retry_after_at)),
    )
    .await
}

/// Explicit transition entry point that also updates work-unit and retry
/// state in the same transaction.
pub async fn transition_task_with_runtime_input<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    input: TaskTransitionInput<'_>,
    runtime_state: Option<(SegmentStatus, Option<&str>, Option<&str>)>,
) -> Result<TaskRecord, TransitionError> {
    transition_task_inner(app, pool, task_id, input, runtime_state).await
}

async fn transition_task_inner<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    input: TaskTransitionInput<'_>,
    runtime_state: Option<(SegmentStatus, Option<&str>, Option<&str>)>,
) -> Result<TaskRecord, TransitionError> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match transition_task_once(app, pool, task_id, input, runtime_state).await {
            Ok(record) => return Ok(record),
            Err(error)
                if is_retryable_sqlite_busy(&error) && attempt < TRANSITION_BUSY_MAX_ATTEMPTS =>
            {
                let delay = db::sqlite_busy_backoff(attempt);
                tracing::warn!(
                    task_id = %task_id,
                    attempt,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "state transition hit SQLITE_BUSY; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn transition_task_once<T: DownloadEventTarget + ?Sized>(
    app: &T,
    pool: &SqlitePool,
    task_id: &str,
    input: TaskTransitionInput<'_>,
    runtime_state: Option<(SegmentStatus, Option<&str>, Option<&str>)>,
) -> Result<TaskRecord, TransitionError> {
    // ARC-06 + R-1: IMMEDIATE write lock + conditional UPDATE in one transaction.
    let mut tx = db::begin_immediate(pool)
        .await
        .map_err(|e| TransitionError::Database(e.to_string()))?;

    let current = db::get_task_record_in_tx(&mut tx, task_id)
        .await?
        .ok_or_else(|| TransitionError::NotFound(task_id.to_string()))?;

    if !current.status.can_transition_to(input.target) {
        tracing::warn!(
            task_id = %task_id,
            from = ?current.status,
            to = ?input.target,
            "rejecting illegal state transition"
        );
        return Err(TransitionError::Illegal {
            from: current.status,
            to: input.target,
        });
    }

    // R-1: conditional update — only matches if status is still `current.status`.
    // If another path changed the status concurrently, this returns None and
    // we surface a Conflict error so the caller can re-evaluate.
    let expected = current.status;
    let updated = db::update_task_progress_status_in_tx(
        &mut tx,
        task_id,
        input.target,
        Some(expected),
        input.downloaded_bytes,
        input.speed_bps,
        input.connection_count,
        input.health_summary,
        input.error_message,
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
                attempted = ?input.target,
                "state transition conflict: status changed concurrently"
            );
            return Err(TransitionError::Conflict {
                task_id: task_id.to_string(),
                current: actual_status,
                attempted: input.target,
            });
        }
    };

    if let Some(event_type) = input.event_type {
        db::insert_task_event_in_tx(&mut tx, task_id, event_type, input.event_message).await?;
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
