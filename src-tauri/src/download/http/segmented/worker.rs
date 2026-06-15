use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use reqwest::{
    header::{ACCEPT_ENCODING, CONTENT_RANGE, IF_RANGE, RANGE},
    Client, StatusCode,
};
use tokio::{
    fs,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::mpsc,
};

use super::super::{
    error::format_http_status,
    request::{apply_forwarded_headers, is_retryable_status, retry_after_duration},
};
use super::{
    diagnostics::{error_diagnostic_record, parse_content_range, response_diagnostic_record},
    SegmentFailure, SegmentMessage, MAX_SEGMENT_RETRIES,
};
use crate::{
    download::GlobalSpeedLimiter,
    models::{AppErrorPayload, RequestDiagnosticRecord, TaskSegmentRecord},
};

pub(in crate::download::http) struct SegmentWorkerRequest {
    pub(in crate::download::http) client: Client,
    pub(in crate::download::http) task_id: String,
    pub(in crate::download::http) url: String,
    pub(in crate::download::http) temp_path: PathBuf,
    pub(in crate::download::http) segment: TaskSegmentRecord,
    pub(in crate::download::http) total_size: i64,
    pub(in crate::download::http) segment_count: usize,
    pub(in crate::download::http) supports_parallel: bool,
    pub(in crate::download::http) cancel: Arc<AtomicBool>,
    pub(in crate::download::http) progress_tx: mpsc::Sender<SegmentMessage>,
    pub(in crate::download::http) range_end: Arc<AtomicI64>,
    pub(in crate::download::http) speed_limiter: Arc<GlobalSpeedLimiter>,
    pub(in crate::download::http) request_headers: Vec<(String, String)>,
    pub(in crate::download::http) if_range: Option<String>,
}

pub(in crate::download::http) async fn download_segment_worker(
    request: SegmentWorkerRequest,
) -> Result<(), SegmentFailure> {
    let SegmentWorkerRequest {
        client,
        task_id,
        url,
        temp_path,
        segment,
        total_size,
        segment_count,
        supports_parallel,
        cancel,
        progress_tx,
        range_end,
        speed_limiter,
        request_headers,
        if_range,
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
            task_id: &task_id,
            url: &url,
            temp_path: &temp_path,
            segment: &segment,
            total_size,
            segment_count,
            supports_parallel,
            cancel: &cancel,
            progress_tx: &progress_tx,
            range_end: &range_end,
            speed_limiter: &speed_limiter,
            request_headers: &request_headers,
            if_range: if_range.as_deref(),
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
    task_id: &'a str,
    url: &'a str,
    temp_path: &'a Path,
    segment: &'a TaskSegmentRecord,
    total_size: i64,
    segment_count: usize,
    supports_parallel: bool,
    cancel: &'a Arc<AtomicBool>,
    progress_tx: &'a mpsc::Sender<SegmentMessage>,
    range_end: &'a Arc<AtomicI64>,
    speed_limiter: &'a Arc<GlobalSpeedLimiter>,
    request_headers: &'a [(String, String)],
    if_range: Option<&'a str>,
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
        task_id,
        url,
        temp_path,
        segment,
        total_size,
        segment_count,
        supports_parallel,
        cancel,
        progress_tx,
        range_end,
        speed_limiter,
        request_headers,
        if_range,
        mut offset,
    } = request;

    let initial_range_end = range_end.load(Ordering::SeqCst);
    if offset > initial_range_end {
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

    let mut http_request = apply_forwarded_headers(client.get(url), request_headers)
        .header(ACCEPT_ENCODING, "identity");
    let range_header = if use_range {
        Some(format!("bytes={offset}-{initial_range_end}"))
    } else {
        None
    };
    if use_range {
        http_request = http_request.header(RANGE, range_header.as_deref().unwrap_or_default());
        if let Some(if_range) = if_range {
            http_request = http_request.header(IF_RANGE, if_range);
        }
    }

    let started_at = Instant::now();
    let mut response = match http_request.send().await {
        Ok(response) => {
            let record = response_diagnostic_record(
                task_id,
                "GET",
                url,
                range_header.clone(),
                use_range.then(|| if_range.map(str::to_string)).flatten(),
                &response,
                segment.retry_count,
                started_at.elapsed(),
            );
            send_request_diagnostic(progress_tx, record)
                .await
                .map_err(non_retryable_attempt)?;
            response
        }
        Err(error) => {
            let message = format!("Could not connect to the server: {error}");
            let record = error_diagnostic_record(
                task_id,
                "GET",
                url,
                range_header.clone(),
                use_range.then(|| if_range.map(str::to_string)).flatten(),
                &message,
                segment.retry_count,
                started_at.elapsed(),
            );
            send_request_diagnostic(progress_tx, record)
                .await
                .map_err(non_retryable_attempt)?;
            return Err(retryable(segment_failure(segment, offset, &message)));
        }
    };

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

    if use_range {
        let expected_total = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range)
            .filter(|range| range.start == offset && range.end == initial_range_end)
            .filter(|range| range.total == total_size);
        if expected_total.is_none() {
            return Err(non_retryable(segment_failure(
                segment,
                offset,
                "Resume unavailable. The server returned a mismatched Content-Range.",
            )));
        }
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

        let current_end = range_end.load(Ordering::SeqCst);
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
        if i64::try_from(chunk.len()).unwrap_or(0) > allowed && current_end == initial_range_end {
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

    let final_end = range_end.load(Ordering::SeqCst);
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

async fn send_request_diagnostic(
    progress_tx: &mpsc::Sender<SegmentMessage>,
    record: RequestDiagnosticRecord,
) -> Result<(), SegmentFailure> {
    progress_tx
        .send(SegmentMessage::Request { record })
        .await
        .map_err(|_| SegmentFailure {
            segment_id: String::new(),
            downloaded_until: 0,
            error: "Progress channel closed before request diagnostics could be recorded."
                .to_string(),
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
