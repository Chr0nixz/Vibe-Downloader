use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::{
    header::{
        ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
        LAST_MODIFIED, RANGE,
    },
    Client, Response, StatusCode,
};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::mpsc,
    task::JoinSet,
};

use crate::{
    db,
    events::{emit_queue_changed, emit_task_progress},
    models::{SegmentStatus, TaskProgressPayload, TaskRecord, TaskSegmentRecord, TaskStatus},
};

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: String,
    pub file_name: String,
    pub total_size: i64,
    pub supports_range: bool,
    pub source_host: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DirectDownloadRequest {
    pub url: String,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub total_size: i64,
    pub supports_range: bool,
}

#[derive(Debug, Clone)]
pub struct DirectSegmentedDownloadRequest {
    pub url: String,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub total_size: i64,
    pub supports_range: bool,
    pub segments: Vec<TaskSegmentRecord>,
}

#[derive(Debug, Clone)]
pub struct HttpEngine {
    client: Client,
}

impl HttpEngine {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("VibeDownloader/0.1")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

        Ok(Self { client })
    }

    pub async fn probe(&self, url: &str) -> Result<ProbeResult, String> {
        let head = self.client.head(url).send().await;
        if let Ok(response) = head {
            if response.status().is_success() {
                if let Some(probe) = probe_from_response(url, &response, false)? {
                    return Ok(probe);
                }
            }
        }

        let response = self
            .client
            .get(url)
            .header(RANGE, "bytes=0-0")
            .send()
            .await
            .map_err(|e| format!("Could not connect to the server: {e}"))?;

        if !response.status().is_success() {
            return Err(format_http_status(response.status()));
        }

        probe_from_response(
            url,
            &response,
            response.status() == StatusCode::PARTIAL_CONTENT,
        )?
        .ok_or_else(|| "The server did not report a file size.".to_string())
    }

    pub async fn download(
        &self,
        app: AppHandle,
        pool: SqlitePool,
        task: TaskRecord,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        run_segmented_download(&self.client, app, pool, task, cancel).await
    }

    pub async fn download_direct(
        &self,
        request: DirectDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        run_direct_download(&self.client, request, cancel).await
    }

    pub async fn download_segmented_direct(
        &self,
        request: DirectSegmentedDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        run_direct_segmented_download(&self.client, request, cancel).await
    }
}

async fn run_direct_download(
    client: &Client,
    request: DirectDownloadRequest,
    cancel: Arc<AtomicBool>,
) -> Result<i64, String> {
    let resume_from = fs::metadata(&request.temp_path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    if resume_from > 0 && !request.supports_range {
        return Err("Resume unavailable. Restart this download from the beginning.".to_string());
    }

    let mut http_request = client.get(&request.url);
    if resume_from > 0 {
        http_request = http_request.header(RANGE, format!("bytes={resume_from}-"));
    }

    let mut response = http_request
        .send()
        .await
        .map_err(|e| format!("Could not connect to the server: {e}"))?;

    if resume_from > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(
            "Resume unavailable. The server did not honor the byte range request.".to_string(),
        );
    }
    if !response.status().is_success() {
        return Err(format_http_status(response.status()));
    }

    if let Some(parent) = request.temp_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }

    let mut file = if resume_from > 0 {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&request.temp_path)
            .await
            .map_err(|e| format!("Could not open the temporary file: {e}"))?
    } else {
        fs::File::create(&request.temp_path)
            .await
            .map_err(|e| format!("Could not create the temporary file: {e}"))?
    };

    let mut downloaded = resume_from;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("The connection failed while downloading: {e}"))?
    {
        if cancel.load(Ordering::SeqCst) {
            file.flush()
                .await
                .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
            return Ok(downloaded);
        }

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Could not write to disk: {e}"))?;
        downloaded += i64::try_from(chunk.len()).unwrap_or(0);
    }

    file.flush()
        .await
        .map_err(|e| format!("Could not flush the temporary file: {e}"))?;

    if request.total_size > 0 && downloaded < request.total_size {
        return Err("The download ended before all bytes were received.".to_string());
    }

    if fs::try_exists(&request.final_path).await.unwrap_or(false) {
        fs::remove_file(&request.final_path)
            .await
            .map_err(|e| format!("Could not replace the existing file: {e}"))?;
    }
    fs::rename(&request.temp_path, &request.final_path)
        .await
        .map_err(|e| format!("Could not finalize the downloaded file: {e}"))?;

    Ok(downloaded)
}

