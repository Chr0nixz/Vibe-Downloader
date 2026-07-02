use sqlx::SqlitePool;
use tauri::AppHandle;

use crate::{
    db,
    events::emit_task_updated_record,
    models::{TaskRecord, TaskStatus},
};

/// 状态转换错误。
#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    /// 当前状态不允许转入目标状态。
    #[error("illegal transition: {from:?} → {to:?}")]
    Illegal { from: TaskStatus, to: TaskStatus },
    /// 数据库错误。
    #[error("database error: {0}")]
    Database(String),
    /// 任务不存在。
    #[error("task not found: {0}")]
    NotFound(String),
    /// 并发冲突：任务状态已被另一个路径修改，条件更新未匹配任何行。
    /// 调用方应视为可恢复（非致命），按当前实际状态重新决策。
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

/// 统一的状态转换入口。
///
/// - 校验转换合法性（不合法返回 `TransitionError::Illegal`，不写 DB）
/// - 写入 `db::update_task_status`（条件更新：`WHERE id = ? AND status = ?`）
/// - 插入 `task_event`（如果 `event_type` 不为 `None`）
/// - 发射 `emit_task_updated_record`
///
/// **R-1 并发保护**：读旧状态、校验、更新均在同一事务内完成。条件更新
/// `WHERE id = ? AND status = ?` 保证若另一路径已修改状态，本次更新不
/// 匹配任何行，返回 `TransitionError::Conflict`（调用方可视为可恢复）。
///
/// **注意**：进度数值更新（Downloading → Downloading）不走此函数，
/// 直接写 DB 以避免热路径开销。此函数仅用于**语义状态变更**。
#[allow(clippy::too_many_arguments)]
pub async fn transition_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task_id: &str,
    target: TaskStatus,
    downloaded_bytes: i64,
    connection_count: i32,
    message: Option<&str>,
    event_type: Option<&str>,
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
        db::insert_task_event_in_tx(&mut tx, task_id, event_type, message).await?;
    }

    tx.commit()
        .await
        .map_err(|e| TransitionError::Database(e.to_string()))?;

    emit_task_updated_record(app, pool, &updated).await;

    Ok(updated)
}
