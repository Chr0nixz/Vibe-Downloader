use sqlx::{Row, SqlitePool};

use crate::models::TaskEvent;

pub async fn insert_task_event(
    pool: &SqlitePool,
    task_id: &str,
    event_type: &str,
    payload: Option<&str>,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO task_events (task_id, event_type, payload, created_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(task_id)
    .bind(event_type)
    .bind(payload)
    .bind(&created_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// E-1: Transaction-scoped variant of insert_task_event. Allows combining the
/// event insert with update_task_status_in_tx in a single transaction.
pub async fn insert_task_event_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    event_type: &str,
    payload: Option<&str>,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO task_events (task_id, event_type, payload, created_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(task_id)
    .bind(event_type)
    .bind(payload)
    .bind(&created_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Returns the event_type of the most recent pause-related event ("paused" or
/// "paused_by_schedule") for a task, or `None` when no such event exists.
pub async fn get_latest_pause_event_type(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<String>, String> {
    let row = sqlx::query(
        r#"
        SELECT event_type FROM task_events
        WHERE task_id = ? AND event_type IN ('paused', 'paused_by_schedule')
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row.map(|r| r.get::<String, _>("event_type")))
}

pub async fn list_task_events_page(
    pool: &SqlitePool,
    task_id: &str,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<TaskEvent>, String> {
    let limit = limit.clamp(1, 500);
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, event_type, payload, created_at
        FROM task_events
        WHERE task_id = ? AND (? IS NULL OR id < ?)
        ORDER BY id DESC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(before_id)
    .bind(before_id)
    .bind(limit + 1)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|row| TaskEvent {
            id: row.get::<i64, _>("id").to_string(),
            task_id: row.get("task_id"),
            event_type: row.get("event_type"),
            payload: row.get("payload"),
            created_at: row.get("created_at"),
        })
        .collect())
}

/// Maximum age (in days) for retained task event rows.
pub const TASK_EVENTS_MAX_AGE_DAYS: i64 = 14;

/// Maximum number of task event rows kept per task.
pub const TASK_EVENTS_MAX_PER_TASK: i64 = 200;

/// PERF-05: prune `task_events` with per-task cap then age cap.
///
/// FUN-07: age deletion preserves the latest `paused` / `paused_by_schedule`
/// event for tasks that are still `paused`, so schedule resume can read intent.
pub async fn prune_task_events(pool: &SqlitePool) -> Result<u64, String> {
    let cap_result = sqlx::query(
        r#"
        DELETE FROM task_events
        WHERE id IN (
            SELECT id FROM (
                SELECT id,
                       ROW_NUMBER() OVER (
                           PARTITION BY task_id
                           ORDER BY id DESC
                       ) AS rn
                FROM task_events
            )
            WHERE rn > ?
        )
        "#,
    )
    .bind(TASK_EVENTS_MAX_PER_TASK)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut removed = cap_result.rows_affected();

    let cutoff = chrono::Utc::now() - chrono::Duration::days(TASK_EVENTS_MAX_AGE_DAYS);
    let cutoff_iso = cutoff.to_rfc3339();
    let age_result = sqlx::query(
        r#"
        DELETE FROM task_events
        WHERE created_at < ?
          AND id NOT IN (
            SELECT keep_id FROM (
              SELECT te.id AS keep_id
              FROM task_events te
              INNER JOIN tasks t ON t.id = te.task_id
              WHERE t.status = 'paused'
                AND te.event_type IN ('paused', 'paused_by_schedule')
                AND te.id = (
                  SELECT id FROM task_events
                  WHERE task_id = te.task_id
                    AND event_type IN ('paused', 'paused_by_schedule')
                  ORDER BY id DESC
                  LIMIT 1
                )
            )
          )
        "#,
    )
    .bind(&cutoff_iso)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    removed += age_result.rows_affected();
    Ok(removed)
}
