use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::{
    header::{ACCEPT_ENCODING, RANGE},
    Client, StatusCode,
};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::{mpsc, RwLock},
    task::JoinSet,
};

use super::{
    error::format_http_status,
    file::{finalize_download_file, persist_completed_path},
    request::{is_retryable_status, retry_after_duration, send_get_with_retry},
};
use crate::{
    db,
    download::GlobalSpeedLimiter,
    events::{emit_queue_changed, emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        AppErrorPayload, SegmentStatus, TaskProgressPayload, TaskRecord, TaskSegmentRecord,
        TaskStatus,
    },
};

async fn run_unknown_size_download(
    client: &Client,
    app: AppHandle,
    pool: SqlitePool,
    task: TaskRecord,
    temp_path_buf: PathBuf,
    final_path_buf: PathBuf,
    segments: Vec<TaskSegmentRecord>,
    cancel: Arc<AtomicBool>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    _connection_limit: usize,
) -> Result<(), String> {
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

    let mut response = send_get_with_retry(client, &url, None).await?;
    if !response.status().is_success() {
        return Err(format_http_status(response.status()));
    }

    let mut file = fs::File::create(&temp_path_buf)
        .await
        .map_err(|e| format!("Could not create the temporary file: {e}"))?;
    let mut downloaded = 0_i64;
    let mut last_emit = Instant::now();
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
            db::update_segment_progress(&pool, &segment.id, downloaded, SegmentStatus::Downloading)
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
}

#[derive(Debug)]
pub(super) struct SegmentFailure {
    pub(super) segment_id: String,
    pub(super) downloaded_until: i64,
    pub(super) error: String,
}

#[tracing::instrument(skip(client, app, pool, cancel), fields(task_id = %task.id))]
pub(super) async fn run_segmented_download(
    client: &Client,
    app: AppHandle,
    pool: SqlitePool,
    task: TaskRecord,
    cancel: Arc<AtomicBool>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    connection_limit: usize,
) -> Result<(), String> {
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
        return run_unknown_size_download(
            client,
            app,
            pool,
            task,
            temp_path_buf,
            final_path_buf,
            segments,
            cancel,
            speed_limiter,
            connection_limit.max(1),
        )
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
    let live_ends = Arc::new(RwLock::new(
        segments
            .iter()
            .map(|segment| (segment.id.clone(), segment.range_end))
            .collect::<HashMap<_, _>>(),
    ));
    let mut pending_segments = VecDeque::new();

    for segment in segments {
        let offset = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if offset > segment.range_end {
            db::complete_segment(&pool, &segment.id).await?;
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
        &live_ends,
        speed_limiter.clone(),
    )
    .await?;

    let mut last_speeds: HashMap<String, i64> = HashMap::new();
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
                    &app,
                    &pool,
                    &task.id,
                    task.total_size,
                    active_connection_count,
                    &mut last_speeds,
                    message,
                    !cancel.load(Ordering::SeqCst),
                )
                .await?;
                if !cancel.load(Ordering::SeqCst) && last_emit.elapsed() >= Duration::from_millis(300) {
                    emit_aggregate_progress(&app, &pool, &task.id, task.total_size, active_connection_count, &last_speeds).await?;
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
                    &live_ends,
                    speed_limiter.clone(),
                )
                .await?;
                active_connection_count = i32::try_from(active_workers.max(1)).unwrap_or(1);
            }
            _ = tick.tick() => {
                if cancel.load(Ordering::SeqCst) {
                    continue;
                }
                emit_aggregate_progress(&app, &pool, &task.id, task.total_size, active_connection_count, &last_speeds).await?;
                let current_speed = last_speeds.values().copied().sum::<i64>().max(0);
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
                    &last_speeds,
                    &live_ends,
                    speed_limiter.clone(),
                    started_at,
                    max_worker_count,
                )
                .await?;
            }
            else => break,
        }
    }

    if cancel.load(Ordering::SeqCst) {
        tracing::info!(task_id = %task.id, "segmented download canceled");
        return Ok(());
    }

    while let Ok(message) = progress_rx.try_recv() {
        handle_segment_message(
            &app,
            &pool,
            &task.id,
            task.total_size,
            active_connection_count,
            &mut last_speeds,
            message,
            false,
        )
        .await?;
    }

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
    live_ends: &Arc<RwLock<HashMap<String, i64>>>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
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
        *active_workers += 1;
        workers.spawn(download_segment_worker(SegmentWorkerRequest {
            client: client.clone(),
            url: url.to_string(),
            temp_path: temp_path.to_path_buf(),
            segment,
            segment_count,
            supports_parallel,
            cancel: cancel.clone(),
            progress_tx: progress_tx.clone(),
            live_ends: live_ends.clone(),
            speed_limiter: speed_limiter.clone(),
        }));
    }

    Ok(())
}

