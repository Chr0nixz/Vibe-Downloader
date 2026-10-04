use sqlx::SqlitePool;

use crate::{
    download::retry::MAX_TASK_AUTO_RETRIES,
    models::{AppErrorPayload, RecoveryAction},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRetryOutcome {
    Scheduled { attempt: u32 },
    Exhausted { attempt: u32 },
    StateChanged,
}

pub async fn auto_retry_attempt(pool: &SqlitePool, task_id: &str) -> Result<u32, String> {
    let attempt: Option<i64> =
        sqlx::query_scalar("SELECT attempt FROM task_auto_retry_state WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(attempt.unwrap_or(0).max(0).try_into().unwrap_or(u32::MAX))
}

pub async fn clear_auto_retry_state(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_auto_retry_state WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Atomically moves a live task back to the delayed queue and records the
/// retry budget. The active-state predicate is the cancellation contract: a
/// pause, cancel, delete, or manual retry that won the race prevents this
/// worker from resurrecting the task.
pub async fn schedule_auto_retry(
    pool: &SqlitePool,
    task_id: &str,
    requested_attempt: u32,
    retry_after_at: &str,
    reason: &str,
    error: &str,
) -> Result<AutoRetryOutcome, String> {
    let mut tx = crate::db::begin_immediate(pool)
        .await
        .map_err(|e| e.to_string())?;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if !matches!(
        status.as_deref(),
        Some("downloading") | Some("retrying") | Some("failed")
    ) {
        return Ok(AutoRetryOutcome::StateChanged);
    }

    let previous_attempt: i64 = sqlx::query_scalar(
        "SELECT COALESCE(attempt, 0) FROM task_auto_retry_state WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?
    .unwrap_or(0);
    let attempt = requested_attempt.max(
        u32::try_from(previous_attempt.max(0))
            .unwrap_or(u32::MAX)
            .saturating_add(1),
    );
    let (error_code, recovery_actions) = error_state(error);
    let updated_at = crate::models::task::now_iso();
    let recovery_actions = serde_json::to_string(&recovery_actions).map_err(|e| e.to_string())?;

    if attempt > MAX_TASK_AUTO_RETRIES {
        sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'failed', speed_bps = 0, connection_count = 0,
                health_summary = 'Automatic retry limit exhausted',
                error_message = ?, error_code = ?, recovery_actions = ?,
                retry_after_at = NULL, updated_at = ?
            WHERE id = ? AND status IN ('downloading', 'retrying', 'failed')
            "#,
        )
        .bind(error)
        .bind(error_code)
        .bind(&recovery_actions)
        .bind(&updated_at)
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        sqlx::query(
            r#"
            INSERT INTO task_auto_retry_state (task_id, attempt, reason, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(task_id) DO UPDATE SET
                attempt = excluded.attempt,
                reason = excluded.reason,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(task_id)
        .bind(i64::from(attempt))
        .bind(reason)
        .bind(&updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        mark_work_units(&mut tx, task_id, "failed", error).await?;
        sqlx::query(
            "UPDATE task_files SET status = 'failed' WHERE task_id = ? AND status != 'completed'",
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        let event_payload = format!("attempt={attempt}; reason={reason}");
        crate::db::insert_task_event_in_tx(
            &mut tx,
            task_id,
            "auto_retry_exhausted",
            Some(&event_payload),
        )
        .await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        return Ok(AutoRetryOutcome::Exhausted { attempt });
    }

    let event_payload =
        format!("attempt={attempt}; reason={reason}; retry_after_at={retry_after_at}");
    let result = sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'queued', speed_bps = 0, connection_count = 0,
            health_summary = 'Automatic retry scheduled',
            error_message = ?, error_code = ?, recovery_actions = ?,
            retry_after_at = ?, updated_at = ?
        WHERE id = ? AND status IN ('downloading', 'retrying', 'failed')
        "#,
    )
    .bind(error)
    .bind(error_code)
    .bind(&recovery_actions)
    .bind(retry_after_at)
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if result.rows_affected() == 0 {
        return Ok(AutoRetryOutcome::StateChanged);
    }

    mark_work_units(&mut tx, task_id, "pending", error).await?;
    sqlx::query(
        "UPDATE task_files SET status = 'queued' WHERE task_id = ? AND status != 'completed'",
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        INSERT INTO task_auto_retry_state (task_id, attempt, reason, updated_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            attempt = excluded.attempt,
            reason = excluded.reason,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task_id)
    .bind(i64::from(attempt))
    .bind(reason)
    .bind(&updated_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    crate::db::insert_task_event_in_tx(
        &mut tx,
        task_id,
        "auto_retry_scheduled",
        Some(&event_payload),
    )
    .await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(AutoRetryOutcome::Scheduled { attempt })
}

async fn mark_work_units(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    status: &str,
    error: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET status = ?, speed_bps = 0, last_error = ?
        WHERE task_id = ? AND status != 'completed'
        "#,
    )
    .bind(status)
    .bind(error)
    .bind(task_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn error_state(error: &str) -> (Option<String>, Vec<String>) {
    let Ok(payload) = serde_json::from_str::<AppErrorPayload>(error) else {
        return (None, Vec::new());
    };
    let actions = payload
        .actions
        .iter()
        .filter_map(|value| value.parse::<RecoveryAction>().ok())
        .map(|action| action.as_str().to_string())
        .collect();
    (Some(payload.code), actions)
}
