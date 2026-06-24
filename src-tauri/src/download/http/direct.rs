use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    Arc,
};

use reqwest::{Client, StatusCode};
use tokio::{fs, io::{AsyncWriteExt, BufWriter}, sync::mpsc, task::JoinSet};

use super::{
    error::format_http_status,
    file::{finalize_download_file, preallocate_temp_file},
    request::send_get_with_retry,
    segmented::diagnostics::{if_range_header_from, parse_content_range},
    segmented::{download_segment_worker, SegmentMessage, SegmentWorkerRequest},
    DirectDownloadRequest, DirectSegmentedDownloadRequest,
};
use crate::{db, download::GlobalSpeedLimiter, models::AppErrorPayload};

pub(super) async fn run_direct_download(
    client: &Client,
    request: DirectDownloadRequest,
    cancel: Arc<AtomicBool>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
) -> Result<i64, String> {
    let resume_from = fs::metadata(&request.temp_path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    if resume_from > 0 && !request.supports_resume {
        return Err("Resume unavailable. Restart this download from the beginning.".to_string());
    }

    let if_range = if_range_header_from(request.etag.as_deref(), request.last_modified.as_deref());
    let mut response = send_get_with_retry(
        client,
        &request.url,
        (resume_from > 0).then(|| format!("bytes={resume_from}-")),
        (resume_from > 0).then_some(if_range.as_deref()).flatten(),
        &[],
    )
    .await?;

    if resume_from > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(
            "Resume unavailable. The server did not honor the byte range request.".to_string(),
        );
    }
    if resume_from > 0 && !valid_direct_content_range(&response, resume_from, request.total_size) {
        return Err(
            "Resume unavailable. The server returned a mismatched Content-Range.".to_string(),
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

    let raw_file = if resume_from > 0 {
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
    let mut file = BufWriter::with_capacity(256 * 1024, raw_file);

    let mut downloaded = resume_from;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("The connection failed while downloading: {e}"))?
    {
        speed_limiter.throttle(chunk.len()).await;
        file.write_all(&chunk).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write to disk: {e}"))
                .command_error()
        })?;
        downloaded += i64::try_from(chunk.len()).unwrap_or(0);

        if cancel.load(Ordering::SeqCst) {
            file.flush()
                .await
                .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
            return Ok(downloaded);
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Could not flush the temporary file: {e}"))?;

    if request.total_size > 0 && downloaded < request.total_size {
        return Err("The download ended before all bytes were received.".to_string());
    }

    finalize_download_file(&request.temp_path, &request.final_path).await?;

    Ok(downloaded)
}

pub(super) async fn run_direct_segmented_download(
    client: &Client,
    request: DirectSegmentedDownloadRequest,
    cancel: Arc<AtomicBool>,
    speed_limiter: Arc<GlobalSpeedLimiter>,
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
    let temp_file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&request.temp_path)
        .await
        .map_err(|e| format!("Could not create the temporary file: {e}"))?;
    preallocate_temp_file(&temp_file, request.total_size, "direct-segmented").await;
    drop(temp_file);

    let (progress_tx, mut progress_rx) = mpsc::channel::<SegmentMessage>(64);
    let mut workers = JoinSet::new();
    let segment_count = request.segments.len();
    let mut active_workers = 0_usize;
    let if_range = if_range_header_from(request.etag.as_deref(), request.last_modified.as_deref());
    let cancel_token = tokio_util::sync::CancellationToken::new();

    for segment in request.segments {
        let offset = segment
            .downloaded_until
            .clamp(segment.range_start, segment.range_end.saturating_add(1));
        if offset > segment.range_end {
            continue;
        }
        let range_end = segment.range_end;
        active_workers += 1;
        workers.spawn(download_segment_worker(SegmentWorkerRequest {
            client: client.clone(),
            task_id: segment.task_id.clone(),
            url: request.url.clone(),
            temp_path: request.temp_path.clone(),
            segment,
            total_size: request.total_size,
            segment_count,
            supports_parallel: request.supports_parallel,
            cancel: cancel.clone(),
            cancel_token: cancel_token.clone(),
            progress_tx: progress_tx.clone(),
            range_end: Arc::new(AtomicI64::new(range_end)),
            speed_limiter: speed_limiter.clone(),
            request_headers: Vec::new(),
            if_range: if_range.clone(),
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

    finalize_download_file(&request.temp_path, &request.final_path).await?;

    Ok(request.total_size)
}

fn valid_direct_content_range(
    response: &reqwest::Response,
    resume_from: i64,
    total_size: i64,
) -> bool {
    let Some(range) = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
    else {
        return false;
    };
    if range.start != resume_from {
        return false;
    }
    if total_size > 0 {
        range.end == total_size.saturating_sub(1) && range.total == total_size
    } else {
        range.end >= range.start
    }
}
