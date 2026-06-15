use sqlx::SqlitePool;

use crate::models::{SegmentStatus, TaskStatus};

use super::task_records::{error_state_from_message, recovery_actions_json};
use super::{delete_task_request_headers, insert_task_event};

pub async fn clear_tasks(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("DELETE FROM metalink_resources")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM metalink_tasks")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM hls_segments")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM hls_tasks")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM torrent_runtime_snapshots")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM torrent_tasks")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_request_headers")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_requests")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_events")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_work_units")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM task_files")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM tasks")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
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
    let updated_at = crate::models::task::now_iso();

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
    .bind(&updated_at)
    .bind(task_id)
    .execute(pool)
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

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
    update_task_progress(
        pool,
        task_id,
        downloaded_bytes,
        speed_bps,
        connection_count,
        status,
    )
    .await?;

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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_task_status(
    pool: &SqlitePool,
    task_id: &str,
    status: TaskStatus,
    speed_bps: i64,
    connection_count: i32,
    health_summary: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), String> {
    let updated_at = crate::models::task::now_iso();
    let (error_code, recovery_actions) = error_state_from_message(error_message);
    let recovery_actions = recovery_actions_json(&recovery_actions)?;

    sqlx::query(
        r#"
        UPDATE tasks
        SET status = ?, speed_bps = ?, connection_count = ?,
            health_summary = ?, error_message = ?, error_code = ?, recovery_actions = ?,
            retry_after_at = CASE WHEN ? IS NULL THEN NULL ELSE retry_after_at END,
            updated_at = ?
        WHERE id = ?
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET status = ?
        WHERE task_id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
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
    .execute(pool)
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_task_save_target(
    pool: &SqlitePool,
    task_id: &str,
    file_name: &str,
    save_dir: &str,
    final_path: &str,
) -> Result<(), String> {
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
    .execute(pool)
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn reset_task_download_state(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
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
    .execute(pool)
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

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
    .execute(pool)
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    insert_task_event(pool, task_id, "completed", None).await?;
    delete_task_request_headers(pool, task_id).await?;

    Ok(())
}

pub async fn complete_task_segment(
    pool: &SqlitePool,
    task_id: &str,
    segment_id: &str,
) -> Result<(), String> {
    complete_task(pool, task_id).await?;

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
    .execute(pool)
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
    .execute(pool)
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    insert_task_event(pool, task_id, "completed", None).await?;
    delete_task_request_headers(pool, task_id).await?;

    Ok(())
}

pub async fn delete_task_record(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM hls_segments WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM hls_tasks WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_request_headers WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_requests WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_work_units WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_files WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM task_events WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn reset_interrupted_tasks(pool: &SqlitePool, auto_resume: bool) -> Result<(), String> {
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
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_work_units
        SET status = 'pending', speed_bps = 0
        WHERE status = 'downloading'
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