async fn run_direct_segmented_download(
    client: &Client,
    request: DirectSegmentedDownloadRequest,
    cancel: Arc<AtomicBool>,
) -> Result<i64, String> {
    if request.segments.is_empty() {
        return Err("No download segments were provided.".to_string());
    }

    if let Some(parent) = request.temp_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }

    let initial_downloaded = db::total_segment_downloaded_bytes(&request.segments);
    if initial_downloaded == 0 && fs::try_exists(&request.temp_path).await.unwrap_or(false) {
        fs::remove_file(&request.temp_path)
            .await
            .map_err(|e| format!("Could not reset the temporary file: {e}"))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&request.temp_path)
        .await
        .map_err(|e| format!("Could not create the temporary file: {e}"))?;

    let (progress_tx, mut progress_rx) = mpsc::channel::<SegmentProgressMessage>(64);
    let mut workers = JoinSet::new();
    let segment_count = request.segments.len();
    let mut active_workers = 0_usize;

    for segment in request.segments {
        let offset = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if offset > segment.range_end {
            continue;
        }
        active_workers += 1;
        workers.spawn(download_segment_worker(SegmentWorkerRequest {
            client: client.clone(),
            url: request.url.clone(),
            temp_path: request.temp_path.clone(),
            segment,
            segment_count,
            supports_range: request.supports_range,
            cancel: cancel.clone(),
            progress_tx: progress_tx.clone(),
        }));
    }
    drop(progress_tx);

    while active_workers > 0 {
        tokio::select! {
            Some(_) = progress_rx.recv() => {}
            Some(result) = workers.join_next() => {
                active_workers = active_workers.saturating_sub(1);
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(failure)) => {
                        cancel.store(true, Ordering::SeqCst);
                        workers.abort_all();
                        return Err(failure.error);
                    }
                    Err(error) => {
                        cancel.store(true, Ordering::SeqCst);
                        workers.abort_all();
                        return Err(format!("A download worker stopped unexpectedly: {error}"));
                    }
                }
            }
            else => break,
        }
    }

    if cancel.load(Ordering::SeqCst) {
        return Ok(initial_downloaded);
    }

    let temp_size = fs::metadata(&request.temp_path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .map_err(|e| format!("Could not inspect the temporary file: {e}"))?;
    if request.total_size > 0 && temp_size != request.total_size {
        return Err("The temporary file size does not match the remote file.".to_string());
    }

    if fs::try_exists(&request.final_path).await.unwrap_or(false) {
        fs::remove_file(&request.final_path)
            .await
            .map_err(|e| format!("Could not replace the existing file: {e}"))?;
    }
    fs::rename(&request.temp_path, &request.final_path)
        .await
        .map_err(|e| format!("Could not finalize the downloaded file: {e}"))?;

    Ok(request.total_size)
}

fn probe_from_response(
    original_url: &str,
    response: &Response,
    range_probe: bool,
) -> Result<Option<ProbeResult>, String> {
    let final_url = response.url().to_string();
    let headers = response.headers();
    let content_range_size = headers
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range_total);
    let content_length = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let total_size = content_range_size.or(content_length);

    let Some(total_size) = total_size else {
        return Ok(None);
    };

    let supports_range = range_probe
        || headers
            .get(ACCEPT_RANGES)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("bytes"));

    let source_host = response
        .url()
        .host_str()
        .map(str::to_string)
        .or_else(|| {
            reqwest::Url::parse(original_url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
        })
        .unwrap_or_else(|| "unknown".to_string());

    Ok(Some(ProbeResult {
        file_name: file_name_from_response(response),
        final_url,
        total_size,
        supports_range,
        source_host,
        etag: header_to_string(response, ETAG),
        last_modified: header_to_string(response, LAST_MODIFIED),
        content_type: header_to_string(response, CONTENT_TYPE),
    }))
}

