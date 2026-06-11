use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::SqlitePool;
use suppaftp::{
    tokio::{AsyncFtpStream, AsyncRustlsConnector, AsyncRustlsFtpStream},
    tokio_rustls::rustls::{ClientConfig, RootCertStore},
    types::FileType,
    Mode,
};
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::mpsc,
    task::JoinSet,
};
use uuid::Uuid;

use super::{
    engine::EngineFuture,
    http::file::{finalize_download_file, persist_completed_path},
    DownloadContext, DownloadEngine, GlobalSpeedLimiter, ProbeOutput, ProbeRequest,
};
use crate::{
    db,
    events::{emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        EngineCapabilities, ProbedFile, RequestDiagnosticRecord, SegmentStatus, TaskKind,
        TaskProgressPayload, TaskRecord, TaskSegmentRecord, TaskStatus,
    },
};

const FTP_MAX_DYNAMIC_CONNECTIONS: usize = 4;
const FTP_DYNAMIC_WARMUP: Duration = Duration::from_secs(8);
const FTP_DYNAMIC_INTERVAL: Duration = Duration::from_secs(5);
const FTP_MIN_SPLIT_REMAINING: i64 = 16 * 1024 * 1024;
const FTP_WORKER_RETRIES: i32 = 2;
const FTP_READ_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct FtpEngine;

#[derive(Debug, Clone)]
struct FtpTarget {
    original_uri: String,
    sanitized_uri: String,
    protocol: String,
    mode: FtpSecurityMode,
    host: String,
    port: u16,
    username: String,
    password: String,
    path: String,
    file_name: String,
    source_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FtpSecurityMode {
    Plain,
    ExplicitTls,
    ImplicitTls,
}

enum FtpSession {
    Plain(AsyncFtpStream),
    Secure(AsyncRustlsFtpStream),
}

#[derive(Debug)]
struct SegmentRuntime {
    range_end: Arc<AtomicI64>,
}

#[derive(Debug)]
struct SegmentProgress {
    range_start: i64,
    range_end: i64,
    downloaded_until: i64,
    speed_bps: i64,
    status: SegmentStatus,
    retry_count: i32,
}

#[derive(Debug)]
struct WorkerRequest {
    target: FtpTarget,
    task_id: String,
    segment: TaskSegmentRecord,
    range_end: Arc<AtomicI64>,
    pool: SqlitePool,
    temp_path: PathBuf,
    total_size: i64,
    cancel: Arc<AtomicBool>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    progress_tx: mpsc::UnboundedSender<WorkerProgress>,
}

#[derive(Debug)]
struct WorkerProgress {
    segment_id: String,
    downloaded_until: i64,
    speed_bps: i64,
}

#[derive(Debug)]
struct WorkerFinished {
    segment_id: String,
    result: Result<i64, String>,
}

impl FtpEngine {
    pub fn new() -> Self {
        Self
    }

    async fn probe_target(&self, target: FtpTarget) -> Result<ProbeOutput, String> {
        let mut session = connect_session(&target).await?;
        session.transfer_type(FileType::Binary).await?;
        session.set_mode(Mode::Passive);

        let total_size = session
            .size(&target.path)
            .await
            .map(|size| i64::try_from(size).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let last_modified = session
            .mdtm(&target.path)
            .await
            .ok()
            .map(format_ftp_datetime);
        let supports_resume = total_size > 0 && session.resume_transfer(0).await.is_ok();
        let supports_parallel =
            supports_resume && total_size >= db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES;
        let _ = session.quit().await;

        Ok(ProbeOutput {
            protocol: target.protocol.clone(),
            task_kind: TaskKind::SingleFile,
            resolved_uri: target.original_uri,
            display_name: target.file_name.clone(),
            total_size,
            source_key: target.source_key,
            capabilities: EngineCapabilities {
                supports_resume,
                supports_parallel,
                supports_multi_file: false,
            },
            files: vec![ProbedFile {
                relative_path: target.file_name,
                size: total_size.to_string(),
                content_type: None,
            }],
            etag: None,
            last_modified,
            content_type: None,
        })
    }

    async fn run_download(&self, context: DownloadContext) -> Result<(), String> {
        let target = FtpTarget::parse(
            context
                .task
                .final_url
                .as_deref()
                .unwrap_or(&context.task.url),
        )?;
        run_ftp_download(target, context).await
    }
}

impl Default for FtpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine for FtpEngine {
    fn id(&self) -> &'static str {
        "ftp"
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "ftp" | "ftps")
    }

    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>> {
        Box::pin(async move {
            let target = FtpTarget::parse(&request.uri)?;
            tracing::debug!(url = %target.sanitized_uri, "probing ftp url");
            self.probe_target(target).await
        })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move { self.run_download(context).await })
    }
}

