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
/// - 写入 `db::update_task_status`
/// - 插入 `task_event`（如果 `event_type` 不为 `None`）
/// - 发射 `emit_task_updated_record`
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
    let current = db::get_task_record(pool, task_id)
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

    // E-1: merge status update + event insert into a single transaction.
    // RETURNING * eliminates the follow-up get_task_record SELECT.
    let mut tx = pool.begin().await.map_err(|e| TransitionError::Database(e.to_string()))?;
    let updated = db::update_task_status_in_tx(
        &mut tx,
        task_id,
        target,
        downloaded_bytes,
        connection_count,
        message,
        message,
    )
    .await?;

    if let Some(event_type) = event_type {
        db::insert_task_event_in_tx(&mut tx, task_id, event_type, message).await?;
    }

    tx.commit().await.map_err(|e| TransitionError::Database(e.to_string()))?;

    emit_task_updated_record(app, pool, &updated).await;

    Ok(updated)
}
