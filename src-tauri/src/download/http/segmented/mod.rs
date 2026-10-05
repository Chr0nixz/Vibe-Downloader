use crate::download::owned_fs as fs;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::{header::CONTENT_RANGE, Client, StatusCode};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::io::{AsyncWriteExt, BufWriter};

mod acceleration;
mod checkpoint;
mod coordinator;
pub(super) mod diagnostics;
mod runtime_progress;
mod worker;
mod writer;

use self::coordinator::SegmentCoordinator;
use self::diagnostics::{
    has_strong_resume_validator, persist_error_diagnostic, persist_response_diagnostic,
    response_validator_matches, RequestDiagnosticContext,
};
use self::runtime_progress::{emit_progress, progress_payload};
pub(super) use self::worker::{download_segment_worker, SegmentWorkerRequest};

use super::{request::send_get_with_retry, HTTP_CHUNK_READ_TIMEOUT};
use crate::{
    db,
    download::network_policy::NetworkPolicy,
    download::{
        file_ops::{finalize_download_file, persist_completed_path},
        GlobalSpeedLimiter,
    },
    events::{emit_queue_changed_with_ids, emit_task_updated_record, TaskProgressEmitGate},
    models::{
        AppErrorPayload, RequestDiagnosticRecord, SegmentStatus, TaskRecord, TaskSegmentRecord,
        TaskStatus,
    },
};

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

pub(super) struct SegmentedDownloadContext<'a> {
    pub(super) client: &'a Client,
    pub(super) app: Option<AppHandle>,
    pub(super) pool: SqlitePool,
    pub(super) task: TaskRecord,
    pub(super) cancel_token: tokio_util::sync::CancellationToken,
    pub(super) speed_limiter: Arc<GlobalSpeedLimiter>,
    pub(super) connection_limit: usize,
    pub(super) request_headers: Vec<(String, String)>,
    pub(super) network_policy: NetworkPolicy,
}

#[tracing::instrument(skip(context), fields(task_id = %context.task.id))]
pub(super) async fn run_segmented_download(
    context: SegmentedDownloadContext<'_>,
) -> Result<(), String> {
    let temp_path = context
        .task
        .temp_path
        .clone()
        .ok_or_else(|| "Task is missing a temporary path.".to_string())?;
    let final_path = context
        .task
        .final_path
        .clone()
        .ok_or_else(|| "Task is missing a final path.".to_string())?;
    let temp_path_buf = PathBuf::from(&temp_path);
    let final_path_buf = PathBuf::from(&final_path);
    let segments = db::ensure_task_segments(&context.pool, &context.task).await?;

    if context.task.total_size <= 0 {
        let SegmentedDownloadContext {
            client,
            app,
            pool,
            task,
            cancel_token,
            speed_limiter,
            connection_limit: _,
            request_headers,
            network_policy,
        } = context;
        return run_unknown_size_download(UnknownSizeDownloadContext {
            client,
            app,
            pool,
            task,
            temp_path_buf,
            final_path_buf,
            segments,
            cancel_token,
            speed_limiter,
            request_headers,
            network_policy,
        })
        .await;
    }

    SegmentCoordinator::new(context, segments, temp_path_buf, final_path_buf)
        .run()
        .await
}