async fn run_ftp_download(target: FtpTarget, context: DownloadContext) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel,
        speed_limiter,
        connection_limit,
        request_headers: _,
    } = context;

    let temp_path = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Task is missing a temporary path.".to_string())?;
    let final_path = task
        .final_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Task is missing a final path.".to_string())?;
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }

    let mut segments = db::ensure_task_segments(&pool, &task).await?;
    if segments.is_empty() {
        return Err("Task segment could not be created.".to_string());
    }
    if segments
        .iter()
        .all(|segment| segment.downloaded_until <= segment.range_start)
        && fs::try_exists(&temp_path).await.unwrap_or(false)
    {
        fs::remove_file(&temp_path)
            .await
            .map_err(|e| format!("Could not reset the temporary file: {e}"))?;
    }

    let max_connections = if task.supports_parallel {
        connection_limit.clamp(1, FTP_MAX_DYNAMIC_CONNECTIONS)
    } else {
        1
    };

    let unknown_size_segment_id = segments.first().map(|segment| segment.id.clone());
    let mut progress = progress_from_segments(&segments);
    let mut pending: VecDeque<TaskSegmentRecord> = segments
        .drain(..)
        .filter(|segment| segment.downloaded_until <= segment.range_end)
        .collect();
    let mut running: HashMap<String, SegmentRuntime> = HashMap::new();
    let mut workers = JoinSet::new();
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let started_at = Instant::now();
    let mut last_split_at = Instant::now();
    let mut acceleration_disabled = false;
    let mut dynamic_failures = 0_i32;

    start_next_ftp_worker(
        &target,
        &task,
        &pool,
        &temp_path,
        &cancel,
        &speed_limiter,
        &progress_tx,
        &mut pending,
        &mut running,
        &mut workers,
    );
    emit_ftp_progress(&app, &pool, &task, &progress, running.len()).await?;

    while !pending.is_empty() || !running.is_empty() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }

        tokio::select! {
            Some(message) = progress_rx.recv() => {
                if let Some(segment) = progress.get_mut(&message.segment_id) {
                    segment.downloaded_until = message.downloaded_until;
                    segment.speed_bps = message.speed_bps;
                    segment.status = SegmentStatus::Downloading;
                }
                emit_ftp_progress(&app, &pool, &task, &progress, running.len()).await?;
            }
            Some(joined) = workers.join_next(), if !running.is_empty() => {
                let finished = joined.map_err(|error| format!("A FTP worker stopped unexpectedly: {error}"))?;
                running.remove(&finished.segment_id);
                match finished.result {
                    Ok(downloaded_until) => {
                        if let Some(segment) = progress.get_mut(&finished.segment_id) {
                            segment.downloaded_until = downloaded_until;
                            segment.speed_bps = 0;
                            segment.status = SegmentStatus::Completed;
                        }
                    }
                    Err(error) if cancel.load(Ordering::SeqCst) => return Ok(()),
                    Err(error) => {
                        let progress_len = progress.len();
                        let Some(segment) = progress.get_mut(&finished.segment_id) else {
                            return Err(error);
                        };
                        segment.status = SegmentStatus::Pending;
                        segment.retry_count += 1;
                        segment.speed_bps = 0;
                        db::update_segment_retry(
                            &pool,
                            &finished.segment_id,
                            segment.downloaded_until,
                            segment.retry_count,
                            &error,
                        ).await?;
                        let dynamic_segment = segment.range_start > 0 || progress_len > 1;
                        if dynamic_segment {
                            dynamic_failures += 1;
                        }
                        if dynamic_failures >= 2 && !acceleration_disabled {
                            acceleration_disabled = true;
                            db::update_task_status(
                                &pool,
                                &task.id,
                                TaskStatus::Downloading,
                                0,
                                1,
                                Some("FTP acceleration disabled; continuing with one connection."),
                                None,
                            ).await?;
                            db::insert_task_event(&pool, &task.id, "ftp_acceleration_disabled", Some(&error)).await?;
                        }
                        if segment.retry_count > FTP_WORKER_RETRIES {
                            return Err(error);
                        }
                        pending.push_front(TaskSegmentRecord {
                            id: finished.segment_id.clone(),
                            task_id: task.id.clone(),
                            file_id: None,
                            unit_kind: "ftp_rest".to_string(),
                            range_start: segment.range_start,
                            range_end: segment.range_end,
                            downloaded_until: segment.downloaded_until,
                            speed_bps: 0,
                            status: SegmentStatus::Pending,
                            retry_count: segment.retry_count,
                            last_error: Some(error),
                        });
                    }
                }
                emit_ftp_progress(&app, &pool, &task, &progress, running.len()).await?;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }

        let effective_max = if acceleration_disabled {
            1
        } else {
            max_connections
        };
        while running.len() < effective_max && !pending.is_empty() {
            start_next_ftp_worker(
                &target,
                &task,
                &pool,
                &temp_path,
                &cancel,
                &speed_limiter,
                &progress_tx,
                &mut pending,
                &mut running,
                &mut workers,
            );
        }

        if !acceleration_disabled
            && task.supports_parallel
            && task.total_size > 0
            && running.len() < max_connections
            && pending.is_empty()
            && started_at.elapsed() >= FTP_DYNAMIC_WARMUP
            && last_split_at.elapsed() >= FTP_DYNAMIC_INTERVAL
        {
            if let Some((head, tail)) = split_largest_remaining_segment(&task, &mut progress) {
                if let Some(runtime) = running.get(&head.id) {
                    runtime.range_end.store(head.range_end, Ordering::SeqCst);
                }
                db::update_segment_range_end(&pool, &head.id, head.range_end).await?;
                db::insert_segment_record(&pool, &tail).await?;
                progress.insert(
                    tail.id.clone(),
                    SegmentProgress {
                        range_start: tail.range_start,
                        range_end: tail.range_end,
                        downloaded_until: tail.downloaded_until,
                        speed_bps: 0,
                        status: SegmentStatus::Pending,
                        retry_count: 0,
                    },
                );
                pending.push_back(tail);
                last_split_at = Instant::now();
                db::insert_task_event(&pool, &task.id, "ftp_segment_split", None).await?;
            }
        }
    }

    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }

    let downloaded = total_downloaded_from_progress(&progress, task.total_size);
    if task.total_size > 0 && downloaded < task.total_size {
        return Err("The download ended before all bytes were received.".to_string());
    }

    let completed_path = finalize_download_file(&temp_path, &final_path).await?;
    persist_completed_path(&pool, &task.id, &completed_path).await?;
    if task.total_size > 0 {
        db::complete_task(&pool, &task.id).await?;
    } else {
        let segment_id = unknown_size_segment_id
            .as_deref()
            .ok_or_else(|| "Task segment could not be completed.".to_string())?;
        db::complete_unknown_size_task(&pool, &task.id, segment_id, downloaded).await?;
    }
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn start_next_ftp_worker(
    target: &FtpTarget,
    task: &TaskRecord,
    pool: &SqlitePool,
    temp_path: &Path,
    cancel: &Arc<AtomicBool>,
    speed_limiter: &Arc<GlobalSpeedLimiter>,
    progress_tx: &mpsc::UnboundedSender<WorkerProgress>,
    pending: &mut VecDeque<TaskSegmentRecord>,
    running: &mut HashMap<String, SegmentRuntime>,
    workers: &mut JoinSet<WorkerFinished>,
) {
    let Some(segment) = pending.pop_front() else {
        return;
    };
    let range_end = Arc::new(AtomicI64::new(segment.range_end));
    running.insert(
        segment.id.clone(),
        SegmentRuntime {
            range_end: range_end.clone(),
        },
    );
    let request = WorkerRequest {
        target: target.clone(),
        task_id: task.id.clone(),
        segment,
        range_end,
        pool: pool.clone(),
        temp_path: temp_path.to_path_buf(),
        total_size: task.total_size,
        cancel: cancel.clone(),
        speed_limiter: speed_limiter.clone(),
        progress_tx: progress_tx.clone(),
    };
    workers.spawn(async move {
        let segment_id = request.segment.id.clone();
        let result = download_ftp_segment(request).await;
        WorkerFinished { segment_id, result }
    });
}

