use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use quick_xml::{events::Event, Reader};
use reqwest::{header::RANGE, Client, StatusCode};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
    sync::mpsc,
};

use super::{
    engine::EngineFuture, file_ops::finalize_download_file, http::HttpEngine,
    url_classify::is_metalink_url, DownloadContext, DownloadEngine, DownloadError, ProbeOutput,
    ProbeRequest,
};
use crate::{
    db,
    download::checksum::hash_file,
    download::error::engine_error,
    download::retry::RetryPolicy,
    events::{emit_task_updated_record, TaskProgressEmitGate},
    logging::sanitize_url,
    models::{
        AppErrorPayload, ChecksumAlgorithm, EngineCapabilities, HashVerificationStatus,
        MetalinkChecksum, MetalinkFile, MetalinkProbeData, MetalinkResource, ProbedFile,
        TaskFileRecord, TaskKind, TaskProgressPayload, TaskRecord, TaskStatus,
    },
};

const PROTOCOL_METALINK: &str = "metalink";
const METALINK_CONTENT_TYPE: &str = "application/metalink4+xml";
const DEFAULT_RESOURCE_PRIORITY: i64 = 999_999;

/// Cap parallel mirror connections per file. Each worker downloads a
/// range from a DIFFERENT mirror, so this is the per-task ceiling on
/// simultaneously active mirrors — kept conservative to avoid tripping
/// per-IP rate limits on small mirror fleets. Aligned with industry
/// defaults (aria2 -s 3, lftp default 3).
const METALINK_MAX_PARALLEL_MIRRORS: usize = 3;

/// F-3: Per-worker progress message for the parallel coordinator. Each
/// worker reports its own cumulative byte count (not a delta), keyed by
/// `worker_index` so the coordinator can maintain a per-worker total and
/// sum them — avoiding the F-3 bug where N workers overwrite each other's
/// progress on the same `file_id` row.
#[derive(Debug)]
pub struct MetalinkWorkerProgress {
    pub worker_index: usize,
    pub downloaded: i64,
}

#[derive(Debug, Clone)]
pub struct MetalinkEngine {
    /// E-4: 共享 `HttpEngine` 的客户端缓存，避免每次 probe/download 新建 Client。
    http: Arc<HttpEngine>,
}

impl MetalinkEngine {
    pub fn new(http: Arc<HttpEngine>) -> Self {
        Self { http }
    }

    async fn client(&self) -> Result<Client, String> {
        self.http.client().await
    }

    async fn probe_metalink(
        &self,
        url: &str,
        request_headers: &[(String, String)],
        app: &Option<tauri::AppHandle>,
        request_id: &Option<String>,
    ) -> Result<MetalinkProbeData, String> {
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "fetching_manifest",
            Some("metalink"),
        );
        let bytes = fetch_manifest_bytes(&self.client().await?, url, request_headers).await?;
        let text = String::from_utf8(bytes).map_err(|_| {
            engine_error(
                "metalink_invalid_manifest",
                "Metalink manifest is not valid UTF-8.",
                false,
            )
        })?;
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "parsing_manifest",
            Some("metalink"),
        );
        parse_metalink_manifest(url, &text)
    }
}

impl DownloadEngine for MetalinkEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_METALINK
    }

    /// R-3: Metalink 仅靠 `matches_url`（`.meta4`/`.metalink` 后缀）路由，
    /// 不参与 scheme 兜底，避免普通 https URL 被误路由到 Metalink。
    fn supports_scheme(&self, _scheme: &str) -> bool {
        false
    }

    /// R-3: `.meta4` / `.metalink` 路径路由到 Metalink 引擎。优先级 90。
    fn matches_url(&self, url: &reqwest::Url) -> bool {
        is_metalink_url(url)
    }

    fn priority(&self) -> i32 {
        90
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            let data = self
                .probe_metalink(
                    &request.uri,
                    &request.request_headers,
                    &request.app,
                    &request.request_id,
                )
                .await
                .map_err(DownloadError::Other)?;
            let files = data
                .files
                .iter()
                .map(|file| ProbedFile {
                    relative_path: file.relative_path.clone(),
                    size: file.size.to_string(),
                    content_type: content_type_for_path(&file.relative_path),
                })
                .collect::<Vec<_>>();
            let total_size = data.files.iter().map(|file| file.size.max(0)).sum::<i64>();
            // Parallel range download is enabled when ALL files have a
            // known size > 0 — the per-file dispatcher in
            // `download_metalink_file` re-checks the threshold for each
            // file individually, so this is just the task-level gate.
            let supports_parallel = data.files.iter().all(|file| file.size > 0);
            Ok(ProbeOutput {
                protocol: PROTOCOL_METALINK.to_string(),
                task_kind: TaskKind::Manifest,
                resolved_uri: data.manifest_url.clone(),
                display_name: display_name_for_manifest(&request.uri, &data.files),
                total_size,
                source_key: format!("metalink:{}", data.manifest_url),
                capabilities: EngineCapabilities {
                    supports_resume: true,
                    supports_parallel,
                    supports_multi_file: data.files.len() > 1,
                },
                files,
                etag: None,
                last_modified: None,
                content_type: Some(METALINK_CONTENT_TYPE.to_string()),
                hls_variants: Vec::new(),
                hls_audio_tracks: Vec::new(),
                hls_subtitle_tracks: Vec::new(),
                metalink: Some(data),
            })
        })
    }

    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), DownloadError>> {
        Box::pin(async move {
            run_metalink_download(self.clone(), context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}

async fn run_metalink_download(
    engine: MetalinkEngine,
    context: DownloadContext,
) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel_token,
        speed_limiter,
        request_headers,
        ..
    } = context;
    let client = engine.client().await?;
    let files = db::list_task_file_records(&pool, &task.id)
        .await?
        .into_iter()
        .filter(|file| file.selected)
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Err(engine_error(
            "metalink_no_files",
            "No Metalink files were selected.",
            false,
        ));
    }
    let mut completed_total = files
        .iter()
        .filter(|file| file.status == TaskStatus::Completed)
        .map(|file| file.downloaded_bytes)
        .sum::<i64>();

    for file in files {
        if cancel_token.is_cancelled() {
            pause_metalink_task(&app, &pool, &task, completed_total).await?;
            return Ok(());
        }
        if file.status == TaskStatus::Completed {
            continue;
        }
        let downloaded = download_metalink_file(
            &app,
            &pool,
            &task,
            &file,
            &client,
            &request_headers,
            &speed_limiter,
            &cancel_token,
            completed_total,
        )
        .await?;
        completed_total = completed_total.saturating_add(downloaded);
    }

    complete_metalink_task(&app, &pool, &task, completed_total).await
}

