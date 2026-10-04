use crate::download::lifecycle::JoinSet;
use crate::download::owned_fs as fs;
use std::sync::{atomic::AtomicI64, Arc};

use reqwest::{Client, StatusCode};
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::mpsc,
};

use super::{
    error::format_http_status_with_retry_after,
    request::{retry_after_at, send_get_with_retry},
    segmented::diagnostics::{
        has_strong_resume_validator, if_range_header_from, parse_content_range,
        response_validator_matches,
    },
    segmented::{download_segment_worker, SegmentMessage, SegmentWorkerRequest},
    DirectDownloadRequest, DirectSegmentedDownloadRequest, HTTP_CHUNK_READ_TIMEOUT,
};
use crate::{
    db,
    download::network_policy::NetworkPolicy,
    download::{
        file_ops::{finalize_download_file, preallocate_temp_file},
        GlobalSpeedLimiter,
    },
    models::AppErrorPayload,
};

pub(super) async fn run_direct_download(
    client: &Client,
    request: DirectDownloadRequest,
    cancel_token: tokio_util::sync::CancellationToken,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    network_policy: &NetworkPolicy,
) -> Result<i64, String> {
    let resume_from = fs::metadata(&request.temp_path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    if resume_from > 0
        && (!request.supports_resume
            || (request.total_size <= 0
                && !has_strong_resume_validator(
                    request.etag.as_deref(),
                    request.last_modified.as_deref(),
                )))
    {
        return Err("Resume unavailable. Restart this download from the beginning.".to_string());
    }

    let if_range = if_range_header_from(request.etag.as_deref(), request.last_modified.as_deref());
    let mut response = send_get_with_retry(
        client,
        &request.url,
        (resume_from > 0).then(|| format!("bytes={resume_from}-")),
        (resume_from > 0).then_some(if_range.as_deref()).flatten(),
        &[],
        network_policy,
    )
    .await?;

    if !response.status().is_success() {
        return Err(format_http_status_with_retry_after(
            response.status(),
            retry_after_at(&response),
        ));
    }
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
    if resume_from > 0
        && request.total_size <= 0
        && !response_validator_matches(
            response.headers(),
            request.etag.as_deref(),
            request.last_modified.as_deref(),
        )
    {
        return Err(
            "Resume unavailable. The remote resource validator changed during resume.".to_string(),
        );
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
    loop {
        let chunk = tokio::select! {
            _ = cancel_token.cancelled() => {
                file.flush()
                    .await
                    .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
                return Ok(downloaded);
            }
            chunk = tokio::time::timeout(HTTP_CHUNK_READ_TIMEOUT, response.chunk()) => match chunk {
                Ok(Ok(Some(data))) => data,
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    return Err(crate::download::probe_error::reqwest_error_to_structured(&error));
                }
                Err(_) => {
                    return Err(crate::download::probe_error::structured_timeout_error(
                        "Connection stalled: no data received for 60 seconds.",
                    ));
                }
            }
        };
        if speed_limiter
            .throttle(chunk.len(), &cancel_token)
            .await
            .is_err()
        {
            file.flush()
                .await
                .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
            return Ok(downloaded);
        }
        file.write_all(&chunk).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write to disk: {e}"))
                .command_error()
        })?;
        downloaded += i64::try_from(chunk.len()).unwrap_or(0);
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
    cancel_token: tokio_util::sync::CancellationToken,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    network_policy: &NetworkPolicy,
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
            supports_resume: request.supports_resume,
            cancel_token: cancel_token.clone(),
            progress_tx: progress_tx.clone(),
            range_end: Arc::new(AtomicI64::new(range_end)),
            speed_limiter: speed_limiter.clone(),
            request_headers: Vec::new(),
            if_range: if_range.clone(),
            network_policy: network_policy.clone(),
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
                        cancel_token.cancel();
                        workers.abort_all();
                        return Err(failure.error);
                    }
                    Err(error) => {
                        cancel_token.cancel();
                        workers.abort_all();
                        return Err(format!("A download worker stopped unexpectedly: {error}"));
                    }
                }
            }
            else => break,
        }
    }

    if cancel_token.is_cancelled() {
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
    if total_size <= 0 {
        return super::segmented::valid_unknown_size_content_range(response, resume_from);
    }
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
    range.end == total_size.saturating_sub(1) && range.total == total_size
}
