use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::Client;
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{fs, io::AsyncWriteExt, sync::mpsc, task::JoinSet};

pub(super) mod diagnostics;
mod worker;

use self::diagnostics::{
    if_range_header_value, persist_error_diagnostic, persist_response_diagnostic,
    RequestDiagnosticContext,
};
pub(super) use self::worker::{download_segment_worker, SegmentWorkerRequest};

use super::{
    error::format_http_status,
    file::{finalize_download_file, persist_completed_path},
    request::send_get_with_retry,
};
use crate::{
    db,
    download::GlobalSpeedLimiter,
    events::{emit_queue_changed, emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        AppErrorPayload, RequestDiagnosticRecord, SegmentStatus, TaskProgressPayload, TaskRecord,
        TaskSegmentRecord, TaskStatus,
    },
};

struct UnknownSizeDownloadContext<'a> {
    client: &'a Client,
    app: AppHandle,
    pool: SqlitePool,
    task: TaskRecord,
    temp_path_buf: PathBuf,
    final_path_buf: PathBuf,
    segments: Vec<TaskSegmentRecord>,
    cancel: Arc<AtomicBool>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    request_headers: Vec<(String, String)>,
}

async fn run_unknown_size_download(context: UnknownSizeDownloadContext<'_>) -> Result<(), String> {
    let UnknownSizeDownloadContext {
        client,
        app,
        pool,
        task,
        temp_path_buf,
        final_path_buf,
        segments,
        cancel,
        speed_limiter,
        request_headers,
    } = context;

    let segment = segments
        .into_iter()
        .next()
        .ok_or_else(|| "Task segment could not be created.".to_string())?;
    let url = task.final_url.clone().unwrap_or_else(|| task.url.clone());

    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        1,
        Some("Downloading"),
        None,
    )
    .await?;
    db::update_segment_status(
        &pool,
        &segment.id,
        SegmentStatus::Downloading,
        Some(0),
        None,
    )
    .await?;

    if let Some(parent) = temp_path_buf.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }
    if fs::try_exists(&temp_path_buf).await.unwrap_or(false) {
        fs::remove_file(&temp_path_buf)
            .await
            .map_err(|e| format!("Could not reset the temporary file: {e}"))?;
    }

    let started_at = Instant::now();
    let mut response = match send_get_with_retry(client, &url, None, None, &request_headers).await {
        Ok(response) => {
            persist_response_diagnostic(
                RequestDiagnosticContext {
                    pool: &pool,
                    task_id: &task.id,
                    method: "GET",
                    url: &url,
                    range_header: None,
                    if_range_header: None,
                    retry_count: 0,
                    duration: started_at.elapsed(),
                },
                &response,
            )
            .await;
            response
        }
        Err(error) => {
            persist_error_diagnostic(
                RequestDiagnosticContext {
                    pool: &pool,
                    task_id: &task.id,
                    method: "GET",
                    url: &url,
                    range_header: None,
                    if_range_header: None,
                    retry_count: 0,
                    duration: started_at.elapsed(),
                },
                &error,
            )
            .await;
            return Err(error);
        }
    };
    if !response.status().is_success() {
        return Err(format_http_status(response.status()));
    }

    let mut file = fs::File::create(&temp_path_buf)
        .await
        .map_err(|e| format!("Could not create the temporary file: {e}"))?;
    let mut downloaded = 0_i64;
    let mut last_emit = Instant::now();
    let mut last_checkpoint = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_bytes = 0_i64;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("The connection failed while downloading: {e}"))?
    {
        if cancel.load(Ordering::SeqCst) {
            file.flush()
                .await
                .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
            db::update_task_and_segment_progress(
                &pool,
                &task.id,
                &segment.id,
                downloaded,
                0,
                1,
                TaskStatus::Downloading,
            )
            .await?;
            return Ok(());
        }

        speed_limiter.throttle(chunk.len()).await;
        file.write_all(&chunk).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write to disk: {e}"))
                .command_error()
        })?;
        downloaded += i64::try_from(chunk.len()).unwrap_or(0);

        if last_emit.elapsed() >= Duration::from_millis(300) {
            let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
            let speed_bps = ((downloaded - last_bytes) as f64 / elapsed) as i64;
            if last_checkpoint.elapsed() >= RUNTIME_PROGRESS_CHECKPOINT_INTERVAL {
                db::update_task_and_segment_progress(
                    &pool,
                    &task.id,
                    &segment.id,
                    downloaded,
                    speed_bps,
                    1,
                    TaskStatus::Downloading,
                )
                .await?;
                last_checkpoint = Instant::now();
            }
            emit_progress(
                &app,
                &task.id,
                downloaded,
                0,
                speed_bps,
                1,
                TaskStatus::Downloading,
            );
            last_emit = Instant::now();
            last_tick = Instant::now();
            last_bytes = downloaded;
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
    db::update_task_and_segment_progress(
        &pool,
        &task.id,
        &segment.id,
        downloaded,
        0,
        1,
        TaskStatus::Downloading,
    )
    .await?;

    let completed_path = finalize_download_file(&temp_path_buf, &final_path_buf).await?;
    persist_completed_path(&pool, &task.id, &completed_path).await?;
    db::complete_unknown_size_task(&pool, &task.id, &segment.id, downloaded).await?;
    if let Some(updated) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &updated).await;
    }
    emit_progress(
        &app,
        &task.id,
        downloaded,
        downloaded,
        0,
        0,
        TaskStatus::Completed,
    );
    emit_queue_changed(&app);
    Ok(())
}

