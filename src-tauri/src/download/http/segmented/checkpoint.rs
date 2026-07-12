use std::time::Instant;

use sqlx::SqlitePool;

use crate::{db, models::TaskStatus};

use super::runtime_progress::RuntimeProgress;
use super::RUNTIME_PROGRESS_CHECKPOINT_INTERVAL;

pub(super) async fn checkpoint_runtime_progress(
    pool: &SqlitePool,
    task_id: &str,
    connection_count: i32,
    status: TaskStatus,
    runtime_progress: &mut RuntimeProgress,
    force: bool,
) -> Result<(), String> {
    let is_terminal = matches!(
        status,
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::NeedsAttention
    );
    if !force && runtime_progress.last_checkpoint.elapsed() < RUNTIME_PROGRESS_CHECKPOINT_INTERVAL {
        return Ok(());
    }
    let has_dirty_segments = runtime_progress
        .segments
        .values()
        .any(|segment| segment.dirty);
    if !force && !has_dirty_segments {
        return Ok(());
    }

    let downloaded = runtime_progress.total_downloaded();
    let speed_bps = runtime_progress.total_speed();
    let task_header_changed = downloaded != runtime_progress.last_written_downloaded
        || speed_bps != runtime_progress.last_written_speed_bps;

    // Fast path: nothing to write at all.
    if !task_header_changed && !has_dirty_segments {
        runtime_progress.last_checkpoint = Instant::now();
        return Ok(());
    }

    let update_files =
        task_header_changed && (is_terminal || runtime_progress.files_selection_changed);

    // Collect work units to update as owned data so the immutable borrow of
    // `runtime_progress.segments` ends before we clear dirty flags below.
    let work_units: Vec<(String, i64, i64, String)> = runtime_progress
        .segments
        .iter()
        .filter(|(_, segment)| (is_terminal && force) || segment.dirty)
        .map(|(id, segment)| {
            (
                id.clone(),
                segment.downloaded_until,
                segment.speed_bps.max(0),
                segment.status.as_str().to_string(),
            )
        })
        .collect();

    db::checkpoint_task_progress(
        pool,
        db::TaskProgressCheckpoint {
            task_id,
            downloaded_bytes: downloaded,
            speed_bps,
            connection_count,
            status: status.as_str(),
            update_files,
        },
        &work_units,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Mark the checkpointed segments as clean now that the transaction committed.
    for segment in runtime_progress.segments.values_mut() {
        if (is_terminal && force) || segment.dirty {
            segment.dirty = false;
        }
    }

    runtime_progress.last_checkpoint = Instant::now();
    runtime_progress.files_selection_changed = false;
    if task_header_changed {
        runtime_progress.last_written_downloaded = downloaded;
        runtime_progress.last_written_speed_bps = speed_bps;
    }
    Ok(())
}
