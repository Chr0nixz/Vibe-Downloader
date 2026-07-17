use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::Client;
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{fs, sync::mpsc, task::JoinSet};

use super::acceleration::{speed_is_stable, AccelerationCheck, AccelerationRuntime};
use super::checkpoint::checkpoint_runtime_progress;
use super::diagnostics::if_range_header_value;
use super::runtime_progress::{
    emit_aggregate_progress, emit_progress, handle_segment_message, progress_payload,
    AggregateProgressInput, RuntimeProgress, SegmentMessageContext,
};
use super::worker::{download_segment_worker, SegmentWorkerRequest};
use super::{
    SegmentFailure, SegmentMessage, SegmentedDownloadContext, AUTO_ACCELERATION_EVALUATION,
    AUTO_ACCELERATION_MAX_SEGMENTS, AUTO_ACCELERATION_MIN_REMAINING_BYTES,
    AUTO_ACCELERATION_STABILITY_WINDOW, AUTO_ACCELERATION_WARMUP,
};
use crate::{
    db,
    download::{
        file_ops::{finalize_download_file, persist_completed_path, preallocate_temp_file},
        GlobalSpeedLimiter,
    },
    events::{emit_queue_changed_with_ids, emit_task_updated_record, TaskProgressEmitGate},
    logging::sanitize_url,
    models::{SegmentStatus, TaskRecord, TaskSegmentRecord, TaskStatus},
};

/// Bundles the live worker queue together with the shared mutable bookkeeping
/// that both `spawn_segment_workers` and `maybe_accelerate_segments` touch.
/// Grouping these avoids exceeding clippy's `too_many_arguments` threshold.
struct WorkerPoolState {
    workers: JoinSet<Result<(), SegmentFailure>>,
    active_workers: usize,
    range_ends: HashMap<String, Arc<AtomicI64>>,
}

/// Bundles acceleration-specific state that is only used by
/// `maybe_accelerate_segments`, keeping its argument list under clippy's
/// `too_many_arguments` threshold.
struct AccelerationContext {
    runtime: AccelerationRuntime,
    speed_history: VecDeque<(Instant, i64)>,
    started_at: Instant,
}

pub(super) struct SegmentCoordinator<'a> {
    client: &'a Client,
    app: Option<AppHandle>,
    pool: SqlitePool,
    task: TaskRecord,
    temp_path: PathBuf,
    final_path: PathBuf,
    segments: Vec<TaskSegmentRecord>,
    cancel_token: tokio_util::sync::CancellationToken,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    request_headers: Vec<(String, String)>,

    // Derived constants computed once in `new`.
    url: String,
    if_range: Option<String>,
    total_size: i64,
    segment_count: usize,
    max_worker_count: usize,
}

impl<'a> SegmentCoordinator<'a> {
    pub(super) fn new(
        context: SegmentedDownloadContext<'a>,
        segments: Vec<TaskSegmentRecord>,
        temp_path: PathBuf,
        final_path: PathBuf,
    ) -> Self {
        let SegmentedDownloadContext {
            client,
            app,
            pool,
            task,
            cancel_token,
            speed_limiter,
            connection_limit,
            request_headers,
        } = context;

        let url = task.final_url.clone().unwrap_or_else(|| task.url.clone());
        let if_range = if_range_header_value(&task);
        let total_size = task.total_size;
        let segment_count = segments.len();
        let max_worker_count = connection_limit.clamp(
            1,
            AUTO_ACCELERATION_MAX_SEGMENTS.min(db::MAX_AUTO_SEGMENT_COUNT),
        );

        Self {
            client,
            app,
            pool,
            task,
            temp_path,
            final_path,
            segments,
            cancel_token,
            speed_limiter,
            request_headers,
            url,
            if_range,
            total_size,
            segment_count,
            max_worker_count,
        }
    }