const MAX_SEGMENT_RETRIES: i32 = 5;
const AUTO_ACCELERATION_MAX_SEGMENTS: usize = 8;
const AUTO_ACCELERATION_MIN_REMAINING_BYTES: i64 = 8 * 1024 * 1024;
const AUTO_ACCELERATION_WARMUP: Duration = Duration::from_secs(10);
const AUTO_ACCELERATION_EVALUATION: Duration = Duration::from_secs(5);
const AUTO_ACCELERATION_STABILITY_WINDOW: usize = 5;
const RUNTIME_PROGRESS_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub(super) enum SegmentMessage {
    Progress {
        segment_id: String,
        downloaded_until: i64,
        speed_bps: i64,
    },
    Retry {
        segment_id: String,
        downloaded_until: i64,
        retry_count: i32,
        error: String,
    },
    Request {
        record: RequestDiagnosticRecord,
    },
}

#[derive(Debug)]
pub(super) struct SegmentFailure {
    pub(super) segment_id: String,
    pub(super) downloaded_until: i64,
    pub(super) error: String,
}

struct RuntimeSegmentProgress {
    range_start: i64,
    range_end: i64,
    downloaded_until: i64,
    speed_bps: i64,
    status: SegmentStatus,
    dirty: bool,
}

struct RuntimeProgress {
    segments: HashMap<String, RuntimeSegmentProgress>,
    last_checkpoint: Instant,
}

impl RuntimeProgress {
    fn from_segments(segments: &[TaskSegmentRecord]) -> Self {
        Self {
            segments: segments
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
                .collect(),
            last_checkpoint: Instant::now(),
        }
    }

