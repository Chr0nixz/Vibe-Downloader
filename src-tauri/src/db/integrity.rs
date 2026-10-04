//! Read-only aggregates backing the Integrity Passport (feature proposal §2.4).
//!
//! The start milestone and retry counters come from `task_events`, which is
//! retention-pruned (200 rows / 14 days per task), so they can legitimately be
//! absent. Completion is a durable task fact and is read from `tasks` instead
//! of being reconstructed from the prunable event log.

use sqlx::{Row, SqlitePool};

/// Milestone timestamps derived from the task event log.
#[derive(Debug, Clone, Default)]
pub struct TaskMilestones {
    /// First `started` event — when the task actually began downloading.
    pub started_at: Option<String>,
}

/// Static SQL only: one scalar subquery, no dynamic table names.
pub async fn task_milestones(pool: &SqlitePool, task_id: &str) -> Result<TaskMilestones, String> {
    let row = sqlx::query(
        r#"
        SELECT
            (SELECT created_at FROM task_events
             WHERE task_id = ? AND event_type = 'started'
             ORDER BY id ASC LIMIT 1) AS started_at
        "#,
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(TaskMilestones {
        started_at: row.get("started_at"),
    })
}

/// Count of `resumed` events — how often the task was resumed after a pause.
pub async fn count_task_resumes(pool: &SqlitePool, task_id: &str) -> Result<i64, String> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS resume_count FROM task_events
        WHERE task_id = ? AND event_type = 'resumed'
        "#,
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.get::<i64, _>("resume_count"))
}

/// Sum of per-segment retry counters across the task's work units.
pub async fn sum_task_segment_retries(pool: &SqlitePool, task_id: &str) -> Result<i64, String> {
    let row = sqlx::query(
        r#"
        SELECT COALESCE(SUM(retry_count), 0) AS retry_sum
        FROM task_work_units
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.get::<i64, _>("retry_sum"))
}