#[allow(clippy::too_many_arguments)]
async fn download_metalink_file(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    file: &TaskFileRecord,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    completed_before_file: i64,
) -> Result<i64, String> {
    let resources = db::list_metalink_resources_for_file(pool, &file.id).await?;
    if resources.is_empty() {
        return Err(engine_error(
            "metalink_no_resources",
            format!(
                "No usable HTTP mirror is available for {}.",
                file.relative_path
            ),
            false,
        ));
    }
    let temp_path = file
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Metalink file is missing a temporary path.".to_string())?;
    let final_path = file
        .final_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Metalink file is missing a final path.".to_string())?;

    // Parallel dispatch: enable when the task advertises parallel support,
    // the file is large enough to benefit, and we have at least 2 healthy
    // mirrors that advertise HTTP Range support. Otherwise fall through to
    // the serial failover path (which is the unchanged legacy behaviour).
    let parallel_eligible = task.supports_parallel
        && file.total_size > 0
        && file.total_size >= db::DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES;
    if parallel_eligible {
        let healthy = db::list_healthy_mirrors_for_file(pool, &file.id).await?;
        if healthy.len() >= 2 {
            return download_metalink_file_parallel(
                app,
                pool,
                task,
                file,
                client,
                request_headers,
                speed_limiter,
                cancel_token,
                completed_before_file,
                &temp_path,
                &final_path,
                healthy,
            )
            .await;
        }
        // Not enough healthy range-capable mirrors — fall through to the
        // serial path, which can still use mirrors that don't advertise
        // Range support via full-file GET.
    }

    let mut last_error = None;

    for resource in resources {
        let retry_policy = RetryPolicy::metalink_mirror();
        let mut mirror_attempt = 0u32;
        let result = loop {
            let result = download_from_resource(
                DownloadFileContext {
                    app,
                    pool,
                    task,
                    file,
                    client,
                    request_headers,
                    speed_limiter,
                    cancel_token,
                    completed_before_file,
                    temp_path: &temp_path,
                    final_path: &final_path,
                },
                &resource,
            )
            .await;
            match result {
                Ok(bytes) => break Ok(bytes),
                Err(error) if cancel_token.is_cancelled() => break Err(error),
                Err(error) => {
                    mirror_attempt += 1;
                    if mirror_attempt < retry_policy.max_attempts {
                        let delay = retry_policy.delay_for_attempt(mirror_attempt);
                        if !delay.is_zero() {
                            tokio::select! {
                                _ = cancel_token.cancelled() => break Err(error),
                                _ = tokio::time::sleep(delay) => {}
                            }
                        }
                    } else {
                        break Err(error);
                    }
                }
            }
        };
        match result {
            Ok(bytes) => {
                db::mark_metalink_resource_completed(pool, &resource.id).await?;
                return Ok(bytes);
            }
            Err(error) if cancel_token.is_cancelled() => return Err(error),
            Err(error) => {
                db::mark_metalink_resource_failed(pool, &resource.id, &error).await?;
                db::insert_task_event(
                    pool,
                    &task.id,
                    "metalink_mirror_failed",
                    Some(&format!("{}: {error}", sanitize_url(&resource.url))),
                )
                .await?;
                last_error = Some(error);
            }
        }
    }

    Err(engine_error(
        "metalink_all_mirrors_failed",
        format!(
            "All Metalink mirrors failed for {}. {}",
            file.relative_path,
            last_error.unwrap_or_else(|| "No additional error was reported.".to_string())
        ),
        true,
    ))
}