    fn mark_downloading(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.status = SegmentStatus::Downloading;
            segment.dirty = true;
        }
    }

    fn update_progress(&mut self, segment_id: String, downloaded_until: i64, speed_bps: i64) {
        if let Some(segment) = self.segments.get_mut(&segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = speed_bps.max(0);
            segment.status = SegmentStatus::Downloading;
            segment.dirty = true;
        }
    }

    fn mark_retrying(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = 0;
            segment.status = SegmentStatus::Pending;
            segment.dirty = false;
        }
    }

    fn mark_completed(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = 0;
            segment.status = SegmentStatus::Completed;
            segment.dirty = false;
        }
    }

    fn mark_failed(&mut self, segment_id: &str, downloaded_until: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.downloaded_until = downloaded_until;
            segment.speed_bps = 0;
            segment.status = SegmentStatus::Failed;
            segment.dirty = false;
        }
    }

    fn update_range_end(&mut self, segment_id: &str, range_end: i64) {
        if let Some(segment) = self.segments.get_mut(segment_id) {
            segment.range_end = range_end;
            segment.dirty = true;
        }
    }

    fn insert_segment(&mut self, segment: &TaskSegmentRecord) {
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

    fn total_downloaded(&self) -> i64 {
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

    fn total_speed(&self) -> i64 {
        self.segments
            .values()
            .map(|segment| segment.speed_bps.max(0))
            .sum()
    }
}

pub(super) struct SegmentedDownloadContext<'a> {
    pub(super) client: &'a Client,
    pub(super) app: AppHandle,
    pub(super) pool: SqlitePool,
    pub(super) task: TaskRecord,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) speed_limiter: Arc<GlobalSpeedLimiter>,
    pub(super) connection_limit: usize,
    pub(super) request_headers: Vec<(String, String)>,
}