async fn download_ftp_segment(request: WorkerRequest) -> Result<i64, String> {
    let started = Instant::now();
    let range_label = if request.total_size > 0 {
        Some(format!(
            "REST {}-{}",
            request.segment.downloaded_until, request.segment.range_end
        ))
    } else {
        None
    };
    let result = download_ftp_segment_inner(&request).await;
    persist_ftp_diagnostic(
        &request.pool,
        &request.task_id,
        &request.target.sanitized_uri,
        range_label,
        result.as_ref().err().map(String::as_str),
        started.elapsed(),
    )
    .await;
    result
}

async fn download_ftp_segment_inner(request: &WorkerRequest) -> Result<i64, String> {
    let mut session = connect_session(&request.target).await?;
    session.transfer_type(FileType::Binary).await?;
    session.set_mode(Mode::Passive);

    let mut offset = request
        .segment
        .downloaded_until
        .max(request.segment.range_start);
    if offset > 0 {
        session
            .resume_transfer(usize::try_from(offset).map_err(|_| "FTP offset is too large.")?)
            .await?;
    }
    db::update_segment_status(
        &request.pool,
        &request.segment.id,
        SegmentStatus::Downloading,
        None,
        None,
    )
    .await?;

    let mut remote_stream = session.retr_as_stream(&request.target.path).await?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&request.temp_path)
        .await
        .map_err(|e| format!("Could not open the temporary file: {e}"))?;
    file.seek(std::io::SeekFrom::Start(u64::try_from(offset).unwrap_or(0)))
        .await
        .map_err(|e| format!("Could not seek in the temporary file: {e}"))?;

    let mut buffer = vec![0_u8; FTP_READ_BUFFER_SIZE];
    let mut last_emit = Instant::now();
    let mut last_emit_bytes = offset;
    loop {
        if request.cancel.load(Ordering::SeqCst) {
            let _ = session.abort(remote_stream).await;
            return Err("Download canceled.".to_string());
        }

        let current_end = request.range_end.load(Ordering::SeqCst);
        let read_len = if request.total_size > 0 {
            if offset > current_end {
                break;
            }
            usize::try_from((current_end - offset + 1).min(FTP_READ_BUFFER_SIZE as i64))
                .unwrap_or(FTP_READ_BUFFER_SIZE)
        } else {
            FTP_READ_BUFFER_SIZE
        };
        let read = remote_stream
            .read(&mut buffer[..read_len])
            .await
            .map_err(|e| format!("The FTP connection failed while downloading: {e}"))?;
        if read == 0 {
            if request.total_size > 0 && offset <= current_end {
                return Err("The FTP download ended before all bytes were received.".to_string());
            }
            break;
        }

        request.speed_limiter.throttle(read).await;
        file.write_all(&buffer[..read])
            .await
            .map_err(|e| format!("Could not write to the temporary file: {e}"))?;
        offset += i64::try_from(read).unwrap_or(0);

        if last_emit.elapsed() >= Duration::from_millis(500) {
            let elapsed = last_emit.elapsed().as_secs_f64().max(0.001);
            let speed = ((offset - last_emit_bytes) as f64 / elapsed) as i64;
            let _ = request.progress_tx.send(WorkerProgress {
                segment_id: request.segment.id.clone(),
                downloaded_until: offset,
                speed_bps: speed,
            });
            db::update_segment_runtime_progress(
                &request.pool,
                &request.segment.id,
                offset,
                speed,
                SegmentStatus::Downloading,
            )
            .await?;
            last_emit = Instant::now();
            last_emit_bytes = offset;
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
    let final_end = request.range_end.load(Ordering::SeqCst);
    if request.total_size > 0 && final_end < request.total_size.saturating_sub(1) {
        let _ = session.abort(remote_stream).await;
    } else {
        session.finalize_retr_stream(remote_stream).await?;
    }
    let _ = session.quit().await;
    db::update_segment_progress(
        &request.pool,
        &request.segment.id,
        offset,
        SegmentStatus::Completed,
    )
    .await?;
    let _ = request.progress_tx.send(WorkerProgress {
        segment_id: request.segment.id.clone(),
        downloaded_until: offset,
        speed_bps: 0,
    });
    Ok(offset)
}

async fn emit_ftp_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    progress: &HashMap<String, SegmentProgress>,
    active_connections: usize,
) -> Result<(), String> {
    let downloaded = total_downloaded_from_progress(progress, task.total_size);
    let speed = progress.values().map(|segment| segment.speed_bps).sum();
    let connection_count = i32::try_from(active_connections).unwrap_or(i32::MAX);
    db::update_task_progress(
        pool,
        &task.id,
        downloaded,
        speed,
        connection_count,
        TaskStatus::Downloading,
    )
    .await?;
    emit_task_progress(
        app,
        &TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: downloaded.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: speed.to_string(),
            connection_count,
            status: TaskStatus::Downloading,
        },
    );
    Ok(())
}