    pub(super) async fn run(mut self) -> Result<(), String> {
        tracing::info!(
            task_id = %self.task.id,
            url = %sanitize_url(self.task.final_url.as_deref().unwrap_or(&self.task.url)),
            total_size = self.task.total_size,
            supports_parallel = self.task.supports_parallel,
            "segmented download started"
        );

        let active_connection_count = i32::try_from(
            self.segments
                .iter()
                .filter(|segment| segment.downloaded_until <= segment.range_end)
                .count()
                .max(1),
        )
        .unwrap_or(1);
        let initial_downloaded = db::total_segment_downloaded_bytes(&self.segments);

        if initial_downloaded > 0 && !self.task.supports_resume {
            return Err(
                "Resume unavailable. Restart this download from the beginning.".to_string(),
            );
        }

        db::update_task_status(
            &self.pool,
            &self.task.id,
            TaskStatus::Downloading,
            None,
            0,
            active_connection_count,
            Some("Downloading"),
            None,
        )
        .await?;
        let mut progress_gate = TaskProgressEmitGate::default();
        emit_progress(
            &self.app,
            &mut progress_gate,
            progress_payload(
                &self.task.id,
                initial_downloaded,
                self.task.total_size,
                0,
                active_connection_count,
                TaskStatus::Downloading,
            ),
            true,
        );

        if let Some(parent) = self.temp_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Could not create the download directory: {e}"))?;
        }

        if initial_downloaded == 0 && fs::try_exists(&self.temp_path).await.unwrap_or(false) {
            fs::remove_file(&self.temp_path)
                .await
                .map_err(|e| format!("Could not reset the temporary file: {e}"))?;
        }