#[tracing::instrument(skip(context), fields(task_id = %context.task.id))]
pub(super) async fn run_segmented_download(
    context: SegmentedDownloadContext<'_>,
) -> Result<(), String> {
    let SegmentedDownloadContext {
        client,
        app,
        pool,
        task,
        cancel,
        speed_limiter,
        connection_limit,
        request_headers,
    } = context;

    tracing::info!(
        task_id = %task.id,
        url = %sanitize_url(task.final_url.as_deref().unwrap_or(&task.url)),
        total_size = task.total_size,
        supports_parallel = task.supports_parallel,
        "segmented download started"
    );

    let temp_path = task
        .temp_path
        .clone()
        .ok_or_else(|| "Task is missing a temporary path.".to_string())?;
    let final_path = task
        .final_path
        .clone()
        .ok_or_else(|| "Task is missing a final path.".to_string())?;

    let temp_path_buf = PathBuf::from(&temp_path);
    let final_path_buf = PathBuf::from(&final_path);
    let segments = db::ensure_task_segments(&pool, &task).await?;
    if task.total_size <= 0 {
        return run_unknown_size_download(UnknownSizeDownloadContext {
            client,
            app,
            pool,
            task,
            temp_path_buf,
            final_path_buf,
            segments,
            cancel,
            speed_limiter,
            request_headers,
        })
        .await;
    }
    let segment_count = segments.len();
    let active_connection_count = i32::try_from(
        segments
            .iter()
            .filter(|segment| segment.downloaded_until <= segment.range_end)
            .count()
            .max(1),
    )
    .unwrap_or(1);
    let initial_downloaded = db::total_segment_downloaded_bytes(&segments);

    if initial_downloaded > 0 && !task.supports_resume {
        return Err("Resume unavailable. Restart this download from the beginning.".to_string());
    }

    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        active_connection_count,
        Some("Downloading"),
        None,
    )
    .await?;
    emit_progress(
        &app,
        &task.id,
        initial_downloaded,
        task.total_size,
        0,
        active_connection_count,
        TaskStatus::Downloading,
    );

    if let Some(parent) = temp_path_buf.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }

    if initial_downloaded == 0 && fs::try_exists(&temp_path_buf).await.unwrap_or(false) {
        fs::remove_file(&temp_path_buf)
            .await
            .map_err(|e| format!("Could not reset the temporary file: {e}"))?;
    }

    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&temp_path_buf)
        .await
        .map_err(|e| format!("Could not create the temporary file: {e}"))?;

    let (progress_tx, mut progress_rx) = mpsc::channel::<SegmentMessage>(64);
    let mut workers = JoinSet::new();
    let url = task.final_url.clone().unwrap_or_else(|| task.url.clone());
    let mut active_workers = 0_usize;
    let max_worker_count = connection_limit.clamp(
        1,
        AUTO_ACCELERATION_MAX_SEGMENTS.min(db::MAX_AUTO_SEGMENT_COUNT),
    );
    let mut runtime_progress = RuntimeProgress::from_segments(&segments);
    let mut range_ends = segments
        .iter()
        .map(|segment| {
            (
                segment.id.clone(),
                Arc::new(AtomicI64::new(segment.range_end)),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut pending_segments = VecDeque::new();
    let if_range = if_range_header_value(&task);

    for segment in segments {
        let offset = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if offset > segment.range_end {
            db::complete_segment(&pool, &segment.id).await?;
            runtime_progress.mark_completed(&segment.id, offset);
            continue;
        }
        pending_segments.push_back(segment);
    }
    spawn_segment_workers(
        client,
        &pool,
        &url,
        &temp_path_buf,
        task.supports_parallel,
        &cancel,
        &progress_tx,
        &mut workers,
        &mut active_workers,
        &mut pending_segments,
        segment_count,
        max_worker_count,
        &mut range_ends,
        &mut runtime_progress,
        speed_limiter.clone(),
        &request_headers,
        task.total_size,
        if_range.as_deref(),
    )
    .await?;

    let mut last_emit = Instant::now();
    let mut active_connection_count = i32::try_from(active_workers.max(1)).unwrap_or(1);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let started_at = Instant::now();
    let mut speed_history: VecDeque<(Instant, i64)> = VecDeque::new();
    let mut acceleration_disabled = false;
    let mut pending_acceleration: Option<AccelerationCheck> = None;

    while active_workers > 0 {
        tokio::select! {
            Some(message) = progress_rx.recv() => {
                handle_segment_message(
                    SegmentMessageContext {
                        app: &app,
                        pool: &pool,
                        task_id: &task.id,
                        total_size: task.total_size,
                        connection_count: active_connection_count,
                        update_task: !cancel.load(Ordering::SeqCst),
                    },
                    &mut runtime_progress,
                    message,
                )
                .await?;
                if !cancel.load(Ordering::SeqCst) && last_emit.elapsed() >= Duration::from_millis(300) {
                    emit_aggregate_progress(&app, &pool, &task.id, task.total_size, active_connection_count, &mut runtime_progress, false).await?;
                    last_emit = Instant::now();
                }
            }
            Some(result) = workers.join_next() => {
                active_workers = active_workers.saturating_sub(1);
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(failure)) => {
                        tracing::error!(
                            task_id = %task.id,
                            segment_id = %failure.segment_id,
                            error = %failure.error,
                            "segment download failed"
                        );
                        cancel.store(true, Ordering::SeqCst);
                        workers.abort_all();
                        runtime_progress.mark_failed(&failure.segment_id, failure.downloaded_until);
                        checkpoint_runtime_progress(
                            &pool,
                            &task.id,
                            active_connection_count,
                            TaskStatus::Downloading,
                            &mut runtime_progress,
                            true,
                        )
                        .await?;
                        db::update_segment_status(
                            &pool,
                            &failure.segment_id,
                            SegmentStatus::Failed,
                            Some(failure.downloaded_until),
                            Some(&failure.error),
                        )
                        .await?;
                        db::update_task_status(
                            &pool,
                            &task.id,
                            TaskStatus::Failed,
                            0,
                            0,
                            Some(&failure.error),
                            Some(&failure.error),
                        )
                        .await?;
                        emit_queue_changed(&app);
                        return Err(failure.error);
                    }
                    Err(error) => {
                        tracing::error!(
                            task_id = %task.id,
                            error = %error,
                            "download worker panicked or was cancelled unexpectedly"
                        );
                        cancel.store(true, Ordering::SeqCst);
                        workers.abort_all();
                        checkpoint_runtime_progress(
                            &pool,
                            &task.id,
                            active_connection_count,
                            TaskStatus::Downloading,
                            &mut runtime_progress,
                            true,
                        )
                        .await?;
                        let message = format!("A download worker stopped unexpectedly: {error}");
                        db::update_task_status(
                            &pool,
                            &task.id,
                            TaskStatus::Failed,
                            0,
                            0,
                            Some(&message),
                            Some(&message),
                        )
                        .await?;
                        emit_queue_changed(&app);
                        return Err(message);
                    }
                }
                spawn_segment_workers(
                    client,
                    &pool,
                    &url,
                    &temp_path_buf,
                    task.supports_parallel,
                    &cancel,
                    &progress_tx,
                    &mut workers,
                    &mut active_workers,
                    &mut pending_segments,
                    segment_count,
                    max_worker_count,
                    &mut range_ends,
                    &mut runtime_progress,
                    speed_limiter.clone(),
                    &request_headers,
                    task.total_size,
                    if_range.as_deref(),
                )
                .await?;
                active_connection_count = i32::try_from(active_workers.max(1)).unwrap_or(1);
            }
            _ = tick.tick() => {
                if cancel.load(Ordering::SeqCst) {
                    continue;
                }
                emit_aggregate_progress(&app, &pool, &task.id, task.total_size, active_connection_count, &mut runtime_progress, true).await?;
                let current_speed = runtime_progress.total_speed().max(0);
                speed_history.push_back((Instant::now(), current_speed));
                while speed_history.len() > AUTO_ACCELERATION_STABILITY_WINDOW {
                    speed_history.pop_front();
                }
                maybe_accelerate_segments(
                    client,
                    &app,
                    &pool,
                    &task,
                    &url,
                    &temp_path_buf,
                    &cancel,
                    &progress_tx,
                    &mut workers,
                    &mut active_workers,
                    &mut active_connection_count,
                    &mut acceleration_disabled,
                    &mut pending_acceleration,
                    &speed_history,
                    &mut runtime_progress,
                    &mut range_ends,
                    speed_limiter.clone(),
                    started_at,
                    max_worker_count,
                    &request_headers,
                    if_range.as_deref(),
                )
                .await?;
            }
            else => break,
        }
    }

    if cancel.load(Ordering::SeqCst) {
        checkpoint_runtime_progress(
            &pool,
            &task.id,
            active_connection_count,
            TaskStatus::Downloading,
            &mut runtime_progress,
            true,
        )
        .await?;
        tracing::info!(task_id = %task.id, "segmented download canceled");
        return Ok(());
    }

    while let Ok(message) = progress_rx.try_recv() {
        handle_segment_message(
            SegmentMessageContext {
                app: &app,
                pool: &pool,
                task_id: &task.id,
                total_size: task.total_size,
                connection_count: active_connection_count,
                update_task: false,
            },
            &mut runtime_progress,
            message,
        )
        .await?;
    }
    checkpoint_runtime_progress(
        &pool,
        &task.id,
        active_connection_count,
        TaskStatus::Downloading,
        &mut runtime_progress,
        true,
    )
    .await?;

    let final_segments = db::list_segment_records(&pool, &task.id).await?;
    for segment in &final_segments {
        if segment.downloaded_until <= segment.range_end {
            return Err("The download ended before all bytes were received.".to_string());
        }
        db::complete_segment(&pool, &segment.id).await?;
    }

    let temp_size = fs::metadata(&temp_path_buf)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .map_err(|e| format!("Could not inspect the temporary file: {e}"))?;
    if task.total_size > 0 && temp_size != task.total_size {
        return Err("The temporary file size does not match the remote file.".to_string());
    }

    let completed_path = finalize_download_file(&temp_path_buf, &final_path_buf).await?;
    persist_completed_path(&pool, &task.id, &completed_path).await?;

    db::complete_task(&pool, &task.id).await?;
    if let Some(updated) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &updated).await;
    }
    tracing::info!(
        task_id = %task.id,
        total_size = task.total_size,
        "segmented download completed"
    );
    emit_progress(
        &app,
        &task.id,
        task.total_size,
        task.total_size,
        0,
        0,
        TaskStatus::Completed,
    );
    emit_queue_changed(&app);

    Ok(())
}