async fn persist_ftp_diagnostic(
    pool: &SqlitePool,
    task_id: &str,
    url: &str,
    range_header: Option<String>,
    error: Option<&str>,
    duration: Duration,
) {
    let record = RequestDiagnosticRecord {
        task_id: task_id.to_string(),
        method: "FTP RETR".to_string(),
        url: sanitize_url(url),
        range_header,
        status_code: if error.is_some() { None } else { Some(226) },
        etag: None,
        last_modified: None,
        content_length: None,
        error_message: error.map(str::to_string),
        retry_count: 0,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    };
    if let Err(error) = db::insert_request_diagnostic(pool, &record).await {
        tracing::warn!(task_id, error = %error, "failed to persist ftp diagnostic");
    }
}

fn progress_from_segments(segments: &[TaskSegmentRecord]) -> HashMap<String, SegmentProgress> {
    segments
        .iter()
        .map(|segment| {
            (
                segment.id.clone(),
                SegmentProgress {
                    range_start: segment.range_start,
                    range_end: segment.range_end,
                    downloaded_until: segment.downloaded_until,
                    speed_bps: segment.speed_bps,
                    status: segment.status,
                    retry_count: segment.retry_count,
                },
            )
        })
        .collect()
}

fn total_downloaded_from_progress(
    progress: &HashMap<String, SegmentProgress>,
    total_size: i64,
) -> i64 {
    progress
        .values()
        .map(|segment| {
            if total_size <= 0 {
                segment
                    .downloaded_until
                    .max(segment.range_start)
                    .saturating_sub(segment.range_start)
            } else {
                segment
                    .downloaded_until
                    .clamp(segment.range_start, segment.range_end.saturating_add(1))
                    .saturating_sub(segment.range_start)
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_size_progress_uses_actual_downloaded_until() {
        let mut progress = HashMap::new();
        progress.insert(
            "segment".to_string(),
            SegmentProgress {
                range_start: 0,
                range_end: 0,
                downloaded_until: 8192,
                speed_bps: 0,
                status: SegmentStatus::Completed,
                retry_count: 0,
            },
        );

        assert_eq!(total_downloaded_from_progress(&progress, 0), 8192);
    }

    #[test]
    fn known_size_progress_is_clamped_to_segment_range() {
        let mut progress = HashMap::new();
        progress.insert(
            "segment".to_string(),
            SegmentProgress {
                range_start: 0,
                range_end: 99,
                downloaded_until: 150,
                speed_bps: 0,
                status: SegmentStatus::Completed,
                retry_count: 0,
            },
        );

        assert_eq!(total_downloaded_from_progress(&progress, 100), 100);
    }
}

fn split_largest_remaining_segment(
    task: &TaskRecord,
    progress: &mut HashMap<String, SegmentProgress>,
) -> Option<(TaskSegmentRecord, TaskSegmentRecord)> {
    let (segment_id, split) = progress
        .iter()
        .filter(|(_, segment)| segment.status == SegmentStatus::Downloading)
        .filter_map(|(id, segment)| planned_ftp_split(segment).map(|split| (id.clone(), split)))
        .max_by_key(|(_, split)| split.tail_end - split.tail_start)?;

    let head_progress = progress.get_mut(&segment_id)?;
    let original_end = head_progress.range_end;
    head_progress.range_end = split.head_end;
    let head = TaskSegmentRecord {
        id: segment_id.clone(),
        task_id: task.id.clone(),
        file_id: None,
        unit_kind: "ftp_rest".to_string(),
        range_start: head_progress.range_start,
        range_end: split.head_end,
        downloaded_until: head_progress.downloaded_until,
        speed_bps: head_progress.speed_bps,
        status: SegmentStatus::Downloading,
        retry_count: head_progress.retry_count,
        last_error: None,
    };
    let tail = TaskSegmentRecord {
        id: Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        file_id: None,
        unit_kind: "ftp_rest".to_string(),
        range_start: split.tail_start,
        range_end: original_end,
        downloaded_until: split.tail_start,
        speed_bps: 0,
        status: SegmentStatus::Pending,
        retry_count: 0,
        last_error: None,
    };
    Some((head, tail))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FtpSplit {
    head_end: i64,
    tail_start: i64,
    tail_end: i64,
}

fn planned_ftp_split(segment: &SegmentProgress) -> Option<FtpSplit> {
    let current = segment
        .downloaded_until
        .clamp(segment.range_start, segment.range_end.saturating_add(1));
    let remaining = segment.range_end.saturating_sub(current).saturating_add(1);
    if remaining < FTP_MIN_SPLIT_REMAINING.saturating_mul(2) {
        return None;
    }
    let tail_start = current + remaining / 2;
    if tail_start <= current || tail_start > segment.range_end {
        return None;
    }
    Some(FtpSplit {
        head_end: tail_start - 1,
        tail_start,
        tail_end: segment.range_end,
    })
}

async fn connect_session(target: &FtpTarget) -> Result<FtpSession, String> {
    let addr = format!("{}:{}", target.host, target.port);
    let result = match target.mode {
        FtpSecurityMode::Plain => {
            let mut session = AsyncFtpStream::connect(addr)
                .await
                .map_err(ftp_connect_error)?;
            session
                .login(&target.username, &target.password)
                .await
                .map_err(ftp_connect_error)?;
            Ok(FtpSession::Plain(session))
        }
        FtpSecurityMode::ExplicitTls => {
            let connector = rustls_connector();
            let session = AsyncRustlsFtpStream::connect(addr)
                .await
                .map_err(ftp_connect_error)?;
            let mut session = session
                .into_secure(connector, &target.host)
                .await
                .map_err(ftp_connect_error)?;
            session
                .login(&target.username, &target.password)
                .await
                .map_err(ftp_connect_error)?;
            Ok(FtpSession::Secure(session))
        }
        FtpSecurityMode::ImplicitTls => {
            let connector = rustls_connector();
            let mut session =
                AsyncRustlsFtpStream::connect_secure_implicit(addr, connector, &target.host)
                    .await
                    .map_err(ftp_connect_error)?;
            session
                .login(&target.username, &target.password)
                .await
                .map_err(ftp_connect_error)?;
            Ok(FtpSession::Secure(session))
        }
    };
    result
}

fn ftp_connect_error(error: suppaftp::FtpError) -> String {
    format!("FTP connection failed: {error}")
}

fn rustls_connector() -> AsyncRustlsConnector {
    let root_store = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    AsyncRustlsConnector::from(suppaftp::tokio_rustls::TlsConnector::from(Arc::new(config)))
}

impl FtpSession {
    async fn transfer_type(&mut self, file_type: FileType) -> Result<(), String> {
        match self {
            Self::Plain(session) => session.transfer_type(file_type).await,
            Self::Secure(session) => session.transfer_type(file_type).await,
        }
        .map_err(|e| format!("FTP transfer mode failed: {e}"))
    }

    fn set_mode(&mut self, mode: Mode) {
        match self {
            Self::Plain(session) => session.set_mode(mode),
            Self::Secure(session) => session.set_mode(mode),
        }
    }

    async fn size(&mut self, path: &str) -> Result<usize, String> {
        match self {
            Self::Plain(session) => session.size(path).await,
            Self::Secure(session) => session.size(path).await,
        }
        .map_err(|e| format!("FTP SIZE failed: {e}"))
    }

    async fn mdtm(&mut self, path: &str) -> Result<NaiveDateTime, String> {
        match self {
            Self::Plain(session) => session.mdtm(path).await,
            Self::Secure(session) => session.mdtm(path).await,
        }
        .map_err(|e| format!("FTP MDTM failed: {e}"))
    }

    async fn resume_transfer(&mut self, offset: usize) -> Result<(), String> {
        match self {
            Self::Plain(session) => session.resume_transfer(offset).await,
            Self::Secure(session) => session.resume_transfer(offset).await,
        }
        .map_err(|e| format!("FTP REST failed: {e}"))
    }

    async fn retr_as_stream(&mut self, path: &str) -> Result<SessionDataStream, String> {
        match self {
            Self::Plain(session) => session
                .retr_as_stream(path)
                .await
                .map(SessionDataStream::Plain),
            Self::Secure(session) => session
                .retr_as_stream(path)
                .await
                .map(SessionDataStream::Secure),
        }
        .map_err(|e| format!("FTP RETR failed: {e}"))
    }

    async fn finalize_retr_stream(&mut self, stream: SessionDataStream) -> Result<(), String> {
        match (self, stream) {
            (Self::Plain(session), SessionDataStream::Plain(stream)) => {
                session.finalize_retr_stream(stream).await
            }
            (Self::Secure(session), SessionDataStream::Secure(stream)) => {
                session.finalize_retr_stream(stream).await
            }
            _ => return Err("FTP stream type mismatch.".to_string()),
        }
        .map_err(|e| format!("FTP transfer finalization failed: {e}"))
    }

    async fn abort(&mut self, stream: SessionDataStream) -> Result<(), String> {
        match (self, stream) {
            (Self::Plain(session), SessionDataStream::Plain(stream)) => session.abort(stream).await,
            (Self::Secure(session), SessionDataStream::Secure(stream)) => {
                session.abort(stream).await
            }
            _ => return Err("FTP stream type mismatch.".to_string()),
        }
        .map_err(|e| format!("FTP transfer abort failed: {e}"))
    }

    async fn quit(&mut self) -> Result<(), String> {
        match self {
            Self::Plain(session) => session.quit().await,
            Self::Secure(session) => session.quit().await,
        }
        .map_err(|e| format!("FTP quit failed: {e}"))
    }
}

enum SessionDataStream {
    Plain(suppaftp::tokio::AsyncDataStream<suppaftp::tokio::AsyncNoTlsStream>),
    Secure(suppaftp::tokio::AsyncDataStream<suppaftp::tokio::AsyncRustlsStream>),
}

impl tokio::io::AsyncRead for SessionDataStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            Self::Secure(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl FtpTarget {
    fn parse(input: &str) -> Result<Self, String> {
        let parsed =
            reqwest::Url::parse(input.trim()).map_err(|_| "FTP URL is invalid.".to_string())?;
        let protocol = parsed.scheme().to_ascii_lowercase();
        if !matches!(protocol.as_str(), "ftp" | "ftps") {
            return Err(format!(
                "The {} protocol is not supported by the FTP engine.",
                protocol
            ));
        }
        let host = parsed
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or_else(|| "FTP URL is missing a host.".to_string())?
            .to_string();
        let mode = match protocol.as_str() {
            "ftp" => FtpSecurityMode::Plain,
            "ftps" if parsed.port() == Some(21) => FtpSecurityMode::ExplicitTls,
            "ftps" => FtpSecurityMode::ImplicitTls,
            _ => FtpSecurityMode::Plain,
        };
        let port = parsed.port().unwrap_or(match mode {
            FtpSecurityMode::ImplicitTls => 990,
            _ => 21,
        });
        let path = percent_decode_path(parsed.path());
        if path.trim_matches('/').is_empty() || path.ends_with('/') {
            return Err("FTP URL must point to a file, not a directory.".to_string());
        }
        let file_name = path
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or("download")
            .to_string();
        let username = if parsed.username().is_empty() {
            "anonymous".to_string()
        } else {
            percent_decode_path(parsed.username())
        };
        let password = parsed
            .password()
            .map(percent_decode_path)
            .unwrap_or_default();
        let source_key = format!("{protocol}://{host}:{port}");

        Ok(Self {
            original_uri: parsed.to_string(),
            sanitized_uri: sanitize_url(parsed.as_str()),
            protocol,
            mode,
            host,
            port,
            username,
            password,
            path,
            file_name,
            source_key,
        })
    }
}

fn percent_decode_path(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(hex);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn format_ftp_datetime(value: NaiveDateTime) -> String {
    DateTime::<Utc>::from_naive_utc_and_offset(value, Utc).to_rfc3339()
}