/// Parallel mirror range download path. Each worker downloads a non-
/// overlapping byte range from a different healthy mirror, writing to
/// `{temp_path}.part-{N}`. After all workers complete the part files
/// are concatenated in order into `temp_path`, verified, and finalized.
///
/// Mirror failover: if a worker's mirror fails (network error, 5xx,
/// 416 Range Not Satisfiable), the worker pulls the next healthy
/// mirror from the queue and retries the same range. If all mirrors
/// for a range are exhausted, the worker reports a partial-completion
/// error and the engine falls back to the serial full-file path.
#[allow(clippy::too_many_arguments)]
async fn download_metalink_file_parallel(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    file: &TaskFileRecord,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    completed_before_file: i64,
    temp_path: &Path,
    final_path: &Path,
    mut mirrors: Vec<db::MetalinkResourceRecord>,
) -> Result<i64, String> {
    use std::collections::VecDeque;
    use tokio::task::JoinSet;

    // Plan N ranges across [0, file.total_size). N is bounded by the
    // mirror count (each worker needs its own mirror) and the
    // METALINK_MAX_PARALLEL_MIRRORS ceiling.
    let worker_count = mirrors.len().min(METALINK_MAX_PARALLEL_MIRRORS);
    let total_size = file.total_size.max(0);
    if worker_count < 2 || total_size == 0 {
        // Defensive: the dispatcher already checks these conditions.
        // If we somehow get here, fall back to the serial path.
        return download_metalink_file_serial(
            app,
            pool,
            task,
            file,
            client,
            request_headers,
            speed_limiter,
            cancel_token,
            completed_before_file,
            temp_path,
            final_path,
            mirrors,
        )
        .await;
    }

    let count_i64 = i64::try_from(worker_count).unwrap_or(1);
    let base = total_size / count_i64;
    let remainder = total_size % count_i64;
    let ranges: Vec<(u64, u64)> = (0..worker_count)
        .map(|index| {
            let extra = if i64::try_from(index).unwrap_or(0) < remainder {
                1
            } else {
                0
            };
            let length = base + extra;
            // computed below; assigned via `start` capture
            (0_u64, 0_u64, length)
        })
        .scan(0_i64, |start, (_s, _e, length)| {
            let s = *start;
            let e = s + length - 1;
            *start = e + 1;
            Some((s as u64, e as u64))
        })
        .collect();

    // Part-file layout: each worker writes to `{temp}.part-{N}`.
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!(
                "Could not create Metalink temp folder: {e}"
            ))
            .command_error()
        })?;
    }

    // F-2: Only clean up part files when none exist (fresh start). If any
    // part file is present, we enter resume mode — workers append to the
    // existing part and send a range request offset by the existing bytes.
    if all_part_files_absent(temp_path, worker_count).await {
        cleanup_metalink_part_files(temp_path).await;
    }

    // Each worker owns exactly one mirror from the head of the list.
    // Excess mirrors form a failover queue that workers can pull from
    // when their primary mirror fails.
    let failover_queue: VecDeque<db::MetalinkResourceRecord> =
        mirrors.drain(worker_count..).collect();
    let primary_mirrors: Vec<db::MetalinkResourceRecord> = mirrors;
    let failover_queue = std::sync::Arc::new(tokio::sync::Mutex::new(failover_queue));

    let mut progress_gate = TaskProgressEmitGate::default();
    let mut last_emit = Instant::now();
    let mut last_checkpoint = Instant::now();
    // F-3: When resuming, persist the already-downloaded byte count so the
    // initial `update_task_file_progress` does not regress the persisted
    // value to 0. Workers will report their per-range `already_downloaded`
    // via mpsc and the coordinator will recompute the total.
    let mut initial_total: i64 = 0;
    for index in 0..worker_count {
        let part_path = part_file_path(temp_path, index);
        if let Ok(metadata) = fs::metadata(&part_path).await {
            initial_total =
                initial_total.saturating_add(i64::try_from(metadata.len()).unwrap_or(i64::MAX));
        }
    }
    db::update_task_file_progress(pool, &file.id, initial_total, TaskStatus::Downloading).await?;

    // F-3: mpsc channel for per-worker progress aggregation. Only the
    // coordinator writes `update_task_file_progress` to avoid N workers
    // overwriting each other's byte count on the same `file_id` row.
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<MetalinkWorkerProgress>();
    let mut worker_bytes: Vec<i64> = vec![0; worker_count];

    let mut workers = JoinSet::new();
    for (index, (range_start, range_end)) in ranges.iter().enumerate() {
        let mirror = primary_mirrors[index].clone();
        let part_path = part_file_path(temp_path, index);
        let worker = MetalinkRangeWorker {
            pool: pool.clone(),
            task_id: task.id.clone(),
            file_id: file.id.clone(),
            client: client.clone(),
            request_headers: request_headers.to_vec(),
            speed_limiter: speed_limiter.clone(),
            cancel_token: cancel_token.clone(),
            worker_index: index,
            range_start: *range_start,
            range_end: *range_end,
            mirror,
            part_path,
            failover_queue: failover_queue.clone(),
            progress_tx: progress_tx.clone(),
        };
        let captured_index = index;
        workers.spawn(async move {
            let result = worker.run().await;
            (captured_index, result)
        });
    }
    drop(progress_tx);

    let mut downloaded_total: i64 = initial_total;
    let mut worker_errors: Vec<String> = Vec::new();
    let mut active_workers = worker_count;
    loop {
        if cancel_token.is_cancelled() && active_workers == 0 {
            break;
        }
        tokio::select! {
            Some(progress) = progress_rx.recv() => {
                if progress.worker_index < worker_bytes.len() {
                    worker_bytes[progress.worker_index] = progress.downloaded;
                }
                downloaded_total = worker_bytes.iter().copied().sum();
                if last_emit.elapsed() >= Duration::from_millis(300) {
                    emit_metalink_progress(
                        app,
                        pool,
                        task,
                        file,
                        completed_before_file,
                        completed_before_file.saturating_add(downloaded_total),
                        &mut progress_gate,
                        false,
                    )
                    .await?;
                    last_emit = Instant::now();
                }
                if last_checkpoint.elapsed() >= Duration::from_secs(1) {
                    db::update_task_file_progress(
                        pool,
                        &file.id,
                        downloaded_total,
                        TaskStatus::Downloading,
                    )
                    .await?;
                    last_checkpoint = Instant::now();
                }
            }
            Some(joined) = workers.join_next(), if active_workers > 0 => {
                active_workers = active_workers.saturating_sub(1);
                match joined {
                    Ok((worker_index, Ok(downloaded))) => {
                        // F-3: Record the worker's final byte count in case the
                        // last mpsc message didn't flush before completion. This
                        // is authoritative for the worker_index.
                        if worker_index < worker_bytes.len() {
                            worker_bytes[worker_index] = downloaded;
                        }
                        downloaded_total = worker_bytes.iter().copied().sum();
                    }
                    Ok((_worker_index, Err(error))) => {
                        worker_errors.push(error);
                    }
                    Err(join_error) => {
                        worker_errors.push(format!("Metalink worker panicked: {join_error}"));
                    }
                }
            }
            else => break,
        }
    }

    if cancel_token.is_cancelled() {
        progress_gate.flush(app);
        return Err("Download canceled.".to_string());
    }

    if !worker_errors.is_empty() {
        // Some workers failed. If at least one range completed we still
        // have a partial file, but we cannot assemble a valid output
        // from partial ranges, so clean up and report partial completion.
        cleanup_metalink_part_files(temp_path).await;
        progress_gate.flush(app);
        return Err(engine_error(
            "metalink_partial_completion",
            format!(
                "Metalink parallel download for {} completed {} of {} bytes; \
                 failing mirrors: {}",
                file.relative_path,
                downloaded_total,
                total_size,
                worker_errors.join("; ")
            ),
            true,
        ));
    }

    // All workers succeeded — assemble the part files in order into the
    // final temp_path, then verify checksum and finalize.
    assemble_metalink_part_files(temp_path, worker_count)
        .await
        .map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not assemble Metalink parts: {e}"))
                .command_error()
        })?;
    cleanup_metalink_part_files(temp_path).await;

    verify_metalink_file(pool, file, temp_path).await?;
    finalize_download_file(temp_path, final_path).await?;
    db::update_task_file_progress(pool, &file.id, total_size, TaskStatus::Completed).await?;
    emit_metalink_progress(
        app,
        pool,
        task,
        file,
        completed_before_file,
        completed_before_file.saturating_add(total_size),
        &mut progress_gate,
        true,
    )
    .await?;
    progress_gate.flush(app);
    Ok(total_size)
}