// Worker spawning still binds the live worker queue, segment queue, and shared
// runtime state; keep this narrow exception until worker state becomes a struct.
#[allow(clippy::too_many_arguments)]
async fn spawn_segment_workers(
    client: &Client,
    pool: &SqlitePool,
    url: &str,
    temp_path: &Path,
    supports_parallel: bool,
    cancel: &Arc<AtomicBool>,
    progress_tx: &mpsc::Sender<SegmentMessage>,
    workers: &mut JoinSet<Result<(), SegmentFailure>>,
    active_workers: &mut usize,
    pending_segments: &mut VecDeque<TaskSegmentRecord>,
    segment_count: usize,
    max_worker_count: usize,
    range_ends: &mut HashMap<String, Arc<AtomicI64>>,
    runtime_progress: &mut RuntimeProgress,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    request_headers: &[(String, String)],
    total_size: i64,
    if_range: Option<&str>,
) -> Result<(), String> {
    while *active_workers < max_worker_count {
        let Some(segment) = pending_segments.pop_front() else {
            break;
        };
        let offset = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        db::update_segment_status(
            pool,
            &segment.id,
            SegmentStatus::Downloading,
            Some(offset),
            None,
        )
        .await?;
        runtime_progress.mark_downloading(&segment.id, offset);
        let range_end = range_ends
            .entry(segment.id.clone())
            .or_insert_with(|| Arc::new(AtomicI64::new(segment.range_end)))
            .clone();
        range_end.store(segment.range_end, Ordering::SeqCst);
        *active_workers += 1;
        workers.spawn(download_segment_worker(SegmentWorkerRequest {
            client: client.clone(),
            task_id: segment.task_id.clone(),
            url: url.to_string(),
            temp_path: temp_path.to_path_buf(),
            segment,
            total_size,
            segment_count,
            supports_parallel,
            cancel: cancel.clone(),
            progress_tx: progress_tx.clone(),
            range_end,
            speed_limiter: speed_limiter.clone(),
            request_headers: request_headers.to_vec(),
            if_range: if_range.map(str::to_string),
        }));
    }

    Ok(())
}

