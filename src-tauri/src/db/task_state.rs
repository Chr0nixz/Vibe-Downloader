use sqlx::SqlitePool;

use crate::models::{SegmentStatus, TaskRecord, TaskStatus};

use super::task_records::{error_state_from_message, recovery_actions_json, row_to_task};

// ---------------------------------------------------------------------------
// Private transaction helpers — shared SQL fragments used by multiple public
// functions to avoid duplicating identical queries across transactions.
// ---------------------------------------------------------------------------

/// UPDATE tasks: downloaded_bytes, speed_bps, connection_count, status.
async fn update_progress_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
    updated_at: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = ?, speed_bps = ?, connection_count = ?, status = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes)
    .bind(speed_bps)
    .bind(connection_count)
    .bind(status.as_str())
    .bind(updated_at)
    .bind(task_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET downloaded_bytes = ?, status = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(downloaded_bytes)
    .bind(status.as_str())
    .bind(task_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// INSERT a 'completed' event + DELETE stale request headers — the tail end
/// of every "task finished" transaction.
async fn finalize_completion_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO task_events (task_id, event_type, payload, created_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(task_id)
    .bind("completed")
    .bind(None::<&str>)
    .bind(&created_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_request_headers WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// UPDATE task_work_units: mark a segment as downloading.
async fn update_segment_downloading_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    segment_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = ?, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes)
    .bind(speed_bps)
    .bind(SegmentStatus::Downloading.as_str())
    .bind(segment_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// UPDATE task_work_units: mark a segment as completed (range_end+1).
async fn complete_segment_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    segment_id: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = range_end + 1, status = 'completed', last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(segment_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn clear_tasks(pool: &SqlitePool) -> Result<(), String> {
    // Wrap every child-table delete plus the parent `tasks` delete in a
    // single transaction. Without this, a mid-sequence failure (disk full,
    // constraint violation, etc.) would leave the database in a partially
    // wiped, inconsistent state.
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM metalink_resources")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM metalink_tasks")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM hls_segments")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM hls_tasks")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM torrent_runtime_snapshots")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM torrent_tasks")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_request_headers")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_requests")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_events")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_work_units")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_files")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM tasks")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn delete_task_files_for_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_files WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_progress(
    pool: &SqlitePool,
    task_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    update_progress_in_tx(
        &mut tx,
        task_id,
        downloaded_bytes,
        speed_bps,
        connection_count,
        status,
        &updated_at,
    )
    .await?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_health_summary(
    pool: &SqlitePool,
    task_id: &str,
    health_summary: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET health_summary = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(health_summary)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_task_runtime_progress(
    pool: &SqlitePool,
    task_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
    health_summary: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = ?, speed_bps = ?, connection_count = ?,
            status = ?, health_summary = COALESCE(?, health_summary), updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes.max(0))
    .bind(speed_bps.max(0))
    .bind(connection_count.max(0))
    .bind(status.as_str())
    .bind(health_summary)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_and_segment_progress(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
    downloaded_bytes: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    update_progress_in_tx(
        &mut tx,
        task_id,
        downloaded_bytes,
        speed_bps,
        connection_count,
        status,
        &updated_at,
    )
    .await?;

    update_segment_downloading_in_tx(&mut tx, segment_id, downloaded_bytes, speed_bps).await?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// E-1: Transaction-scoped variant of update_task_status. Uses RETURNING * to
/// return the updated TaskRecord in the same round trip, eliminating the need
/// for a follow-up get_task_record SELECT. Callers that need to combine this
/// with other writes (e.g. insert_task_event_in_tx) can pass the same tx.
pub async fn update_task_status_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    status: TaskStatus,
    speed_bps: i64,
    connection_count: i32,
    health_summary: Option<&str>,
    error_message: Option<&str>,
) -> Result<TaskRecord, String> {
    let updated_at = crate::models::task::now_iso();
    let (error_code, recovery_actions) = error_state_from_message(error_message);
    let recovery_actions = recovery_actions_json(&recovery_actions)?;

    let row = sqlx::query(
        r#"
        UPDATE tasks
        SET status = ?, speed_bps = ?, connection_count = ?,
            health_summary = ?, error_message = ?, error_code = ?, recovery_actions = ?,
            retry_after_at = CASE WHEN ? IS NULL THEN NULL ELSE retry_after_at END,
            updated_at = ?
        WHERE id = ?
        RETURNING *
        "#,
    )
    .bind(status.as_str())
    .bind(speed_bps)
    .bind(connection_count)
    .bind(health_summary)
    .bind(error_message)
    .bind(error_code)
    .bind(recovery_actions)
    .bind(error_message)
    .bind(&updated_at)
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    let record = row_to_task(&row)?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = ?
        WHERE task_id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(task_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    Ok(record)
}

pub async fn update_task_status(
    pool: &SqlitePool,
    task_id: &str,
    status: TaskStatus,
    speed_bps: i64,
    connection_count: i32,
    health_summary: Option<&str>,
    error_message: Option<&str>,
) -> Result<TaskRecord, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let record = update_task_status_in_tx(&mut tx, task_id, status, speed_bps, connection_count, health_summary, error_message).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(record)
}

pub async fn update_task_retry_after(
    pool: &SqlitePool,
    task_id: &str,
    retry_after_at: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    sqlx::query(
        r#"
        UPDATE tasks
        SET retry_after_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(retry_after_at)
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_final_path(
    pool: &SqlitePool,
    task_id: &str,
    file_name: &str,
    final_path: &str,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET file_name = ?, final_path = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(file_name)
    .bind(final_path)
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET file_name = ?, relative_path = ?, final_path = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(file_name)
    .bind(file_name)
    .bind(final_path)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // E-7: file paths changed → bump files_version.
    sqlx::query("UPDATE tasks SET files_version = files_version + 1 WHERE id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_save_target(
    pool: &SqlitePool,
    task_id: &str,
    file_name: &str,
    save_dir: &str,
    final_path: &str,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET file_name = ?, save_dir = ?, final_path = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(file_name)
    .bind(save_dir)
    .bind(final_path)
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let temp_path = format!("{final_path}.vibe-downloading");
    sqlx::query(
        r#"
        UPDATE task_files
        SET file_name = ?, relative_path = ?, save_dir = ?, final_path = ?, temp_path = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(file_name)
    .bind(file_name)
    .bind(save_dir)
    .bind(final_path)
    .bind(temp_path)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // E-7: save target changed → bump files_version.
    sqlx::query("UPDATE tasks SET files_version = files_version + 1 WHERE id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn reset_task_download_state(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = 0, speed_bps = 0, connection_count = 0,
            status = 'queued', health_summary = 'Queued', error_message = NULL,
            error_code = NULL, recovery_actions = NULL, retry_after_at = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET downloaded_bytes = 0, status = 'queued'
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn delete_segments_for_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_work_units WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn complete_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'completed', downloaded_bytes = total_size, speed_bps = 0,
            connection_count = 0, health_summary = 'Completed',
            error_message = NULL, error_code = NULL, recovery_actions = NULL,
            retry_after_at = NULL, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = 'completed', downloaded_bytes = total_size
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    finalize_completion_in_tx(&mut tx, task_id).await?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn complete_task_segment(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'completed', downloaded_bytes = total_size, speed_bps = 0,
            connection_count = 0, health_summary = 'Completed',
            error_message = NULL, error_code = NULL, recovery_actions = NULL,
            retry_after_at = NULL, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = 'completed', downloaded_bytes = total_size
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    finalize_completion_in_tx(&mut tx, task_id).await?;
    complete_segment_in_tx(&mut tx, segment_id).await?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn complete_segment(pool: &SqlitePool, segment_id: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = range_end + 1, status = 'completed', last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn complete_unknown_size_task(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
    downloaded_bytes: i64,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();
    let final_size = downloaded_bytes.max(0);

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = 'completed', total_size = ?, downloaded_bytes = ?, speed_bps = 0,
            connection_count = 0, health_summary = 'Completed',
            error_message = NULL, error_code = NULL, recovery_actions = NULL,
            retry_after_at = NULL, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(final_size)
    .bind(final_size)
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = 'completed', total_size = ?, downloaded_bytes = ?
        WHERE task_id = ?
        "#,
    )
    .bind(final_size)
    .bind(final_size)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_work_units
        SET range_start = 0, range_end = ?, downloaded_until = ?, status = 'completed',
            last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(final_size.saturating_sub(1).max(0))
    .bind(final_size)
    .bind(segment_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    finalize_completion_in_tx(&mut tx, task_id).await?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Bundles the task-header progress fields written by `checkpoint_task_progress`.
/// Grouping these avoids exceeding clippy's `too_many_arguments` threshold.
pub struct TaskProgressCheckpoint<'a> {
    pub task_id: &'a str,
    pub downloaded_bytes: i64,
    pub speed_bps: i64,
    pub connection_count: i32,
    pub status: &'a str,
    pub update_files: bool,
}

/// Checkpoints runtime progress for a segmented download in a single transaction.
///
/// Updates the `tasks` header row, optionally updates all selected `task_files`
/// rows (when `update_files` is true), and updates each work unit in
/// `work_units`. The caller decides which work units to include and whether
/// file rows need updating, preserving the original checkpoint gating logic.
pub async fn checkpoint_task_progress(
    pool: &SqlitePool,
    checkpoint: TaskProgressCheckpoint<'_>,
    work_units: &[(String, i64, i64, String)],
) -> sqlx::Result<()> {
    let updated_at = crate::models::task::now_iso();
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = ?, speed_bps = ?, connection_count = ?, status = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(checkpoint.downloaded_bytes)
    .bind(checkpoint.speed_bps)
    .bind(checkpoint.connection_count)
    .bind(checkpoint.status)
    .bind(&updated_at)
    .bind(checkpoint.task_id)
    .execute(&mut *tx)
    .await?;

    // Only write task_files on terminal status or when file selection changed,
    // since the downloaded_bytes/status here duplicate the task header values.
    if checkpoint.update_files {
        sqlx::query(
            r#"
            UPDATE task_files
            SET downloaded_bytes = ?, status = ?
            WHERE task_id = ? AND selected = 1
            "#,
        )
        .bind(checkpoint.downloaded_bytes)
        .bind(checkpoint.status)
        .bind(checkpoint.task_id)
        .execute(&mut *tx)
        .await?;
    }

    // Non-terminal force checkpoints still respect per-segment dirty to avoid
    // writing all segments when only a few changed. Terminal force writes all.
    for (work_unit_id, downloaded_until, unit_speed_bps, unit_status) in work_units {
        sqlx::query(
            r#"
            UPDATE task_work_units
            SET downloaded_until = ?, speed_bps = ?, status = ?, last_error = NULL
            WHERE id = ?
            "#,
        )
        .bind(downloaded_until)
        .bind(unit_speed_bps)
        .bind(unit_status)
        .bind(work_unit_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn delete_task_record(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    // Wrap every cascade in one transaction. Without this, a failure mid-way
    // (e.g. a constraint violation or I/O error) would leave orphan rows in
    // the child tables that reference a now-deleted task id.
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM hls_segments WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM hls_tasks WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_request_headers WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_requests WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_work_units WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_files WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_events WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Deletes multiple task records in a single transaction. Used by
/// `bulk_delete_tasks` to ensure all DB cascades either succeed together
/// or fail together, preventing partial state on crash.
pub async fn delete_task_records_batch(
    pool: &SqlitePool,
    task_ids: &[String],
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for task_id in task_ids {
        sqlx::query("DELETE FROM hls_segments WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM hls_tasks WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM task_request_headers WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM task_requests WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM task_work_units WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM task_files WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM task_events WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM tasks WHERE id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn reset_interrupted_tasks(pool: &SqlitePool, auto_resume: bool) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let updated_at = crate::models::task::now_iso();
    let next_status = if auto_resume { "queued" } else { "paused" };
    let health_summary = if auto_resume {
        "Queued after app restart"
    } else {
        "Paused after app restart"
    };

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = ?, speed_bps = 0, connection_count = 0,
            health_summary = ?, error_message = NULL,
            error_code = NULL, recovery_actions = NULL,
            updated_at = ?
        WHERE status IN ('downloading', 'retrying')
        "#,
    )
    .bind(next_status)
    .bind(health_summary)
    .bind(&updated_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_work_units
        SET status = 'pending', speed_bps = 0
        WHERE status = 'downloading'
        "#,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}