        let temp_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.temp_path)
            .await
            .map_err(|e| format!("Could not create the temporary file: {e}"))?;
        preallocate_temp_file(&temp_file, self.task.total_size, &self.task.id).await;
        drop(temp_file);

        let (progress_tx, mut progress_rx) = mpsc::channel::<SegmentMessage>(64);
        let mut worker_pool = WorkerPoolState {
            workers: JoinSet::new(),
            active_workers: 0_usize,
            range_ends: self
                .segments
                .iter()
                .map(|segment| {
                    (
                        segment.id.clone(),
                        Arc::new(AtomicI64::new(segment.range_end)),
                    )
                })
                .collect::<HashMap<_, _>>(),
        };
        let mut runtime_progress = RuntimeProgress::from_segments(&self.segments);
        let mut pending_segments = VecDeque::new();

        // Take segments out of self so we can consume them in the loop below
        // while still borrowing `self` for method calls.
        let segments = std::mem::take(&mut self.segments);
        for segment in segments {
            // `downloaded_until` is exclusive: a value of `range_end + 1` means the segment is
            // fully downloaded. The clamp allows `range_end + 1` through so the `offset > range_end`
            // check below detects completion correctly.
            let offset = segment
                .downloaded_until
                .clamp(segment.range_start, segment.range_end.saturating_add(1));
            if offset > segment.range_end {
                db::complete_segment(&self.pool, &segment.id).await?;
                runtime_progress.mark_completed(&segment.id, offset);
                continue;
            }
            pending_segments.push_back(segment);
        }

        self.spawn_segment_workers(
            &mut worker_pool,
            &progress_tx,
            &mut pending_segments,
            &mut runtime_progress,
        )
        .await?;

        let mut last_emit = Instant::now();
        let mut active_connection_count =
            i32::try_from(worker_pool.active_workers.max(1)).unwrap_or(1);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut acceleration = AccelerationContext {
            runtime: AccelerationRuntime::default(),
            speed_history: VecDeque::new(),
            started_at: Instant::now(),
        };

        while worker_pool.active_workers > 0 {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    break;
                }
                Some(message) = progress_rx.recv() => {
                    handle_segment_message(
                        SegmentMessageContext {
                            app: &self.app,
                            pool: &self.pool,
                            task_id: &self.task.id,
                            total_size: self.task.total_size,
                            connection_count: active_connection_count,
                            update_task: !self.cancel_token.is_cancelled(),
                        },
                        &mut runtime_progress,
                        &mut progress_gate,
                        message,
                    )
                    .await?;
                    if !self.cancel_token.is_cancelled() && last_emit.elapsed() >= Duration::from_millis(300) {
                        emit_aggregate_progress(
                            &self.app,
                            &self.pool,
                            &mut runtime_progress,
                            &mut progress_gate,
                            AggregateProgressInput {
                                task_id: &self.task.id,
                                total_size: self.task.total_size,
                                connection_count: active_connection_count,
                                force_checkpoint: false,
                                force_emit: false,
                            },
                        )
                        .await?;
                        last_emit = Instant::now();
                    }
                }
                Some(result) = worker_pool.workers.join_next() => {
                    worker_pool.active_workers = worker_pool.active_workers.saturating_sub(1);
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(failure)) => {
                            tracing::error!(
                                task_id = %self.task.id,
                                segment_id = %failure.segment_id,
                                error = %failure.error,
                                "segment download failed"
                            );
                            self.cancel_token.cancel();
                            worker_pool.workers.abort_all();
                            acceleration.runtime.record_failure();
                            runtime_progress.mark_failed(&failure.segment_id, failure.downloaded_until);
                            checkpoint_runtime_progress(
                                &self.pool,
                                &self.task.id,
                                active_connection_count,
                                TaskStatus::Downloading,
                                &mut runtime_progress,
                                true,
                            )
                            .await?;
                            db::update_segment_status(
                                &self.pool,
                                &failure.segment_id,
                                SegmentStatus::Failed,
                                Some(failure.downloaded_until),
                                Some(&failure.error),
                            )
                            .await?;
                            crate::state_machine::transition_task(
                                &self.app,
                                &self.pool,
                                &self.task.id,
                                TaskStatus::Failed,
                                0,
                                0,
                                Some(&failure.error),
                                Some("failed"),
                            )
                            .await
                            .map_err(String::from)?;
                            emit_queue_changed_with_ids(&self.app, Some(vec![self.task.id.clone()]));
                            return Err(failure.error);
                        }
                        Err(error) => {
                            tracing::error!(
                                task_id = %self.task.id,
                                error = %error,
                                "download worker panicked or was cancelled unexpectedly"
                            );
                            self.cancel_token.cancel();
                            worker_pool.workers.abort_all();
                            checkpoint_runtime_progress(
                                &self.pool,
                                &self.task.id,
                                active_connection_count,
                                TaskStatus::Downloading,
                                &mut runtime_progress,
                                true,
                            )
                            .await?;
                            let message = format!("A download worker stopped unexpectedly: {error}");
                            crate::state_machine::transition_task(
                                &self.app,
                                &self.pool,
                                &self.task.id,
                                TaskStatus::Failed,
                                0,
                                0,
                                Some(&message),
                                Some("failed"),
                            )
                            .await
                            .map_err(String::from)?;
                            emit_queue_changed_with_ids(&self.app, Some(vec![self.task.id.clone()]));
                            return Err(message);
                        }
                    }
                    self.spawn_segment_workers(
                        &mut worker_pool,
                        &progress_tx,
                        &mut pending_segments,
                        &mut runtime_progress,
                    )
                    .await?;
                    active_connection_count = i32::try_from(worker_pool.active_workers.max(1)).unwrap_or(1);
                }
                _ = tick.tick() => {
                    if self.cancel_token.is_cancelled() {
                        continue;
                    }
                    emit_aggregate_progress(
                        &self.app,
                        &self.pool,
                        &mut runtime_progress,
                        &mut progress_gate,
                        AggregateProgressInput {
                            task_id: &self.task.id,
                            total_size: self.task.total_size,
                            connection_count: active_connection_count,
                            force_checkpoint: true,
                            force_emit: false,
                        },
                    )
                    .await?;
                    let current_speed = runtime_progress.total_speed().max(0);
                    acceleration.speed_history.push_back((Instant::now(), current_speed));
                    while acceleration.speed_history.len() > AUTO_ACCELERATION_STABILITY_WINDOW {
                        acceleration.speed_history.pop_front();
                    }
                    self.maybe_accelerate_segments(
                        &mut worker_pool,
                        &progress_tx,
                        &mut active_connection_count,
                        &mut acceleration,
                        &mut runtime_progress,
                        &mut progress_gate,
                    )
                    .await?;
                }
                else => break,
            }
        }

        if self.cancel_token.is_cancelled() {
            checkpoint_runtime_progress(
                &self.pool,
                &self.task.id,
                active_connection_count,
                TaskStatus::Downloading,
                &mut runtime_progress,
                true,
            )
            .await?;
            progress_gate.flush(&self.app);
            tracing::info!(task_id = %self.task.id, "segmented download canceled");
            return Ok(());
        }

        while let Ok(message) = progress_rx.try_recv() {
            handle_segment_message(
                SegmentMessageContext {
                    app: &self.app,
                    pool: &self.pool,
                    task_id: &self.task.id,
                    total_size: self.task.total_size,
                    connection_count: active_connection_count,
                    update_task: false,
                },
                &mut runtime_progress,
                &mut progress_gate,
                message,
            )
            .await?;
        }
        checkpoint_runtime_progress(
            &self.pool,
            &self.task.id,
            active_connection_count,
            TaskStatus::Downloading,
            &mut runtime_progress,
            true,
        )
        .await?;

        let final_segments = db::list_segment_records(&self.pool, &self.task.id).await?;
        for segment in &final_segments {
            if segment.downloaded_until <= segment.range_end {
                return Err("The download ended before all bytes were received.".to_string());
            }
            db::complete_segment(&self.pool, &segment.id).await?;
        }

        let temp_size = fs::metadata(&self.temp_path)
            .await
            .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
            .map_err(|e| format!("Could not inspect the temporary file: {e}"))?;
        if self.task.total_size > 0 && temp_size != self.task.total_size {
            return Err("The temporary file size does not match the remote file.".to_string());
        }

        let completed_path = finalize_download_file(&self.temp_path, &self.final_path).await?;
        persist_completed_path(&self.pool, &self.task.id, &completed_path).await?;

        db::complete_task(&self.pool, &self.task.id).await?;
        if let Some(updated) = db::get_task_record(&self.pool, &self.task.id).await? {
            emit_task_updated_record(&self.app, &self.pool, &updated).await;
        }
        tracing::info!(
            task_id = %self.task.id,
            total_size = self.task.total_size,
            "segmented download completed"
        );
        emit_progress(
            &self.app,
            &mut progress_gate,
            progress_payload(
                &self.task.id,
                self.task.total_size,
                self.task.total_size,
                0,
                0,
                TaskStatus::Completed,
            ),
            true,
        );
        emit_queue_changed_with_ids(&self.app, Some(vec![self.task.id.clone()]));

        Ok(())
    }

    async fn spawn_segment_workers(
        &self,
        worker_pool: &mut WorkerPoolState,
        progress_tx: &mpsc::Sender<SegmentMessage>,
        pending_segments: &mut VecDeque<TaskSegmentRecord>,
        runtime_progress: &mut RuntimeProgress,
    ) -> Result<(), String> {
        while worker_pool.active_workers < self.max_worker_count {
            let Some(segment) = pending_segments.pop_front() else {
                break;
            };
            let offset = segment
                .downloaded_until
                .clamp(segment.range_start, segment.range_end.saturating_add(1));
            db::update_segment_status(
                &self.pool,
                &segment.id,
                SegmentStatus::Downloading,
                Some(offset),
                None,
            )
            .await?;
            runtime_progress.mark_downloading(&segment.id, offset);
            let range_end = worker_pool
                .range_ends
                .entry(segment.id.clone())
                .or_insert_with(|| Arc::new(AtomicI64::new(segment.range_end)))
                .clone();
            range_end.store(segment.range_end, Ordering::SeqCst);
            worker_pool.active_workers += 1;
            worker_pool
                .workers
                .spawn(download_segment_worker(SegmentWorkerRequest {
                    client: self.client.clone(),
                    task_id: segment.task_id.clone(),
                    url: self.url.clone(),
                    temp_path: self.temp_path.clone(),
                    segment,
                    total_size: self.total_size,
                    segment_count: self.segment_count,
                    supports_resume: self.task.supports_resume,
                    cancel_token: self.cancel_token.clone(),
                    progress_tx: progress_tx.clone(),
                    range_end,
                    speed_limiter: self.speed_limiter.clone(),
                    request_headers: self.request_headers.clone(),
                    if_range: self.if_range.clone(),
                }));
        }

        Ok(())
    }

    async fn maybe_accelerate_segments(
        &self,
        worker_pool: &mut WorkerPoolState,
        progress_tx: &mpsc::Sender<SegmentMessage>,
        active_connection_count: &mut i32,
        acceleration: &mut AccelerationContext,
        runtime_progress: &mut RuntimeProgress,
        progress_gate: &mut TaskProgressEmitGate,
    ) -> Result<(), String> {
        if acceleration.runtime.disabled
            || self.cancel_token.is_cancelled()
            || !self.task.supports_parallel
        {
            return Ok(());
        }
        if acceleration
            .runtime
            .last_failure_at
            .is_some_and(|failed_at| failed_at.elapsed() < AUTO_ACCELERATION_EVALUATION)
        {
            return Ok(());
        }

        let current_speed = runtime_progress.total_speed().max(0);
        if let Some(check) = acceleration.runtime.pending.take() {
            if check.started_at.elapsed() >= AUTO_ACCELERATION_EVALUATION {
                // Require post-split speed to reach at least 80% of the theoretical linear speedup
                // (before_speed × (1 + 0.8 × growth)). If it falls short, the server isn't benefiting
                // from extra connections, so disable further splits to avoid wasting bandwidth.
                let connection_growth = (*active_connection_count as f64
                    / check.before_connections.max(1) as f64)
                    - 1.0;
                let required_speed =
                    check.before_speed_bps as f64 * (1.0 + (connection_growth.max(0.0) * 0.8));
                if check.before_speed_bps > 0 && (current_speed as f64) < required_speed {
                    acceleration.runtime.disabled = true;
                    db::insert_task_event(
                        &self.pool,
                        &self.task.id,
                        "http_acceleration_disabled",
                        Some("Acceleration did not improve throughput enough."),
                    )
                    .await?;
                    tracing::debug!(
                        task_id = %self.task.id,
                        current_speed,
                        before_speed = check.before_speed_bps,
                        "auto acceleration disabled after low yield"
                    );
                }
            } else {
                acceleration.runtime.pending = Some(check);
            }
            return Ok(());
        }

        // Only split when ALL preconditions hold: warmup complete (stable baseline), speed
        // history is stable (trustworthy measurement), pool has free worker slots, and the
        // split budget remains. Splitting without a stable baseline risks disabling
        // acceleration based on noisy measurements.
        if acceleration.started_at.elapsed() < AUTO_ACCELERATION_WARMUP
            || acceleration.speed_history.len() < AUTO_ACCELERATION_STABILITY_WINDOW
            || !speed_is_stable(&acceleration.speed_history)
            || *active_connection_count as usize >= AUTO_ACCELERATION_MAX_SEGMENTS
            || *active_connection_count as usize >= self.max_worker_count
            || acceleration.runtime.split_count >= AUTO_ACCELERATION_MAX_SEGMENTS.saturating_sub(1)
        {
            return Ok(());
        }

        let Some(split) = db::split_largest_remaining_segment(
            &self.pool,
            &self.task.id,
            AUTO_ACCELERATION_MIN_REMAINING_BYTES,
            AUTO_ACCELERATION_MAX_SEGMENTS,
        )
        .await?
        else {
            return Ok(());
        };

        runtime_progress.update_range_end(&split.original_segment_id, split.original_range_end);
        if let Some(range_end) = worker_pool.range_ends.get(&split.original_segment_id) {
            range_end.store(split.original_range_end, Ordering::SeqCst);
        }
        let tail_range_end = Arc::new(AtomicI64::new(split.tail_segment.range_end));
        worker_pool
            .range_ends
            .insert(split.tail_segment.id.clone(), tail_range_end.clone());
        runtime_progress.insert_segment(&split.tail_segment);

        db::update_segment_status(
            &self.pool,
            &split.tail_segment.id,
            SegmentStatus::Downloading,
            Some(split.tail_segment.downloaded_until),
            None,
        )
        .await?;

        worker_pool.active_workers += 1;
        *active_connection_count = i32::try_from(
            worker_pool
                .active_workers
                .min(AUTO_ACCELERATION_MAX_SEGMENTS)
                .min(self.max_worker_count),
        )
        .unwrap_or(*active_connection_count);
        let tail_segment_id = split.tail_segment.id.clone();
        worker_pool
            .workers
            .spawn(download_segment_worker(SegmentWorkerRequest {
                client: self.client.clone(),
                task_id: split.tail_segment.task_id.clone(),
                url: self.url.clone(),
                temp_path: self.temp_path.clone(),
                segment: split.tail_segment,
                total_size: self.total_size,
                segment_count: *active_connection_count as usize,
                supports_resume: self.task.supports_resume,
                cancel_token: self.cancel_token.clone(),
                progress_tx: progress_tx.clone(),
                range_end: tail_range_end,
                speed_limiter: self.speed_limiter.clone(),
                request_headers: self.request_headers.clone(),
                if_range: self.if_range.clone(),
            }));

        acceleration.runtime.split_count += 1;
        let event_payload = format!(
            "split={}; connections={}",
            tail_segment_id, *active_connection_count
        );
        db::insert_task_event(
            &self.pool,
            &self.task.id,
            "http_acceleration_split",
            Some(&event_payload),
        )
        .await?;
        acceleration.runtime.pending = Some(AccelerationCheck {
            before_connections: (*active_connection_count - 1).max(1),
            before_speed_bps: current_speed,
            started_at: Instant::now(),
        });
        emit_aggregate_progress(
            &self.app,
            &self.pool,
            runtime_progress,
            progress_gate,
            AggregateProgressInput {
                task_id: &self.task.id,
                total_size: self.task.total_size,
                connection_count: *active_connection_count,
                force_checkpoint: true,
                force_emit: false,
            },
        )
        .await?;
        Ok(())
    }
}