struct AccelerationCheck {
    before_connections: i32,
    before_speed_bps: i64,
    started_at: Instant,
}

// Auto-acceleration coordinates worker state, speed history, and live ranges;
// keep this exception until acceleration gets a dedicated coordinator object.
#[allow(clippy::too_many_arguments)]
async fn maybe_accelerate_segments(
    client: &Client,
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    url: &str,
    temp_path: &Path,
    cancel: &Arc<AtomicBool>,
    progress_tx: &mpsc::Sender<SegmentMessage>,
    workers: &mut JoinSet<Result<(), SegmentFailure>>,
    active_workers: &mut usize,
    active_connection_count: &mut i32,
    acceleration_disabled: &mut bool,
    pending_acceleration: &mut Option<AccelerationCheck>,
    speed_history: &VecDeque<(Instant, i64)>,
    runtime_progress: &mut RuntimeProgress,
    range_ends: &mut HashMap<String, Arc<AtomicI64>>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    started_at: Instant,
    max_worker_count: usize,
    request_headers: &[(String, String)],
    if_range: Option<&str>,
) -> Result<(), String> {
    if *acceleration_disabled || cancel.load(Ordering::SeqCst) || !task.supports_parallel {
        return Ok(());
    }

    let current_speed = runtime_progress.total_speed().max(0);
    if let Some(check) = pending_acceleration.take() {
        if check.started_at.elapsed() >= AUTO_ACCELERATION_EVALUATION {
            let connection_growth =
                (*active_connection_count as f64 / check.before_connections.max(1) as f64) - 1.0;
            let required_speed =
                check.before_speed_bps as f64 * (1.0 + (connection_growth.max(0.0) * 0.8));
            if check.before_speed_bps > 0 && (current_speed as f64) < required_speed {
                *acceleration_disabled = true;
                tracing::debug!(
                    task_id = %task.id,
                    current_speed,
                    before_speed = check.before_speed_bps,
                    "auto acceleration disabled after low yield"
                );
            }
        } else {
            *pending_acceleration = Some(check);
        }
        return Ok(());
    }

    if started_at.elapsed() < AUTO_ACCELERATION_WARMUP
        || speed_history.len() < AUTO_ACCELERATION_STABILITY_WINDOW
        || !speed_is_stable(speed_history)
        || *active_connection_count as usize >= AUTO_ACCELERATION_MAX_SEGMENTS
        || *active_connection_count as usize >= max_worker_count
    {
        return Ok(());
    }

    let Some(split) = db::split_largest_remaining_segment(
        pool,
        &task.id,
        AUTO_ACCELERATION_MIN_REMAINING_BYTES,
        AUTO_ACCELERATION_MAX_SEGMENTS,
    )
    .await?
    else {
        return Ok(());
    };

    runtime_progress.update_range_end(&split.original_segment_id, split.original_range_end);
    if let Some(range_end) = range_ends.get(&split.original_segment_id) {
        range_end.store(split.original_range_end, Ordering::SeqCst);
    }
    let tail_range_end = Arc::new(AtomicI64::new(split.tail_segment.range_end));
    range_ends.insert(split.tail_segment.id.clone(), tail_range_end.clone());
    runtime_progress.insert_segment(&split.tail_segment);

    db::update_segment_status(
        pool,
        &split.tail_segment.id,
        SegmentStatus::Downloading,
        Some(split.tail_segment.downloaded_until),
        None,
    )
    .await?;

    *active_workers += 1;
    *active_connection_count = i32::try_from(
        (*active_workers)
            .min(AUTO_ACCELERATION_MAX_SEGMENTS)
            .min(max_worker_count),
    )
    .unwrap_or(*active_connection_count);
    workers.spawn(download_segment_worker(SegmentWorkerRequest {
        client: client.clone(),
        task_id: split.tail_segment.task_id.clone(),
        url: url.to_string(),
        temp_path: temp_path.to_path_buf(),
        segment: split.tail_segment,
        total_size: task.total_size,
        segment_count: *active_connection_count as usize,
        supports_parallel: task.supports_parallel,
        cancel: cancel.clone(),
        progress_tx: progress_tx.clone(),
        range_end: tail_range_end,
        speed_limiter,
        request_headers: request_headers.to_vec(),
        if_range: if_range.map(str::to_string),
    }));

    *pending_acceleration = Some(AccelerationCheck {
        before_connections: (*active_connection_count - 1).max(1),
        before_speed_bps: current_speed,
        started_at: Instant::now(),
    });
    emit_aggregate_progress(
        app,
        pool,
        &task.id,
        task.total_size,
        *active_connection_count,
        runtime_progress,
        true,
    )
    .await?;
    Ok(())
}

