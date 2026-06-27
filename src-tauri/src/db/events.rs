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