/// Serial fallback path extracted from the legacy `download_metalink_file`
/// body so the parallel dispatcher can hand off to it cleanly when
/// parallel eligibility is not met (e.g. only one healthy mirror).
#[allow(clippy::too_many_arguments)]
async fn download_metalink_file_serial(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    file: &TaskFileRecord,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    completed_before_file: i64,
    temp_path: &Path,
    final_path: &Path,
    resources: Vec<db::MetalinkResourceRecord>,
) -> Result<i64, String> {
    let mut last_error = None;
    for resource in resources {
        let retry_policy = RetryPolicy::metalink_mirror();
        let mut mirror_attempt = 0u32;
        let result = loop {
            let result = download_from_resource(
                DownloadFileContext {
                    app,
                    pool,
                    task,
                    file,
                    client,
                    request_headers,
                    speed_limiter,
                    cancel_token,
                    completed_before_file,
                    temp_path,
                    final_path,
                },
                &resource,
            )
            .await;
            match result {
                Ok(bytes) => break Ok(bytes),
                Err(error) if cancel_token.is_cancelled() => break Err(error),
                Err(error) => {
                    mirror_attempt += 1;
                    if mirror_attempt < retry_policy.max_attempts {
                        let delay = retry_policy.delay_for_attempt(mirror_attempt);
                        if !delay.is_zero() {
                            tokio::select! {
                                _ = cancel_token.cancelled() => break Err(error),
                                _ = tokio::time::sleep(delay) => {}
                            }
                        }
                    } else {
                        break Err(error);
                    }
                }
            }
        };
        match result {
            Ok(bytes) => {
                db::mark_metalink_resource_completed(pool, &resource.id).await?;
                return Ok(bytes);
            }
            Err(error) if cancel_token.is_cancelled() => return Err(error),
            Err(error) => {
                db::mark_metalink_resource_failed(pool, &resource.id, &error).await?;
                db::insert_task_event(
                    pool,
                    &task.id,
                    "metalink_mirror_failed",
                    Some(&format!("{}: {error}", sanitize_url(&resource.url))),
                )
                .await?;
                last_error = Some(error);
            }
        }
    }

    Err(engine_error(
        "metalink_all_mirrors_failed",
        format!(
            "All Metalink mirrors failed for {}. {}",
            file.relative_path,
            last_error.unwrap_or_else(|| "No additional error was reported.".to_string())
        ),
        true,
    ))
}

/// Single-worker state for the parallel path. Owns the mirror assignment
/// for one range. On failure, pulls the next healthy mirror from the
/// shared failover queue (under a Mutex) and retries the same range.
struct MetalinkRangeWorker {
    pool: SqlitePool,
    task_id: String,
    file_id: String,
    client: Client,
    request_headers: Vec<(String, String)>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: tokio_util::sync::CancellationToken,
    /// F-3: Index into the coordinator's `worker_bytes` vec. Sent with each
    /// mpsc progress message so the coordinator can attribute the byte count.
    worker_index: usize,
    range_start: u64,
    range_end: u64,
    mirror: db::MetalinkResourceRecord,
    part_path: PathBuf,
    failover_queue:
        std::sync::Arc<tokio::sync::Mutex<std::collections::VecDeque<db::MetalinkResourceRecord>>>,
    /// F-3: Sender for per-worker progress aggregation. Only the coordinator
    /// writes `update_task_file_progress`; workers report via this channel.
    progress_tx: mpsc::UnboundedSender<MetalinkWorkerProgress>,
}

impl MetalinkRangeWorker {
    async fn run(mut self) -> Result<i64, String> {
        let retry_policy = RetryPolicy::metalink_segment();
        let mut attempt = 0u32;
        let worker_index = self.worker_index;
        let progress_tx = self.progress_tx.clone();
        let mut current_mirror = std::mem::replace(&mut self.mirror, {
            // placeholder; we never use self.mirror after construction
            db::MetalinkResourceRecord {
                id: String::new(),
                task_id: String::new(),
                file_id: String::new(),
                url: String::new(),
                priority: i64::MAX,
                location: None,
                status: String::new(),
                failure_count: 0,
                last_error: None,
                last_attempt_at: None,
                cooldown_until: None,
                avg_speed_bps: 0,
                supports_range: false,
            }
        });
        loop {
            db::mark_metalink_resource_attempted(&self.pool, &current_mirror.id)
                .await
                .ok();
            let result = download_metalink_range_from_mirror(
                &self.pool,
                &self.task_id,
                &self.client,
                &self.request_headers,
                &self.speed_limiter,
                &self.cancel_token,
                worker_index,
                &progress_tx,
                self.range_start,
                self.range_end,
                &current_mirror,
                &self.part_path,
            )
            .await;
            match result {
                Ok(downloaded) => return Ok(downloaded),
                Err(error) if self.cancel_token.is_cancelled() => return Err(error),
                Err(error) => {
                    attempt += 1;
                    if attempt < retry_policy.max_attempts {
                        let delay = retry_policy.delay_for_attempt(attempt);
                        if !delay.is_zero() {
                            tokio::select! {
                                _ = self.cancel_token.cancelled() => return Err(error),
                                _ = tokio::time::sleep(delay) => {}
                            }
                        }
                        continue;
                    }
                    // Mirror exhausted — failover to the next healthy mirror.
                    db::mark_metalink_resource_failed(&self.pool, &current_mirror.id, &error)
                        .await
                        .ok();
                    let next = {
                        let mut queue = self.failover_queue.lock().await;
                        queue.pop_front()
                    };
                    match next {
                        Some(next_mirror) => {
                            attempt = 0;
                            current_mirror = next_mirror;
                            continue;
                        }
                        None => {
                            return Err(engine_error(
                                "metalink_no_healthy_mirrors",
                                format!(
                                    "Range {}-{} for file {} exhausted all mirrors: {error}",
                                    self.range_start, self.range_end, self.file_id
                                ),
                                false,
                            ))
                        }
                    }
                }
            }
        }
    }
}

