use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::{
    header::{ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE},
    Client, Response, StatusCode,
};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{fs, io::AsyncWriteExt};

use crate::{
    db,
    events::{emit_queue_changed, emit_task_progress},
    models::{TaskProgressPayload, TaskRecord, TaskStatus},
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

        probe_from_response(url, &response, response.status() == StatusCode::PARTIAL_CONTENT)?
            .ok_or_else(|| "The server did not report a file size.".to_string())
    }

    pub async fn download(
        &self,
        app: AppHandle,
        pool: SqlitePool,
        task: TaskRecord,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        run_single_connection_download(&self.client, app, pool, task, cancel).await
    }

    pub async fn download_direct(
        &self,
        request: DirectDownloadRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<i64, String> {
        run_direct_download(&self.client, request, cancel).await
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
        return Err("Resume unavailable. The server did not honor the byte range request.".to_string());
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

async fn run_single_connection_download(
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
    let segment = db::ensure_single_segment_for_task(&pool, &task).await?;
    let disk_downloaded = fs::metadata(&temp_path_buf)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let resume_from = task
        .downloaded_bytes
        .max(segment.downloaded_until)
        .max(disk_downloaded);

    let resume_from = if resume_from > 0 {
        if !task.supports_range {
            return Err("Resume unavailable. Restart this download from the beginning.".to_string());
        }
        resume_from
    } else {
        0
    };

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
        crate::models::SegmentStatus::Downloading,
        Some(resume_from),
        None,
    )
    .await?;
    emit_progress(&app, &task.id, resume_from, task.total_size, 0, 1, TaskStatus::Downloading);

    let mut request = client.get(task.final_url.as_deref().unwrap_or(&task.url));
    if resume_from > 0 {
        request = request.header(RANGE, format!("bytes={resume_from}-"));
    }

    let mut response = request
        .send()
        .await
        .map_err(|e| format!("Could not connect to the server: {e}"))?;

    if resume_from > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err("Resume unavailable. The server did not honor the byte range request.".to_string());
    }
    if !response.status().is_success() {
        return Err(format_http_status(response.status()));
    }

    if let Some(parent) = temp_path_buf.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }

    let mut file = if resume_from > 0 {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&temp_path_buf)
            .await
            .map_err(|e| format!("Could not open the temporary file: {e}"))?
    } else {
        fs::File::create(&temp_path_buf)
            .await
            .map_err(|e| format!("Could not create the temporary file: {e}"))?
    };

    let mut downloaded = resume_from;
    let mut last_emit = Instant::now();
    let mut last_bytes = downloaded;
    let mut last_tick = Instant::now();

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("The connection failed while downloading: {e}"))?
    {
        if cancel.load(Ordering::SeqCst) {
            file.flush()
                .await
                .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
            return Ok(());
        }

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Could not write to disk: {e}"))?;
        downloaded += i64::try_from(chunk.len()).unwrap_or(0);

        if last_emit.elapsed() >= Duration::from_millis(300) {
            let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
            let speed_bps = ((downloaded - last_bytes) as f64 / elapsed) as i64;
            db::update_task_and_segment_progress(
                &pool,
                &task.id,
                &segment.id,
                downloaded,
                speed_bps.max(0),
                1,
                TaskStatus::Downloading,
            )
            .await?;
            emit_progress(
                &app,
                &task.id,
                downloaded,
                task.total_size,
                speed_bps.max(0),
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

    if task.total_size > 0 && downloaded < task.total_size {
        return Err("The download ended before all bytes were received.".to_string());
    }

    let final_path_buf = PathBuf::from(&final_path);
    if fs::try_exists(&final_path_buf).await.unwrap_or(false) {
        fs::remove_file(&final_path_buf)
            .await
            .map_err(|e| format!("Could not replace the existing file: {e}"))?;
    }
    fs::rename(&temp_path_buf, &final_path_buf)
        .await
        .map_err(|e| format!("Could not finalize the downloaded file: {e}"))?;

    db::complete_task_segment(&pool, &task.id, &segment.id).await?;
    emit_progress(&app, &task.id, task.total_size, task.total_size, 0, 0, TaskStatus::Completed);
    emit_queue_changed(&app);

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