#[derive(Debug)]
struct SegmentProgressMessage {
    segment_id: String,
    downloaded_until: i64,
    speed_bps: i64,
}

#[derive(Debug)]
struct SegmentFailure {
    segment_id: String,
    downloaded_until: i64,
    error: String,
}

async fn run_segmented_download(
    client: &Client,
    app: AppHandle,
    pool: SqlitePool,
    task: TaskRecord,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
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

    if initial_downloaded > 0 && !task.supports_range {
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

    let (progress_tx, mut progress_rx) = mpsc::channel::<SegmentProgressMessage>(64);
    let mut workers = JoinSet::new();
    let url = task.final_url.clone().unwrap_or_else(|| task.url.clone());
    let mut active_workers = 0_usize;

    for segment in segments {
        let offset = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if offset > segment.range_end {
            db::complete_segment(&pool, &segment.id).await?;
            continue;
        }
        db::update_segment_status(
            &pool,
            &segment.id,
            SegmentStatus::Downloading,
            Some(offset),
            None,
        )
        .await?;
        active_workers += 1;
        workers.spawn(download_segment_worker(SegmentWorkerRequest {
            client: client.clone(),
            url: url.clone(),
            temp_path: temp_path_buf.clone(),
            segment,
            segment_count,
            supports_range: task.supports_range,
            cancel: cancel.clone(),
            progress_tx: progress_tx.clone(),
        }));
    }
    drop(progress_tx);

    let mut last_speeds: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut last_emit = Instant::now();

    while active_workers > 0 {
        tokio::select! {
            Some(message) = progress_rx.recv() => {
                db::update_segment_progress(
                    &pool,
                    &message.segment_id,
                    message.downloaded_until,
                    SegmentStatus::Downloading,
                )
                .await?;
                last_speeds.insert(message.segment_id, message.speed_bps.max(0));
                if last_emit.elapsed() >= Duration::from_millis(300) {
                    emit_aggregate_progress(
                        &app,
                        &pool,
                        &task.id,
                        task.total_size,
                        active_connection_count,
                        &last_speeds,
                    )
                    .await?;
                    last_emit = Instant::now();
                }
            }
            Some(result) = workers.join_next() => {
                active_workers = active_workers.saturating_sub(1);
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(failure)) => {
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
            }
            else => break,
        }
    }

    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }

    while let Ok(message) = progress_rx.try_recv() {
        db::update_segment_progress(
            &pool,
            &message.segment_id,
            message.downloaded_until,
            SegmentStatus::Downloading,
        )
        .await?;
        last_speeds.insert(message.segment_id, message.speed_bps.max(0));
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

    if fs::try_exists(&final_path_buf).await.unwrap_or(false) {
        fs::remove_file(&final_path_buf)
            .await
            .map_err(|e| format!("Could not replace the existing file: {e}"))?;
    }
    fs::rename(&temp_path_buf, &final_path_buf)
        .await
        .map_err(|e| format!("Could not finalize the downloaded file: {e}"))?;

    db::complete_task(&pool, &task.id).await?;
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

struct SegmentWorkerRequest {
    client: Client,
    url: String,
    temp_path: PathBuf,
    segment: TaskSegmentRecord,
    segment_count: usize,
    supports_range: bool,
    cancel: Arc<AtomicBool>,
    progress_tx: mpsc::Sender<SegmentProgressMessage>,
}

async fn download_segment_worker(request: SegmentWorkerRequest) -> Result<(), SegmentFailure> {
    let SegmentWorkerRequest {
        client,
        url,
        temp_path,
        segment,
        segment_count,
        supports_range,
        cancel,
        progress_tx,
    } = request;

    let mut offset = segment
        .downloaded_until
        .clamp(segment.range_start, segment.range_end.saturating_add(1));
    let use_range = segment_count > 1 || offset > segment.range_start;

    if use_range && !supports_range {
        return Err(segment_failure(
            &segment,
            offset,
            "Resume unavailable. Restart this download from the beginning.",
        ));
    }

    let mut request = client.get(&url);
    if use_range {
        request = request.header(RANGE, format!("bytes={offset}-{}", segment.range_end));
    }

    let mut response = request.send().await.map_err(|e| {
        segment_failure(
            &segment,
            offset,
            &format!("Could not connect to the server: {e}"),
        )
    })?;

    if !response.status().is_success() {
        return Err(segment_failure(
            &segment,
            offset,
            &format_http_status(response.status()),
        ));
    }
    if use_range && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(segment_failure(
            &segment,
            offset,
            "Resume unavailable. The server did not honor the byte range request.",
        ));
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&temp_path)
        .await
        .map_err(|e| {
            segment_failure(
                &segment,
                offset,
                &format!("Could not open the temporary file: {e}"),
            )
        })?;

    file.seek(std::io::SeekFrom::Start(u64::try_from(offset).unwrap_or(0)))
        .await
        .map_err(|e| {
            segment_failure(
                &segment,
                offset,
                &format!("Could not seek in the temporary file: {e}"),
            )
        })?;

    let mut last_emit = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_bytes = offset;

    while let Some(chunk) = response.chunk().await.map_err(|e| {
        segment_failure(
            &segment,
            offset,
            &format!("The connection failed while downloading: {e}"),
        )
    })? {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await.map_err(|e| {
                segment_failure(
                    &segment,
                    offset,
                    &format!("Could not flush the temporary file: {e}"),
                )
            })?;
            return Ok(());
        }

        let chunk_len = i64::try_from(chunk.len()).unwrap_or(0);
        if offset + chunk_len > segment.range_end + 1 {
            return Err(segment_failure(
                &segment,
                offset,
                "The server sent more bytes than the requested segment range.",
            ));
        }

        file.write_all(&chunk).await.map_err(|e| {
            segment_failure(&segment, offset, &format!("Could not write to disk: {e}"))
        })?;
        offset += chunk_len;

        if last_emit.elapsed() >= Duration::from_millis(300) {
            let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
            let speed_bps = ((offset - last_bytes) as f64 / elapsed) as i64;
            send_segment_progress(&progress_tx, &segment.id, offset, speed_bps).await?;
            last_emit = Instant::now();
            last_tick = Instant::now();
            last_bytes = offset;
        }
    }

    file.flush().await.map_err(|e| {
        segment_failure(
            &segment,
            offset,
            &format!("Could not flush the temporary file: {e}"),
        )
    })?;

    if offset <= segment.range_end {
        return Err(segment_failure(
            &segment,
            offset,
            "The download ended before all bytes were received.",
        ));
    }

    send_segment_progress(&progress_tx, &segment.id, offset, 0).await?;
    Ok(())
}