/// Download one byte range from one mirror to `part_path`. Returns the
/// number of bytes written (including any pre-existing resumed bytes).
/// Detects 416 Range Not Satisfiable and marks the mirror as not supporting
/// ranges, so future dispatches will route it to the serial full-file path
/// instead.
///
/// F-2: When `part_path` already exists, appends to it and sends a range
/// request offset by the existing byte count (`bytes={range_start+already}-{range_end}`).
/// F-3: Reports progress via `progress_tx` (aggregated by the coordinator)
/// instead of writing `update_task_file_progress` directly, so N workers
/// cannot overwrite each other's byte count on the same `file_id` row.
#[allow(clippy::too_many_arguments)]
pub async fn download_metalink_range_from_mirror(
    pool: &SqlitePool,
    task_id: &str,
    client: &Client,
    request_headers: &[(String, String)],
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &tokio_util::sync::CancellationToken,
    worker_index: usize,
    progress_tx: &mpsc::UnboundedSender<MetalinkWorkerProgress>,
    range_start: u64,
    range_end: u64,
    mirror: &db::MetalinkResourceRecord,
    part_path: &Path,
) -> Result<i64, String> {
    let expected = (range_end - range_start + 1) as i64;

    // F-2: Resume — stat the existing part file to get `already_downloaded`.
    let already_downloaded: u64 = fs::metadata(part_path).await.map(|m| m.len()).unwrap_or(0);
    let already_downloaded_i64 = i64::try_from(already_downloaded).unwrap_or(i64::MAX);
    if already_downloaded_i64 >= expected {
        // Range already complete — skip download. Report final byte count
        // so the coordinator's `worker_bytes` is accurate.
        let _ = progress_tx.send(MetalinkWorkerProgress {
            worker_index,
            downloaded: already_downloaded_i64,
        });
        return Ok(already_downloaded_i64);
    }

    let effective_start = range_start + already_downloaded;
    let started = Instant::now();
    let mut request = client.get(&mirror.url);
    for (name, value) in request_headers {
        request = request.header(name, value);
    }
    request = request.header(RANGE, format!("bytes={effective_start}-{range_end}"));
    let mut response = request
        .send()
        .await
        .map_err(|e| format!("Metalink mirror request failed: {e}"))?;
    crate::download::diagnostics::persist_engine_diagnostic(
        crate::download::diagnostics::EngineDiagnosticContext {
            pool,
            task_id,
            method: "GET",
            url: &mirror.url,
            range_header: Some(format!("bytes={effective_start}-{range_end}")),
            status_code: Some(i32::from(response.status().as_u16())),
            content_length: None,
            error: None,
            retry_count: 0,
            duration: started.elapsed(),
        },
    )
    .await;

    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        // 416 proves this mirror does not support Range requests.
        // Mark it so future dispatches route it to the serial path.
        db::mark_mirror_unsupported_range(pool, &mirror.id)
            .await
            .ok();
        return Err(engine_error(
            "metalink_mirror_unsupported_range",
            format!(
                "Mirror {} returned 416 Range Not Satisfiable for bytes={effective_start}-{range_end}.",
                sanitize_url(&mirror.url)
            ),
            false,
        ));
    }
    if !response.status().is_success() {
        return Err(format!(
            "Metalink mirror {} returned {} for range {}-{}",
            sanitize_url(&mirror.url),
            response.status(),
            effective_start,
            range_end
        ));
    }

    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!(
                "Could not create Metalink part folder: {e}"
            ))
            .command_error()
        })?;
    }
    // F-2: Open in append mode (create if absent, never truncate) so resumed
    // bytes are preserved on disk and new bytes are written after them.
    let out = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .await
        .map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not open Metalink part file: {e}"))
                .command_error()
        })?;
    let mut out = BufWriter::with_capacity(256 * 1024, out);

    // F-2: `downloaded` counts from `already_downloaded` so the final value
    // reflects the full range size and matches `expected` for validation.
    let mut downloaded: i64 = already_downloaded_i64;
    let mut last_progress = Instant::now();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Metalink mirror connection failed: {e}"))?
    {
        if cancel_token.is_cancelled() {
            out.flush()
                .await
                .map_err(|e| format!("Could not flush Metalink part file: {e}"))?;
            return Err("Download canceled.".to_string());
        }
        speed_limiter.throttle(chunk.len()).await;
        out.write_all(&chunk).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write Metalink part file: {e}"))
                .command_error()
        })?;
        downloaded = downloaded.saturating_add(i64::try_from(chunk.len()).unwrap_or(0));
        if last_progress.elapsed() >= Duration::from_millis(300) {
            // F-3: Report via mpsc instead of writing `file_id` row directly.
            let _ = progress_tx.send(MetalinkWorkerProgress {
                worker_index,
                downloaded,
            });
            last_progress = Instant::now();
        }
    }
    out.flush()
        .await
        .map_err(|e| format!("Could not flush Metalink part file: {e}"))?;

    if expected > 0 && downloaded != expected {
        return Err(format!(
            "Metalink mirror {} size mismatch for range {}-{}: expected {}, got {}.",
            sanitize_url(&mirror.url),
            range_start,
            range_end,
            expected,
            downloaded
        ));
    }
    Ok(downloaded)
}

/// Concatenate the `worker_count` part files in index order into
/// `temp_path`. After assembly each part file is removed.
pub async fn assemble_metalink_part_files(
    temp_path: &Path,
    worker_count: usize,
) -> Result<(), String> {
    let mut out = fs::File::create(temp_path)
        .await
        .map_err(|e| format!("Could not create assembled Metalink file: {e}"))?;
    let mut out = BufWriter::with_capacity(256 * 1024, &mut out);
    for index in 0..worker_count {
        let part_path = part_file_path(temp_path, index);
        let mut file = fs::File::open(&part_path)
            .await
            .map_err(|e| format!("Could not open Metalink part {}: {e}", part_path.display()))?;
        tokio::io::copy(&mut file, &mut out)
            .await
            .map_err(|e| format!("Could not copy Metalink part {}: {e}", part_path.display()))?;
    }
    out.flush()
        .await
        .map_err(|e| format!("Could not flush assembled Metalink file: {e}"))?;
    Ok(())
}

/// Remove all `*.part-N` files associated with `temp_path`. Best-effort,
/// ignores NotFound errors so it's safe to call before/after a download.
pub async fn cleanup_metalink_part_files(temp_path: &Path) {
    let Some(parent) = temp_path.parent() else {
        return;
    };
    let Ok(mut entries) = fs::read_dir(parent).await else {
        return;
    };
    let prefix = match temp_path.file_name().and_then(|s| s.to_str()) {
        Some(name) => format!("{name}.part-"),
        None => return,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with(&prefix) {
            let _ = fs::remove_file(entry.path()).await;
        }
    }
}

pub fn part_file_path(temp_path: &Path, index: usize) -> PathBuf {
    let mut name = temp_path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(format!(".part-{index}"));
    let mut path = temp_path.to_path_buf();
    path.set_file_name(name);
    path
}

/// F-2: Returns true only when none of the `worker_count` part files exist.
/// When any part file is present, the caller skips cleanup and enters resume
/// mode (append + offset range request) instead of truncating fresh parts.
pub async fn all_part_files_absent(temp_path: &Path, worker_count: usize) -> bool {
    for index in 0..worker_count {
        let part_path = part_file_path(temp_path, index);
        if fs::try_exists(&part_path).await.unwrap_or(false) {
            return false;
        }
    }
    true
}