fn speed_is_stable(speed_history: &VecDeque<(Instant, i64)>) -> bool {
    if speed_history.len() < AUTO_ACCELERATION_STABILITY_WINDOW {
        return false;
    }
    let speeds = speed_history
        .iter()
        .map(|(_, speed)| *speed)
        .filter(|speed| *speed > 0)
        .collect::<Vec<_>>();
    if speeds.len() < AUTO_ACCELERATION_STABILITY_WINDOW {
        return false;
    }

    let min = speeds.iter().copied().min().unwrap_or(0);
    let max = speeds.iter().copied().max().unwrap_or(0);
    let average = speeds.iter().sum::<i64>() as f64 / speeds.len() as f64;
    average > 0.0 && (max - min) as f64 <= average * 0.15
}

struct SegmentMessageContext<'a> {
    app: &'a AppHandle,
    pool: &'a SqlitePool,
    task_id: &'a str,
    total_size: i64,
    connection_count: i32,
    update_task: bool,
}

async fn handle_segment_message(
    context: SegmentMessageContext<'_>,
    runtime_progress: &mut RuntimeProgress,
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
                    task_id,
                    downloaded,
                    total_size,
                    0,
                    connection_count,
                    TaskStatus::Retrying,
                );
            }
        }
        SegmentMessage::Request { record } => {
            db::insert_request_diagnostic(pool, &record).await?;
        }
    }
    Ok(())
}