async fn send_segment_progress(
    progress_tx: &mpsc::Sender<SegmentProgressMessage>,
    segment_id: &str,
    downloaded_until: i64,
    speed_bps: i64,
) -> Result<(), SegmentFailure> {
    progress_tx
        .send(SegmentProgressMessage {
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

async fn emit_aggregate_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task_id: &str,
    total_size: i64,
    connection_count: i32,
    last_speeds: &std::collections::HashMap<String, i64>,
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

fn file_name_from_response(response: &Response) -> String {
    response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .or_else(|| {
            response
                .url()
                .path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| format!("download-{}", chrono::Utc::now().timestamp()))
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    value.split(';').find_map(|part| {
        let trimmed = part.trim();
        let value = if let Some(value) = trimmed.strip_prefix("filename*=") {
            value.strip_prefix("UTF-8''").unwrap_or(value)
        } else {
            trimmed.strip_prefix("filename=")?
        };
        Some(value.trim_matches('"').to_string())
    })
}

fn parse_content_range_total(value: &str) -> Option<i64> {
    value.rsplit_once('/')?.1.parse::<i64>().ok()
}

fn header_to_string(response: &Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn format_http_status(status: StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "The server denied access to this file.".to_string(),
        404 => "The file was not found on the server.".to_string(),
        429 => "The server is limiting requests. Try again later.".to_string(),
        code => format!("The server returned HTTP {code}."),
    }
}