struct AccelerationCheck {
    before_connections: i32,
    before_speed_bps: i64,
    started_at: Instant,
}

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
    last_speeds: &HashMap<String, i64>,
    live_ends: &Arc<RwLock<HashMap<String, i64>>>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    started_at: Instant,
    max_worker_count: usize,
) -> Result<(), String> {
    if *acceleration_disabled || cancel.load(Ordering::SeqCst) || !task.supports_parallel {
        return Ok(());
    }

    let current_speed = last_speeds.values().copied().sum::<i64>().max(0);
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

    {
        let mut ends = live_ends.write().await;
        ends.insert(split.original_segment_id.clone(), split.original_range_end);
        ends.insert(split.tail_segment.id.clone(), split.tail_segment.range_end);
    }

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
        url: url.to_string(),
        temp_path: temp_path.to_path_buf(),
        segment: split.tail_segment,
        segment_count: *active_connection_count as usize,
        supports_parallel: task.supports_parallel,
        cancel: cancel.clone(),
        progress_tx: progress_tx.clone(),
        live_ends: live_ends.clone(),
        speed_limiter,
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
        last_speeds,
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

pub(super) struct SegmentWorkerRequest {
    pub(super) client: Client,
    pub(super) url: String,
    pub(super) temp_path: PathBuf,
    pub(super) segment: TaskSegmentRecord,
    pub(super) segment_count: usize,
    pub(super) supports_parallel: bool,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) progress_tx: mpsc::Sender<SegmentMessage>,
    pub(super) live_ends: Arc<RwLock<HashMap<String, i64>>>,
    pub(super) speed_limiter: Arc<GlobalSpeedLimiter>,
}

pub(super) async fn download_segment_worker(
    request: SegmentWorkerRequest,
) -> Result<(), SegmentFailure> {
    let SegmentWorkerRequest {
        client,
        url,
        temp_path,
        segment,
        segment_count,
        supports_parallel,
        cancel,
        progress_tx,
        live_ends,
        speed_limiter,
    } = request;

    let mut offset = segment
        .downloaded_until
        .clamp(segment.range_start, segment.range_end.saturating_add(1));
    let mut retry_count = segment.retry_count.max(0);

    loop {
        if cancel.load(Ordering::SeqCst) {
            send_segment_progress(&progress_tx, &segment.id, offset, 0).await?;
            return Ok(());
        }

        match download_segment_once(SegmentAttemptRequest {
            client: &client,
            url: &url,
            temp_path: &temp_path,
            segment: &segment,
            segment_count,
            supports_parallel,
            cancel: &cancel,
            progress_tx: &progress_tx,
            live_ends: &live_ends,
            speed_limiter: &speed_limiter,
            offset,
        })
        .await
        {
            Ok(_) => return Ok(()),
            Err(error) if error.retryable && retry_count < MAX_SEGMENT_RETRIES => {
                offset = error.failure.downloaded_until;
                retry_count += 1;
                send_segment_retry(
                    &progress_tx,
                    &segment.id,
                    offset,
                    retry_count,
                    &error.failure.error,
                )
                .await?;
                tokio::time::sleep(
                    error
                        .retry_after
                        .unwrap_or_else(|| retry_delay(retry_count)),
                )
                .await;
            }
            Err(error) => return Err(error.failure),
        }
    }
}

struct SegmentAttemptRequest<'a> {
    client: &'a Client,
    url: &'a str,
    temp_path: &'a Path,
    segment: &'a TaskSegmentRecord,
    segment_count: usize,
    supports_parallel: bool,
    cancel: &'a Arc<AtomicBool>,
    progress_tx: &'a mpsc::Sender<SegmentMessage>,
    live_ends: &'a Arc<RwLock<HashMap<String, i64>>>,
    speed_limiter: &'a Arc<GlobalSpeedLimiter>,
    offset: i64,
}

struct SegmentAttemptError {
    failure: SegmentFailure,
    retryable: bool,
    retry_after: Option<Duration>,
}

