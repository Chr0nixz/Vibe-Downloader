use std::collections::HashMap;
use std::time::Instant;

use sqlx::SqlitePool;
use tauri::AppHandle;

use crate::{
    db,
    events::TaskProgressEmitGate,
    models::{SegmentStatus, TaskProgressPayload, TaskStatus, TaskSegmentRecord},
};

use super::{checkpoint::checkpoint_runtime_progress, SegmentMessage};

pub(super) struct RuntimeSegmentProgress {
    pub(super) range_start: i64,
    pub(super) range_end: i64,
    pub(super) downloaded_until: i64,
    pub(super) speed_bps: i64,
    pub(super) status: SegmentStatus,
    pub(super) dirty: bool,
}

pub(super) struct RuntimeProgress {
    pub(super) segments: HashMap<String, RuntimeSegmentProgress>,
    pub(super) last_checkpoint: Instant,
    pub(super) last_written_downloaded: i64,
    pub(super) last_written_speed_bps: i64,
    pub(super) files_selection_changed: bool,
}

impl RuntimeProgress {
    pub(super) fn from_segments(segments: &[TaskSegmentRecord]) -> Self {
        let seg_map: HashMap<String, RuntimeSegmentProgress> = segments
            .iter()
            .map(|segment| {
                (
                    segment.id.clone(),
                    RuntimeSegmentProgress {
                        range_start: segment.range_start,
                        range_end: segment.range_end,
                        downloaded_until: segment.downloaded_until,
                        speed_bps: segment.speed_bps,
                        status: segment.status,
                        dirty: false,
                    },
                )
            })
            .collect();
        let initial_downloaded = seg_map
            .values()
            .map(|s| (s.downloaded_until - s.range_start).max(0))
            .sum();
        Self {
            segments: seg_map,
            last_checkpoint: Instant::now(),
            last_written_downloaded: initial_downloaded,
            last_written_speed_bps: 0,
            files_selection_changed: false,
        }
    }

    pub(super) fn mark_downloading(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.status = SegmentStatus::Downloading;
            segment.dirty = true;
        }
    }

    pub(super) fn update_progress(&mut self, segment_id: String, downloaded_until: i64, speed_bps: i64) {
        if let Some(segment) = self.segments.get_mut(&segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = speed_bps.max(0);
            segment.status = SegmentStatus::Downloading;
            segment.dirty = true;
        }
    }

    pub(super) fn mark_retrying(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = 0;
            segment.status = SegmentStatus::Pending;
            segment.dirty = false;
        }
    }

    pub(super) fn mark_completed(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = 0;
            segment.status = SegmentStatus::Completed;
            segment.dirty = false;
        }
    }

    pub(super) fn mark_failed(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = 0;
            segment.status = SegmentStatus::Failed;
            segment.dirty = false;
        }
    }

    pub(super) fn update_range_end(&mut self, segment_id: &str, range_end: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.range_end = range_end;
            segment.dirty = true;
        }
    }

    pub(super) fn insert_segment(&mut self, segment: &TaskSegmentRecord) {
        self.segments.insert(
            segment.id.clone(),
            RuntimeSegmentProgress {
                range_start: segment.range_start,
                range_end: segment.range_end,
                downloaded_until: segment.downloaded_until,
                speed_bps: segment.speed_bps,
                status: segment.status,
                dirty: true,
            },
        );
    }

    pub(super) fn total_downloaded(&self) -> i64 {
        self.segments
            .values()
            .map(|segment| {
                let next_byte = segment
                    .downloaded_until
                    .clamp(segment.range_start, segment.range_end.saturating_add(1));
                next_byte.saturating_sub(segment.range_start)
            })
            .sum()
    }

    pub(super) fn total_speed(&self) -> i64 {
        self.segments
            .values()
            .map(|segment| segment.speed_bps.max(0))
            .sum()
    }
}

pub(super) struct SegmentMessageContext<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) pool: &'a SqlitePool,
    pub(super) task_id: &'a str,
    pub(super) total_size: i64,
    pub(super) connection_count: i32,
    pub(super) update_task: bool,
}

pub(super) async fn handle_segment_message(
    context: SegmentMessageContext<'_>,
    runtime_progress: &mut RuntimeProgress,
    progress_gate: &mut TaskProgressEmitGate,
    message: SegmentMessage,
) -> Result<(), String> {
    let SegmentMessageContext {
        app,
        pool,
        task_id,
        total_size,
        connection_count,
        update_task,
    } = context;

    match message {
        SegmentMessage::Progress {
            segment_id,
            downloaded_until,
            speed_bps,
        } => {
            let _ = update_task;
            runtime_progress.update_progress(segment_id, downloaded_until, speed_bps);
        }
        SegmentMessage::Retry {
            segment_id,
            downloaded_until,
            retry_count,
            error,
        } => {
            db::update_segment_retry(pool, &segment_id, downloaded_until, retry_count, &error)
                .await?;
            let payload = format!("{segment_id}: {error}");
            db::insert_task_event(pool, task_id, "retrying", Some(&payload)).await?;
            runtime_progress.mark_retrying(&segment_id, downloaded_until);
            if update_task {
                let downloaded = runtime_progress.total_downloaded();
                db::update_task_status(
                    pool,
                    task_id,
                    TaskStatus::Retrying,
                    0,
                    connection_count,
                    Some("Network fluctuation, retrying"),
                    None,
                )
                .await?;
                emit_progress(
                    app,
                    progress_gate,
                    progress_payload(
                        task_id,
                        downloaded,
                        total_size,
                        0,
                        connection_count,
                        TaskStatus::Retrying,
                    ),
                    true,
                );
            }
        }
        SegmentMessage::Request { record } => {
            db::insert_request_diagnostic(pool, &record).await?;
        }
    }
    Ok(())
}

pub(super) struct AggregateProgressInput<'a> {
    pub(super) task_id: &'a str,
    pub(super) total_size: i64,
    pub(super) connection_count: i32,
    pub(super) force_checkpoint: bool,
    pub(super) force_emit: bool,
}

pub(super) async fn emit_aggregate_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    runtime_progress: &mut RuntimeProgress,
    progress_gate: &mut TaskProgressEmitGate,
    input: AggregateProgressInput<'_>,
) -> Result<(), String> {
    let downloaded = runtime_progress.total_downloaded();
    let speed_bps = runtime_progress.total_speed();
    checkpoint_runtime_progress(
        pool,
        input.task_id,
        input.connection_count,
        TaskStatus::Downloading,
        runtime_progress,
        input.force_checkpoint,
    )
    .await?;
    emit_progress(
        app,
        progress_gate,
        progress_payload(
            input.task_id,
            downloaded,
            input.total_size,
            speed_bps,
            input.connection_count,
            TaskStatus::Downloading,
        ),
        input.force_emit,
    );
    Ok(())
}

pub(super) fn progress_payload(
    task_id: &str,
    downloaded_bytes: i64,
    total_size: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
) -> TaskProgressPayload {
    TaskProgressPayload {
        task_id: task_id.to_string(),
        downloaded_bytes: downloaded_bytes.to_string(),
        total_size: total_size.to_string(),
        speed_bps: speed_bps.to_string(),
        connection_count,
        status,
    }
}

pub(super) fn emit_progress(
    app: &AppHandle,
    progress_gate: &mut TaskProgressEmitGate,
    payload: TaskProgressPayload,
    force: bool,
) {
    progress_gate.emit_or_store(app, payload, force);
}