async fn emit_aggregate_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task_id: &str,
    total_size: i64,
    connection_count: i32,
    runtime_progress: &mut RuntimeProgress,
    force_checkpoint: bool,
) -> Result<(), String> {
    let downloaded = runtime_progress.total_downloaded();
    let speed_bps = runtime_progress.total_speed();
    checkpoint_runtime_progress(
        pool,
        task_id,
        connection_count,
        TaskStatus::Downloading,
        runtime_progress,
        force_checkpoint,
    )
    .await?;
    emit_progress(
        app,
        task_id,
        downloaded,
        total_size,
        speed_bps,
        connection_count,
        TaskStatus::Downloading,
    );
    Ok(())
}

async fn checkpoint_runtime_progress(
    pool: &SqlitePool,
    task_id: &str,
    connection_count: i32,
    status: TaskStatus,
    runtime_progress: &mut RuntimeProgress,
    force: bool,
) -> Result<(), String> {
    if !force && runtime_progress.last_checkpoint.elapsed() < RUNTIME_PROGRESS_CHECKPOINT_INTERVAL {
        return Ok(());
    }
    if !force
        && !runtime_progress
            .segments
            .values()
            .any(|segment| segment.dirty)
    {
        return Ok(());
    }

    let downloaded = runtime_progress.total_downloaded();
    let speed_bps = runtime_progress.total_speed();
    let updated_at = crate::models::task::now_iso();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE tasks
        SET downloaded_bytes = ?, speed_bps = ?, connection_count = ?, status = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded)
    .bind(speed_bps)
    .bind(connection_count)
    .bind(status.as_str())
    .bind(&updated_at)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        UPDATE task_files
        SET downloaded_bytes = ?, status = ?
        WHERE task_id = ? AND selected = 1
        "#,
    )
    .bind(downloaded)
    .bind(status.as_str())
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    for (segment_id, segment) in runtime_progress
        .segments
        .iter_mut()
        .filter(|(_, segment)| force || segment.dirty)
    {
        sqlx::query(
            r#"
            UPDATE task_work_units
            SET downloaded_until = ?, speed_bps = ?, status = ?, last_error = NULL
            WHERE id = ?
            "#,
        )
        .bind(segment.downloaded_until)
        .bind(segment.speed_bps.max(0))
        .bind(segment.status.as_str())
        .bind(segment_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        segment.dirty = false;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    runtime_progress.last_checkpoint = Instant::now();
    Ok(())
}

fn emit_progress(
    app: &AppHandle,
    task_id: &str,
    downloaded_bytes: i64,
    total_size: i64,
    speed_bps: i64,
    connection_count: i32,
    status: TaskStatus,
) {
    let payload = TaskProgressPayload {
        task_id: task_id.to_string(),
        downloaded_bytes: downloaded_bytes.to_string(),
        total_size: total_size.to_string(),
        speed_bps: speed_bps.to_string(),
        connection_count,
        status,
    };
    emit_task_progress(app, &payload);
}
