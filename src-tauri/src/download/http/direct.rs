use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use reqwest::{Client, StatusCode};
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::{mpsc, RwLock},
    task::JoinSet,
};

use super::{
    error::format_http_status,
    file::finalize_download_file,
    request::send_get_with_retry,
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

    let mut response = send_get_with_retry(
        client,
        &request.url,
        (resume_from > 0).then(|| format!("bytes={resume_from}-")),
    )
    .await?;

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

        speed_limiter.throttle(chunk.len()).await;
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
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&request.temp_path)
        .await
        .map_err(|e| format!("Could not create the temporary file: {e}"))?;

    let (progress_tx, mut progress_rx) = mpsc::channel::<SegmentMessage>(64);
    let mut workers = JoinSet::new();
    let segment_count = request.segments.len();
    let live_ends = Arc::new(RwLock::new(
        request
            .segments
            .iter()
            .map(|segment| (segment.id.clone(), segment.range_end))
            .collect::<HashMap<_, _>>(),
    ));
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
            supports_parallel: request.supports_parallel,
            cancel: cancel.clone(),
            progress_tx: progress_tx.clone(),
            live_ends: live_ends.clone(),
            speed_limiter: speed_limiter.clone(),
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