struct DownloadFileContext<'a> {
    app: &'a AppHandle,
    pool: &'a SqlitePool,
    task: &'a TaskRecord,
    file: &'a TaskFileRecord,
    client: &'a Client,
    request_headers: &'a [(String, String)],
    speed_limiter: &'a Arc<crate::download::GlobalSpeedLimiter>,
    cancel_token: &'a tokio_util::sync::CancellationToken,
    completed_before_file: i64,
    temp_path: &'a Path,
    final_path: &'a Path,
}

async fn download_from_resource(
    context: DownloadFileContext<'_>,
    resource: &db::MetalinkResourceRecord,
) -> Result<i64, String> {
    let DownloadFileContext {
        app,
        pool,
        task,
        file,
        client,
        request_headers,
        speed_limiter,
        cancel_token,
        completed_before_file,
        temp_path,
        final_path,
    } = context;
    let mut progress_gate = TaskProgressEmitGate::default();
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!(
                "Could not create Metalink temp folder: {e}"
            ))
            .command_error()
        })?;
    }
    let mut resume_from = fs::metadata(temp_path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let started = Instant::now();
    let mut request = client.get(&resource.url);
    for (name, value) in request_headers {
        request = request.header(name, value);
    }
    if resume_from > 0 {
        request = request.header(RANGE, format!("bytes={resume_from}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Could not request Metalink mirror: {e}"))?;
    crate::download::diagnostics::persist_engine_diagnostic(
        crate::download::diagnostics::EngineDiagnosticContext {
            pool,
            task_id: &task.id,
            method: "GET",
            url: &resource.url,
            range_header: (resume_from > 0).then(|| format!("bytes={resume_from}-")),
            status_code: Some(i32::from(response.status().as_u16())),
            content_length: None,
            error: None,
            retry_count: 0,
            duration: started.elapsed(),
        },
    )
    .await;

    let mut response = if resume_from > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        fs::remove_file(temp_path).await.ok();
        resume_from = 0;
        let mut retry = client.get(&resource.url);
        for (name, value) in request_headers {
            retry = retry.header(name, value);
        }
        retry
            .send()
            .await
            .map_err(|e| format!("Could not restart Metalink mirror: {e}"))?
    } else {
        response
    };
    if !response.status().is_success() {
        return Err(format!("Metalink mirror returned {}", response.status()));
    }

    let out = if resume_from > 0 {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(temp_path)
            .await
    } else {
        fs::File::create(temp_path).await
    }
    .map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not open Metalink temp file: {e}"))
            .command_error()
    })?;
    let mut out = BufWriter::with_capacity(256 * 1024, out);

    let mut downloaded = resume_from;
    let mut last_emit = Instant::now();
    db::update_task_file_progress(pool, &file.id, downloaded, TaskStatus::Downloading).await?;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Metalink mirror connection failed: {e}"))?
    {
        if cancel_token.is_cancelled() {
            out.flush()
                .await
                .map_err(|e| format!("Could not flush Metalink temp file: {e}"))?;
            progress_gate.flush(app);
            return Err("Download canceled.".to_string());
        }
        speed_limiter.throttle(chunk.len()).await;
        out.write_all(&chunk).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write Metalink file: {e}"))
                .command_error()
        })?;
        downloaded = downloaded.saturating_add(i64::try_from(chunk.len()).unwrap_or(0));
        if last_emit.elapsed() >= Duration::from_millis(300) {
            emit_metalink_progress(
                app,
                pool,
                task,
                file,
                completed_before_file,
                downloaded,
                &mut progress_gate,
                false,
            )
            .await?;
            last_emit = Instant::now();
        }
    }
    out.flush()
        .await
        .map_err(|e| format!("Could not flush Metalink temp file: {e}"))?;
    if file.total_size > 0 && downloaded != file.total_size {
        return Err(format!(
            "Metalink mirror size mismatch for {}: expected {}, got {}.",
            file.relative_path, file.total_size, downloaded
        ));
    }
    verify_metalink_file(pool, file, temp_path).await?;
    finalize_download_file(temp_path, final_path).await?;
    db::update_task_file_progress(pool, &file.id, downloaded, TaskStatus::Completed).await?;
    emit_metalink_progress(
        app,
        pool,
        task,
        file,
        completed_before_file,
        downloaded,
        &mut progress_gate,
        true,
    )
    .await?;
    progress_gate.flush(app);
    Ok(downloaded)
}

async fn verify_metalink_file(
    pool: &SqlitePool,
    file: &TaskFileRecord,
    path: &Path,
) -> Result<(), String> {
    let checksums = db::list_task_checksum_records_for_file(pool, &file.id).await?;
    if checksums.is_empty() {
        return Ok(());
    }
    let checksum = checksums
        .iter()
        .find(|checksum| checksum.algorithm == ChecksumAlgorithm::Sha256)
        .or_else(|| {
            checksums
                .iter()
                .find(|checksum| checksum.algorithm == ChecksumAlgorithm::Sha512)
        })
        .or_else(|| {
            checksums
                .iter()
                .find(|checksum| checksum.algorithm == ChecksumAlgorithm::Sha1)
        })
        .or_else(|| {
            checksums
                .iter()
                .find(|checksum| checksum.algorithm == ChecksumAlgorithm::Md5)
        })
        .ok_or_else(|| "No supported Metalink checksum is available.".to_string())?;
    let actual = hash_file(path, checksum.algorithm).await?;
    let status = if actual.eq_ignore_ascii_case(&checksum.expected_hash) {
        HashVerificationStatus::Verified
    } else {
        HashVerificationStatus::Failed
    };
    let error = (status == HashVerificationStatus::Failed)
        .then(|| format!("{} checksum does not match.", checksum.algorithm.as_str()));
    db::update_task_checksum_record(pool, &checksum.id, Some(&actual), status, error.as_deref())
        .await?;
    if let Some(error) = error {
        return Err(error);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn emit_metalink_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    file: &TaskFileRecord,
    completed_before_file: i64,
    file_downloaded: i64,
    progress_gate: &mut TaskProgressEmitGate,
    force: bool,
) -> Result<(), String> {
    let total_downloaded = completed_before_file.saturating_add(file_downloaded);
    db::update_task_file_progress(pool, &file.id, file_downloaded, TaskStatus::Downloading).await?;
    db::update_task_runtime_progress(
        pool,
        &task.id,
        total_downloaded,
        0,
        1,
        TaskStatus::Downloading,
        Some("Downloading Metalink file"),
    )
    .await?;
    progress_gate.emit_or_store(
        app,
        TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: total_downloaded.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: "0".to_string(),
            connection_count: 1,
            status: TaskStatus::Downloading,
        },
        force,
    );
    Ok(())
}