struct UnknownSizeDownloadContext<'a> {
    client: &'a Client,
    app: Option<AppHandle>,
    pool: SqlitePool,
    task: TaskRecord,
    temp_path_buf: PathBuf,
    final_path_buf: PathBuf,
    segments: Vec<TaskSegmentRecord>,
    cancel_token: tokio_util::sync::CancellationToken,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    request_headers: Vec<(String, String)>,
    network_policy: NetworkPolicy,
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
        cancel_token,
        speed_limiter,
        request_headers,
        network_policy,
    } = context;

    let segment = segments
        .into_iter()
        .next()
        .ok_or_else(|| "Task segment could not be created.".to_string())?;
    let url = task.final_url.clone().unwrap_or_else(|| task.url.clone());
    let mut resume_from = fs::metadata(&temp_path_buf)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    if resume_from > 0
        && (!task.supports_resume
            || !has_strong_resume_validator(task.etag.as_deref(), task.last_modified.as_deref()))
    {
        // Unknown-size streams without a stable range identity cannot be
        // appended safely. A fresh worker may still recover by starting over.
        fs::remove_file(&temp_path_buf)
            .await
            .map_err(|error| format!("Could not restart the temporary file: {error}"))?;
        db::update_task_and_segment_progress(
            &pool,
            &task.id,
            &segment.id,
            0,
            0,
            1,
            TaskStatus::Downloading,
        )
        .await?;
        resume_from = 0;
    }

    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Downloading,
        None,
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
        Some(resume_from),
        None,
    )
    .await?;

    if let Some(parent) = temp_path_buf.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }
    let started_at = Instant::now();
    let range = (resume_from > 0).then(|| format!("bytes={resume_from}-"));
    let if_range = (resume_from > 0)
        .then(|| {
            diagnostics::if_range_header_from(task.etag.as_deref(), task.last_modified.as_deref())
        })
        .flatten();
    let mut response = match send_get_with_retry(
        client,
        &url,
        range.clone(),
        if_range.as_deref(),
        &request_headers,
        &network_policy,
    )
    .await
    {
        Ok(response) => {
            persist_response_diagnostic(
                RequestDiagnosticContext {
                    pool: &pool,
                    task_id: &task.id,
                    method: "GET",
                    url: &url,
                    range_header: range.clone(),
                    if_range_header: if_range.clone(),
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
        return Err(super::error::format_http_status_with_retry_after(
            response.status(),
            super::request::retry_after_at(&response),
        ));
    }
    if resume_from > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(AppErrorPayload::new(
            "remote_changed",
            "The remote resource did not honor the verified resume range.",
            false,
            vec!["restart", "check_url"],
        )
        .command_error());
    }
    if resume_from > 0 && !valid_unknown_size_content_range(&response, resume_from) {
        return Err(AppErrorPayload::new(
            "remote_changed",
            "The remote resource returned a mismatched resume range.",
            false,
            vec!["restart", "check_url"],
        )
        .command_error());
    }
    if resume_from > 0
        && task.total_size <= 0
        && !response_validator_matches(
            response.headers(),
            task.etag.as_deref(),
            task.last_modified.as_deref(),
        )
    {
        return Err(AppErrorPayload::new(
            "remote_changed",
            "The remote resource validator changed during resume.",
            false,
            vec!["restart", "check_url"],
        )
        .command_error());
    }
    let raw_file = if resume_from > 0 {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&temp_path_buf)
            .await
            .map_err(|e| format!("Could not open the temporary file for resume: {e}"))?
    } else {
        fs::File::create(&temp_path_buf)
            .await
            .map_err(|e| format!("Could not create the temporary file: {e}"))?
    };
    let mut file = BufWriter::with_capacity(256 * 1024, raw_file);
    let mut downloaded = resume_from;
    let mut progress_gate = TaskProgressEmitGate::default();
    let mut last_emit = Instant::now();
    let mut last_checkpoint = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_bytes = 0_i64;

    loop {
        let chunk = tokio::select! {
            _ = cancel_token.cancelled() => {
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
                progress_gate.flush(&app);
                return Ok(());
            }
            chunk = tokio::time::timeout(HTTP_CHUNK_READ_TIMEOUT, response.chunk()) => match chunk {
                Ok(Ok(Some(data))) => data,
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    // A midstream abort must not discard bytes already read:
                    // flush so the partial file length matches `downloaded`
                    // and a later retry can resume from it.
                    file.flush()
                        .await
                        .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
                    return Err(crate::download::probe_error::reqwest_error_to_structured(&error));
                }
                Err(_) => {
                    // Same durability guarantee as the transport-error path:
                    // a stalled connection is also a midstream abort.
                    file.flush()
                        .await
                        .map_err(|e| format!("Could not flush the temporary file: {e}"))?;
                    return Err(crate::download::probe_error::structured_timeout_error(
                        "Connection stalled: no data received for 60 seconds.",
                    ));
                }
            }
        };
        if cancel_token.is_cancelled() {
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
            progress_gate.flush(&app);
            return Ok(());
        }

        if speed_limiter
            .throttle(chunk.len(), &cancel_token)
            .await
            .is_err()
        {
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
            progress_gate.flush(&app);
            return Ok(());
        }
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
                &mut progress_gate,
                progress_payload(
                    &task.id,
                    downloaded,
                    0,
                    speed_bps,
                    1,
                    TaskStatus::Downloading,
                ),
                false,
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
        &mut progress_gate,
        progress_payload(
            &task.id,
            downloaded,
            downloaded,
            0,
            0,
            TaskStatus::Completed,
        ),
        true,
    );
    emit_queue_changed_with_ids(&app, Some(vec![task.id.clone()]));
    Ok(())
}

pub(super) fn valid_unknown_size_content_range(
    response: &reqwest::Response,
    resume_from: i64,
) -> bool {
    let valid_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (unit, range_and_total) = value.trim().split_once(' ')?;
            if !unit.eq_ignore_ascii_case("bytes") {
                return None;
            }
            let (range, total) = range_and_total.split_once('/')?;
            let (start, end) = range.split_once('-')?;
            let start = start.trim().parse::<i64>().ok()?;
            let end = end.trim().parse::<i64>().ok()?;
            let total = total.trim();
            let total_matches = total == "*"
                || total
                    .parse::<i64>()
                    .is_ok_and(|total| total > 0 && end < total);
            total_matches.then_some((start, end))
        });
    valid_range.is_some_and(|(start, end)| start == resume_from && start >= 0 && end >= start)
}