async fn download_segment_once(
    request: SegmentAttemptRequest<'_>,
) -> Result<i64, SegmentAttemptError> {
    let SegmentAttemptRequest {
        client,
        url,
        temp_path,
        segment,
        segment_count,
        supports_parallel,
        cancel,
        progress_tx,
        live_ends,
        speed_limiter,
        mut offset,
    } = request;

    let range_end = live_segment_end(live_ends, segment).await;
    if offset > range_end {
        send_segment_progress(progress_tx, &segment.id, offset, 0)
            .await
            .map_err(non_retryable_attempt)?;
        return Ok(offset);
    }

    let use_range = segment_count > 1 || offset > segment.range_start;
    if use_range && !supports_parallel {
        return Err(non_retryable(segment_failure(
            segment,
            offset,
            "Resume unavailable. Restart this download from the beginning.",
        )));
    }

    let mut http_request = client.get(url).header(ACCEPT_ENCODING, "identity");
    if use_range {
        http_request = http_request.header(RANGE, format!("bytes={offset}-{range_end}"));
    }

    let mut response = http_request.send().await.map_err(|e| {
        retryable(segment_failure(
            segment,
            offset,
            &format!("Could not connect to the server: {e}"),
        ))
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let retry_after = retry_after_duration(&response);
        let failure = segment_failure(segment, offset, &format_http_status(status));
        return if is_retryable_status(status) {
            Err(retryable_with_delay(failure, retry_after))
        } else {
            Err(non_retryable(failure))
        };
    }
    if use_range && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(non_retryable(segment_failure(
            segment,
            offset,
            "Resume unavailable. The server did not honor the byte range request.",
        )));
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(temp_path)
        .await
        .map_err(|e| {
            non_retryable(segment_failure(
                segment,
                offset,
                &format!("Could not open the temporary file: {e}"),
            ))
        })?;

    file.seek(std::io::SeekFrom::Start(u64::try_from(offset).unwrap_or(0)))
        .await
        .map_err(|e| {
            non_retryable(segment_failure(
                segment,
                offset,
                &format!("Could not seek in the temporary file: {e}"),
            ))
        })?;

    let mut last_emit = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_bytes = offset;

    while let Some(chunk) = response.chunk().await.map_err(|e| {
        retryable(segment_failure(
            segment,
            offset,
            &format!("The connection failed while downloading: {e}"),
        ))
    })? {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await.map_err(|e| {
                non_retryable(segment_failure(
                    segment,
                    offset,
                    &format!("Could not flush the temporary file: {e}"),
                ))
            })?;
            send_segment_progress(progress_tx, &segment.id, offset, 0)
                .await
                .map_err(non_retryable_attempt)?;
            return Ok(offset);
        }

        let current_end = live_segment_end(live_ends, segment).await;
        if offset > current_end {
            send_segment_progress(progress_tx, &segment.id, offset, 0)
                .await
                .map_err(non_retryable_attempt)?;
            return Ok(offset);
        }

        let allowed = current_end.saturating_add(1).saturating_sub(offset);
        let write_len = i64::try_from(chunk.len()).unwrap_or(0).min(allowed);
        if write_len <= 0 {
            return Ok(offset);
        }
        if i64::try_from(chunk.len()).unwrap_or(0) > allowed && current_end == range_end {
            return Err(non_retryable(segment_failure(
                segment,
                offset,
                "The server sent more bytes than the requested segment range.",
            )));
        }

        let write_len_usize = usize::try_from(write_len).unwrap_or(0);
        speed_limiter.throttle(write_len_usize).await;
        file.write_all(&chunk[..write_len_usize])
            .await
            .map_err(|e| {
                non_retryable(segment_failure(
                    segment,
                    offset,
                    &AppErrorPayload::disk_write_failed(format!("Could not write to disk: {e}"))
                        .command_error(),
                ))
            })?;
        offset += write_len;

        if last_emit.elapsed() >= Duration::from_millis(300) {
            let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
            let speed_bps = ((offset - last_bytes) as f64 / elapsed) as i64;
            send_segment_progress(progress_tx, &segment.id, offset, speed_bps)
                .await
                .map_err(non_retryable_attempt)?;
            last_emit = Instant::now();
            last_tick = Instant::now();
            last_bytes = offset;
        }

        if write_len_usize < chunk.len() {
            send_segment_progress(progress_tx, &segment.id, offset, 0)
                .await
                .map_err(non_retryable_attempt)?;
            return Ok(offset);
        }
    }

    file.flush().await.map_err(|e| {
        non_retryable(segment_failure(
            segment,
            offset,
            &format!("Could not flush the temporary file: {e}"),
        ))
    })?;

    let final_end = live_segment_end(live_ends, segment).await;
    if offset <= final_end {
        return Err(retryable(segment_failure(
            segment,
            offset,
            "The download ended before all bytes were received.",
        )));
    }

    send_segment_progress(progress_tx, &segment.id, offset, 0)
        .await
        .map_err(non_retryable_attempt)?;
    Ok(offset)
}