async fn pause_metalink_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    downloaded: i64,
) -> Result<(), String> {
    db::update_task_runtime_progress(
        pool,
        &task.id,
        downloaded,
        0,
        0,
        TaskStatus::Paused,
        Some("Paused"),
    )
    .await?;
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn complete_metalink_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    downloaded: i64,
) -> Result<(), String> {
    let checksums = db::list_task_checksum_records(pool, &task.id).await?;
    let hash_status = if checksums.is_empty() {
        HashVerificationStatus::NotRequested
    } else if checksums
        .iter()
        .any(|checksum| checksum.status == HashVerificationStatus::Failed)
    {
        HashVerificationStatus::Failed
    } else if checksums
        .iter()
        .filter(|checksum| checksum.file_id.is_some())
        .all(|checksum| checksum.status == HashVerificationStatus::Verified)
    {
        HashVerificationStatus::Verified
    } else {
        HashVerificationStatus::Pending
    };
    db::update_hash_verification(pool, &task.id, None, hash_status, None).await?;
    db::update_task_runtime_progress(
        pool,
        &task.id,
        downloaded,
        0,
        0,
        TaskStatus::Completed,
        Some("Completed"),
    )
    .await?;
    db::insert_task_event(pool, &task.id, "completed", None).await?;
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn fetch_manifest_bytes(
    client: &Client,
    url: &str,
    request_headers: &[(String, String)],
) -> Result<Vec<u8>, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Metalink URL is invalid.".to_string())?;
    if parsed.scheme() == "file" {
        let path = parsed
            .to_file_path()
            .map_err(|_| "Metalink file path is invalid.".to_string())?;
        return fs::read(&path)
            .await
            .map_err(|e| format!("Could not read Metalink file {}: {e}", path.display()));
    }
    let mut request = client.get(url);
    for (name, value) in request_headers {
        request = request.header(name, value);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Could not request Metalink manifest: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Metalink manifest returned {}", response.status()));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| format!("Could not read Metalink manifest: {e}"))
}

fn parse_metalink_manifest(manifest_url: &str, text: &str) -> Result<MetalinkProbeData, String> {
    let manifest_format = if manifest_url.to_ascii_lowercase().ends_with(".metalink") {
        "metalink"
    } else {
        "meta4"
    };
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut current_file: Option<ParsedMetalinkFile> = None;
    let mut text_target: Option<String> = None;
    let mut files = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "file" => {
                        let file_name = attr_value(&event, "name").unwrap_or_default();
                        current_file = Some(ParsedMetalinkFile::new(file_name));
                    }
                    "size" | "hash" | "url" => text_target = Some(name.clone()),
                    _ => {}
                }
                if name == "hash" {
                    if let Some(file) = current_file.as_mut() {
                        file.pending_hash_algorithm = attr_value(&event, "type")
                            .and_then(|value| ChecksumAlgorithm::from_metalink_type(&value));
                    }
                }
                if name == "url" {
                    if let Some(file) = current_file.as_mut() {
                        let priority = attr_value(&event, "priority")
                            .and_then(|value| value.parse::<i64>().ok())
                            .or_else(|| {
                                attr_value(&event, "preference")
                                    .and_then(|value| value.parse::<i64>().ok())
                                    .map(|value| 1_000_i64.saturating_sub(value))
                            })
                            .unwrap_or(DEFAULT_RESOURCE_PRIORITY);
                        file.pending_resource_priority = priority;
                        file.pending_resource_location = attr_value(&event, "location");
                    }
                }
            }
            Ok(Event::Text(event)) => {
                if let (Some(file), Some(target)) = (current_file.as_mut(), text_target.as_deref())
                {
                    let text = event
                        .decode()
                        .map(|value| value.into_owned())
                        .unwrap_or_default();
                    match target {
                        "size" => file.size = text.trim().parse::<i64>().unwrap_or(0).max(0),
                        "hash" => {
                            if let Some(algorithm) = file.pending_hash_algorithm {
                                let value = text.trim().to_ascii_lowercase();
                                if valid_checksum(&value, algorithm) {
                                    file.checksums.push(MetalinkChecksum {
                                        algorithm,
                                        value,
                                        weak: matches!(
                                            algorithm,
                                            ChecksumAlgorithm::Md5 | ChecksumAlgorithm::Sha1
                                        ),
                                    });
                                }
                            }
                        }
                        "url" => {
                            let url = text.trim();
                            if usable_metalink_resource(url) {
                                file.resources.push(MetalinkResource {
                                    url: url.to_string(),
                                    priority: file.pending_resource_priority,
                                    location: file.pending_resource_location.clone(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(event)) => {
                let name = local_name(event.name().as_ref());
                if matches!(name.as_str(), "size" | "hash" | "url") {
                    text_target = None;
                }
                if name == "file" {
                    let Some(file) = current_file.take() else {
                        continue;
                    };
                    let file = file.finalize()?;
                    files.push(file);
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(engine_error(
                    "metalink_invalid_manifest",
                    format!("Metalink manifest could not be parsed: {error}"),
                    false,
                ))
            }
            _ => {}
        }
        buf.clear();
    }

    if files.is_empty() {
        return Err(engine_error(
            "metalink_invalid_manifest",
            "Metalink manifest does not contain any downloadable files.",
            false,
        ));
    }
    Ok(MetalinkProbeData {
        manifest_url: manifest_url.to_string(),
        manifest_format: manifest_format.to_string(),
        files,
    })
}

#[derive(Default)]
struct ParsedMetalinkFile {
    name: String,
    size: i64,
    checksums: Vec<MetalinkChecksum>,
    resources: Vec<MetalinkResource>,
    pending_hash_algorithm: Option<ChecksumAlgorithm>,
    pending_resource_priority: i64,
    pending_resource_location: Option<String>,
}

impl ParsedMetalinkFile {
    fn new(name: String) -> Self {
        Self {
            name,
            pending_resource_priority: DEFAULT_RESOURCE_PRIORITY,
            ..Self::default()
        }
    }

    fn finalize(mut self) -> Result<MetalinkFile, String> {
        let relative_path = sanitize_metalink_path(&self.name)?;
        if self.resources.is_empty() {
            return Err(engine_error(
                "metalink_no_resources",
                format!("Metalink file {relative_path} has no usable HTTP mirror."),
                false,
            ));
        }
        self.resources.sort_by_key(|resource| resource.priority);
        Ok(MetalinkFile {
            relative_path,
            size: self.size,
            checksums: self.checksums,
            resources: self.resources,
        })
    }
}

fn attr_value(event: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attr| local_name(attr.key.as_ref()).eq_ignore_ascii_case(name))
        .map(|attr| String::from_utf8_lossy(attr.value.as_ref()).to_string())
}

fn local_name(name: &[u8]) -> String {
    let value = String::from_utf8_lossy(name);
    value
        .rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(&value)
        .to_ascii_lowercase()
}

fn sanitize_metalink_path(value: &str) -> Result<String, String> {
    let trimmed = value.trim().replace('\\', "/");
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains(':') {
        return Err(engine_error(
            "metalink_invalid_manifest",
            "Metalink file name is not a safe relative path.",
            false,
        ));
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." || part == ".." {
            return Err(engine_error(
                "metalink_invalid_manifest",
                "Metalink file name contains an unsafe path segment.",
                false,
            ));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn usable_metalink_resource(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .is_some_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

fn valid_checksum(value: &str, algorithm: ChecksumAlgorithm) -> bool {
    let len = match algorithm {
        ChecksumAlgorithm::Md5 => 32,
        ChecksumAlgorithm::Sha1 => 40,
        ChecksumAlgorithm::Sha256 => 64,
        ChecksumAlgorithm::Sha512 => 128,
    };
    value.len() == len && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

impl ChecksumAlgorithm {
    fn from_metalink_type(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "").as_str() {
            "sha256" => Some(Self::Sha256),
            "sha512" => Some(Self::Sha512),
            "sha1" => Some(Self::Sha1),
            "md5" => Some(Self::Md5),
            _ => None,
        }
    }
}

fn display_name_for_manifest(input_url: &str, files: &[MetalinkFile]) -> String {
    if files.len() == 1 {
        return Path::new(&files[0].relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("download")
            .to_string();
    }
    reqwest::Url::parse(input_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
        })
        .and_then(|name| {
            Path::new(&name)
                .file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("metalink-{}", chrono::Utc::now().timestamp()))
}

fn content_type_for_path(path: &str) -> Option<String> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        "zip" => Some("application/zip"),
        "7z" => Some("application/x-7z-compressed"),
        "tar" => Some("application/x-tar"),
        "gz" => Some("application/gzip"),
        "pdf" => Some("application/pdf"),
        "mp4" => Some("video/mp4"),
        "mkv" => Some("video/x-matroska"),
        _ => None,
    }
    .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meta4_multiple_files_and_orders_resources() {
        let data = parse_metalink_manifest(
            "https://example.com/files.meta4",
            r#"
            <metalink xmlns="urn:ietf:params:xml:ns:metalink">
              <file name="one.bin">
                <size>4</size>
                <hash type="sha-256">9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a</hash>
                <url priority="2">https://mirror2.example/one.bin</url>
                <url priority="1" location="us">https://mirror1.example/one.bin</url>
              </file>
              <file name="dir/two.bin">
                <size>8</size>
                <url>https://mirror.example/two.bin</url>
              </file>
            </metalink>
            "#,
        )
        .expect("manifest");
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.files[0].relative_path, "one.bin");
        assert_eq!(
            data.files[0].resources[0].url,
            "https://mirror1.example/one.bin"
        );
        assert_eq!(data.files[1].relative_path, "dir/two.bin");
    }

    #[test]
    fn rejects_unsafe_paths() {
        let error = parse_metalink_manifest(
            "https://example.com/bad.meta4",
            r#"<metalink><file name="../escape.bin"><url>https://example.com/file</url></file></metalink>"#,
        )
        .unwrap_err();
        assert!(error.contains("metalink_invalid_manifest"));
    }

    #[tokio::test]
    async fn hash_file_streams_supported_algorithms() {
        let path = std::env::temp_dir().join(format!(
            "vibe-metalink-hash-{}.bin",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(&path, b"abc").await.expect("write test file");

        let sha256 = hash_file(&path, ChecksumAlgorithm::Sha256)
            .await
            .expect("sha256");
        let md5 = hash_file(&path, ChecksumAlgorithm::Md5).await.expect("md5");

        let _ = fs::remove_file(&path).await;
        assert_eq!(
            sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(md5, "900150983cd24fb0d6963f7d28e17f72");
    }
}

/// F-4: Test-only exports for integration tests in `tests/metalink_engine.rs`.
/// `#[doc(hidden)]` keeps these out of rustdoc; they expose internal helpers
/// that have been stripped of `AppHandle` dependencies specifically so the
/// parallel mirror download path can be exercised end-to-end without a full
/// Tauri runtime.
#[doc(hidden)]
pub mod testing {
    pub use super::{
        all_part_files_absent, assemble_metalink_part_files, cleanup_metalink_part_files,
        download_metalink_range_from_mirror, part_file_path, MetalinkWorkerProgress,
    };

    /// Constructs a `MetalinkRangeWorker` with a failover queue and runs it
    /// to completion. Exercises the full failover orchestration path
    /// (primary mirror failure → retry → failover to next mirror) without
    /// requiring a Tauri `AppHandle`.
    ///
    /// Returns the total bytes downloaded by the worker.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_metalink_range_worker_with_failover(
        pool: sqlx::SqlitePool,
        task_id: String,
        file_id: String,
        client: reqwest::Client,
        request_headers: Vec<(String, String)>,
        speed_limiter: std::sync::Arc<crate::download::GlobalSpeedLimiter>,
        cancel_token: tokio_util::sync::CancellationToken,
        worker_index: usize,
        range_start: u64,
        range_end: u64,
        primary_mirror: crate::db::MetalinkResourceRecord,
        failover_mirrors: Vec<crate::db::MetalinkResourceRecord>,
        part_path: std::path::PathBuf,
        progress_tx: tokio::sync::mpsc::UnboundedSender<MetalinkWorkerProgress>,
    ) -> Result<i64, String> {
        use std::collections::VecDeque;
        let failover_queue: VecDeque<crate::db::MetalinkResourceRecord> =
            failover_mirrors.into_iter().collect();
        let failover_queue = std::sync::Arc::new(tokio::sync::Mutex::new(failover_queue));
        let worker = super::MetalinkRangeWorker {
            pool,
            task_id,
            file_id,
            client,
            request_headers,
            speed_limiter,
            cancel_token,
            worker_index,
            range_start,
            range_end,
            mirror: primary_mirror,
            part_path,
            failover_queue,
            progress_tx,
        };
        worker.run().await
    }
}
