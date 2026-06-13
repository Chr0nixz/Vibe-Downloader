use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::Client;
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::{mpsc, RwLock},
    task::JoinSet,
};

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
    let live_ends = Arc::new(RwLock::new(
        segments
            .iter()
            .map(|segment| (segment.id.clone(), segment.range_end))
            .collect::<HashMap<_, _>>(),
    ));
    let mut pending_segments = VecDeque::new();
    let if_range = if_range_header_value(&task);

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
        &request_headers,
        task.total_size,
        if_range.as_deref(),
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
                    SegmentMessageContext {
                        app: &app,
                        pool: &pool,
                        task_id: &task.id,
                        total_size: task.total_size,
                        connection_count: active_connection_count,
                        update_task: !cancel.load(Ordering::SeqCst),
                    },
                    &mut last_speeds,
                    message,
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
                    &request_headers,
                    if_range.as_deref(),
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
            SegmentMessageContext {
                app: &app,
                pool: &pool,
                task_id: &task.id,
                total_size: task.total_size,
                connection_count: active_connection_count,
                update_task: false,
            },
            &mut last_speeds,
            message,
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
    live_ends: &Arc<RwLock<HashMap<String, i64>>>,
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
            live_ends: live_ends.clone(),
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
    last_speeds: &HashMap<String, i64>,
    live_ends: &Arc<RwLock<HashMap<String, i64>>>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    started_at: Instant,
    max_worker_count: usize,
    request_headers: &[(String, String)],
    if_range: Option<&str>,
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
        task_id: split.tail_segment.task_id.clone(),
        url: url.to_string(),
        temp_path: temp_path.to_path_buf(),
        segment: split.tail_segment,
        total_size: task.total_size,
        segment_count: *active_connection_count as usize,
        supports_parallel: task.supports_parallel,
        cancel: cancel.clone(),
        progress_tx: progress_tx.clone(),
        live_ends: live_ends.clone(),
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
    last_speeds: &mut HashMap<String, i64>,
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
            if update_task {
                db::update_segment_runtime_progress(
                    pool,
                    &segment_id,
                    downloaded_until,
                    speed_bps,
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
            let payload = format!("{segment_id}: {error}");
            db::insert_task_event(pool, task_id, "retrying", Some(&payload)).await?;
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