async fn live_segment_end(
    live_ends: &Arc<RwLock<HashMap<String, i64>>>,
    segment: &TaskSegmentRecord,
) -> i64 {
    live_ends
        .read()
        .await
        .get(&segment.id)
        .copied()
        .unwrap_or(segment.range_end)
}

fn retryable(failure: SegmentFailure) -> SegmentAttemptError {
    retryable_with_delay(failure, None)
}

fn retryable_with_delay(
    failure: SegmentFailure,
    retry_after: Option<Duration>,
) -> SegmentAttemptError {
    SegmentAttemptError {
        failure,
        retryable: true,
        retry_after,
    }
}

fn non_retryable(failure: SegmentFailure) -> SegmentAttemptError {
    SegmentAttemptError {
        failure,
        retryable: false,
        retry_after: None,
    }
}

fn non_retryable_attempt(failure: SegmentFailure) -> SegmentAttemptError {
    non_retryable(failure)
}

fn retry_delay(retry_count: i32) -> Duration {
    if std::env::var_os("VIBE_FAST_RETRY_DELAYS").is_some() {
        return match retry_count {
            1 => Duration::from_millis(10),
            2 => Duration::from_millis(20),
            3 => Duration::from_millis(40),
            4 => Duration::from_millis(80),
            _ => Duration::from_millis(150),
        };
    }

    match retry_count {
        1 => Duration::from_secs(1),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(4),
        4 => Duration::from_secs(8),
        _ => Duration::from_secs(15),
    }
}

async fn send_segment_progress(
    progress_tx: &mpsc::Sender<SegmentMessage>,
    segment_id: &str,
    downloaded_until: i64,
    speed_bps: i64,
) -> Result<(), SegmentFailure> {
    progress_tx
        .send(SegmentMessage::Progress {
            segment_id: segment_id.to_string(),
            downloaded_until,
            speed_bps,
        })
        .await
        .map_err(|_| SegmentFailure {
            segment_id: segment_id.to_string(),
            downloaded_until,
            error: "Progress channel closed before the segment completed.".to_string(),
        })
}

async fn send_segment_retry(
    progress_tx: &mpsc::Sender<SegmentMessage>,
    segment_id: &str,
    downloaded_until: i64,
    retry_count: i32,
    error: &str,
) -> Result<(), SegmentFailure> {
    progress_tx
        .send(SegmentMessage::Retry {
            segment_id: segment_id.to_string(),
            downloaded_until,
            retry_count,
            error: error.to_string(),
        })
        .await
        .map_err(|_| SegmentFailure {
            segment_id: segment_id.to_string(),
            downloaded_until,
            error: "Progress channel closed before the segment completed.".to_string(),
        })
}

fn segment_failure(
    segment: &TaskSegmentRecord,
    downloaded_until: i64,
    error: &str,
) -> SegmentFailure {
    SegmentFailure {
        segment_id: segment.id.clone(),
        downloaded_until,
        error: error.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_segment_message(
    app: &AppHandle,
    pool: &SqlitePool,
    task_id: &str,
    total_size: i64,
    connection_count: i32,
    last_speeds: &mut HashMap<String, i64>,
    message: SegmentMessage,
    update_task: bool,
) -> Result<(), String> {
    match message {
        SegmentMessage::Progress {
            segment_id,
            downloaded_until,
            speed_bps,
        } => {
            if update_task {
                db::update_segment_progress(
                    pool,
                    &segment_id,
                    downloaded_until,
                    SegmentStatus::Downloading,
                )
                .await?;
            } else {
                db::update_segment_downloaded_until(pool, &segment_id, downloaded_until).await?;
            }
            last_speeds.insert(segment_id, speed_bps.max(0));
        }
        SegmentMessage::Retry {
            segment_id,
            downloaded_until,
            retry_count,
            error,
        } => {
            db::update_segment_retry(pool, &segment_id, downloaded_until, retry_count, &error)
                .await?;
            last_speeds.insert(segment_id, 0);
            if update_task {
                let segments = db::list_segment_records(pool, task_id).await?;
                let downloaded = db::total_segment_downloaded_bytes(&segments);
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
    }
    Ok(())
}

async fn emit_aggregate_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task_id: &str,
    total_size: i64,
    connection_count: i32,
    last_speeds: &HashMap<String, i64>,
) -> Result<(), String> {
    let segments = db::list_segment_records(pool, task_id).await?;
    let downloaded = db::total_segment_downloaded_bytes(&segments);
    let speed_bps = last_speeds.values().copied().sum::<i64>();
    db::update_task_progress(
        pool,
        task_id,
        downloaded,
        speed_bps,
        connection_count,
        TaskStatus::Downloading,
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
