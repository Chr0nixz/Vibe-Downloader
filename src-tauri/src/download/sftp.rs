use std::{
    collections::{HashMap, VecDeque},
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use russh::{
    client::{self, AuthResult, Config, Handle, Handler},
    keys::{
        ssh_key::{HashAlg, PrivateKey, PublicKey},
        PrivateKeyWithHashAlg,
    },
};
use russh_sftp::client::SftpSession;
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter},
    sync::{mpsc, Mutex},
    task::JoinSet,
};

use super::{
    engine::EngineFuture,
    file_ops::{finalize_download_file, persist_completed_path},
    read_with_idle_timeout, DownloadContext, DownloadEngine, DownloadError, GlobalSpeedLimiter,
    IdleReadOutcome, ProbeOutput, ProbeRequest, READ_IDLE_TIMEOUT,
};
use crate::{
    db,
    download::error::engine_error,
    download::retry::{with_retry_if, RetryPolicy},
    events::{emit_task_updated_record, TaskProgressEmitGate},
    models::{
        AppErrorPayload, EngineCapabilities, ProbedFile, SegmentStatus, SftpDirectoryEntry,
        SftpDirectoryProbe, TaskKind, TaskProgressPayload, TaskRecord, TaskSegmentRecord,
        TaskStatus,
    },
    proxy::{socks5_connect, AppProxyMode, ResolvedProxyConfig, SharedProxyConfig},
};

const PROTOCOL_SFTP: &str = "sftp";
const DEFAULT_SFTP_PORT: u16 = 22;
const SFTP_PROGRESS_INTERVAL: Duration = Duration::from_millis(300);
const SFTP_PROGRESS_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);
const SFTP_READ_BUFFER_SIZE: usize = 64 * 1024;
const SFTP_WORKER_BUFFER_SIZE: usize = 256 * 1024;
const SFTP_MAX_DYNAMIC_CONNECTIONS: usize = 2;
const SFTP_DYNAMIC_WARMUP: Duration = Duration::from_secs(8);
const SFTP_DYNAMIC_INTERVAL: Duration = Duration::from_secs(5);
const SFTP_MIN_SPLIT_REMAINING: i64 = 16 * 1024 * 1024;
const SFTP_WORKER_RETRIES: i32 = 2;

#[derive(Debug, Clone)]
pub struct SftpEngine {
    proxy_config: SharedProxyConfig,
}

#[derive(Debug, Clone)]
struct SftpTarget {
    sanitized_uri: String,
    host: String,
    port: u16,
    username: String,
    password: String,
    path: String,
    file_name: String,
    source_key: String,
    private_key_data: Option<String>,
    private_key_passphrase: Option<String>,
}

struct SftpConnection {
    session: SftpSession,
    _handle: Handle<SftpHostKeyHandler>,
}

#[derive(Clone)]
struct SftpHostKeyHandler {
    pool: SqlitePool,
    host: String,
    port: u16,
    failure: Arc<Mutex<Option<String>>>,
}

impl Handler for SftpHostKeyHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let algorithm = server_public_key.algorithm().as_str().to_string();
        let fingerprint = server_public_key
            .fingerprint(HashAlg::Sha256)
            .to_string()
            .trim_start_matches("SHA256:")
            .to_string();
        match db::verify_or_record_sftp_host_key(
            &self.pool,
            &self.host,
            self.port,
            &algorithm,
            &fingerprint,
        )
        .await
        {
            Ok(()) => Ok(true),
            Err(error) => {
                *self.failure.lock().await = Some(error);
                Ok(false)
            }
        }
    }
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
    dirty: bool,
}

#[derive(Debug)]
struct WorkerRequest {
    target: SftpTarget,
    task_id: String,
    segment: TaskSegmentRecord,
    range_end: Arc<AtomicI64>,
    pool: SqlitePool,
    temp_path: PathBuf,
    total_size: i64,
    cancel_token: tokio_util::sync::CancellationToken,
    speed_limiter: Arc<GlobalSpeedLimiter>,
    progress_tx: mpsc::UnboundedSender<WorkerProgress>,
    proxy_config: ResolvedProxyConfig,
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

impl SftpEngine {
    pub fn new(proxy_config: SharedProxyConfig) -> Self {
        Self { proxy_config }
    }

    async fn probe_target(
        &self,
        pool: &SqlitePool,
        target: SftpTarget,
        app: &Option<tauri::AppHandle>,
        request_id: &Option<String>,
    ) -> Result<ProbeOutput, String> {
        let proxy_config = self.proxy_config.read().await.clone();
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "verifying_host_key",
            Some("sftp"),
        );
        let connection = connect_sftp(pool, &target, &proxy_config).await?;
        crate::download::engine::emit_probe_phase(
            app,
            request_id,
            "querying_metadata",
            Some("sftp"),
        );
        let metadata = connection
            .session
            .metadata(&target.path)
            .await
            .map_err(|e| {
                engine_error(
                    "sftp_stat_failed",
                    format!("Could not inspect SFTP file: {e}"),
                    true,
                )
            })?;
        if metadata.is_dir() {
            return Err(engine_error(
                "sftp_directory_not_file",
                "SFTP URL points to a directory. Probe the directory and choose a file.",
                true,
            ));
        }
        let total_size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
        let last_modified = metadata.mtime.map(|mtime| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(i64::from(mtime), 0)
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()
        });
        let _ = connection.session.close().await;

        // The SFTP engine probes per-file metadata, so any non-empty file is a
        // candidate for parallel range reads. The actual decision to split into
        // multiple segments is made by the segment planner, which gates on the
        // configured multi-connection threshold (see `db::segment_planner`).
        let supports_resume = total_size > 0;
        let supports_parallel = supports_resume;
        Ok(ProbeOutput {
            protocol: PROTOCOL_SFTP.to_string(),
            task_kind: TaskKind::SingleFile,
            resolved_uri: target.sanitized_uri,
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
            hls_variants: Vec::new(),
            hls_audio_tracks: Vec::new(),
            hls_subtitle_tracks: Vec::new(),
            metalink: None,
        })
    }

    async fn run_download(&self, context: DownloadContext) -> Result<(), String> {
        let mut target = SftpTarget::parse_file(
            context
                .task
                .final_url
                .as_deref()
                .unwrap_or(&context.task.url),
        )?;
        if let Some(credentials) =
            db::resolve_task_credentials(&context.pool, &context.task.id).await?
        {
            target.username = credentials.username;
            target.password = credentials.password;
            target.private_key_data = credentials.private_key_data;
            target.private_key_passphrase = credentials.private_key_passphrase;
        }
        if target.username.is_empty() {
            return Err(engine_error(
                "sftp_credentials_required",
                "SFTP username and password are required. Include credentials in the URL when creating the task.",
                true,
            ));
        }
        let proxy_config = context.proxy_config.clone();
        run_sftp_download(target, context, proxy_config).await
    }
}

impl DownloadEngine for SftpEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_SFTP
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        scheme == PROTOCOL_SFTP
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            crate::download::engine::emit_probe_phase(
                &request.app,
                &request.request_id,
                "connecting",
                Some("sftp"),
            );
            let mut target = SftpTarget::parse_file(&request.uri).map_err(DownloadError::Other)?;
            let pool = request
                .pool
                .as_ref()
                .ok_or_else(|| {
                    engine_error(
                        "sftp_probe_state_unavailable",
                        "SFTP probe requires database state for host key verification.",
                        true,
                    )
                })
                .map_err(DownloadError::Other)?;
            // Prefer credentials passed directly in the request (from the dialog).
            if let Some(creds) = &request.credentials {
                if !creds.username.is_empty() {
                    target.username = creds.username.clone();
                }
                if !creds.password.is_empty() {
                    target.password = creds.password.clone();
                }
                if creds.private_key_data.is_some() {
                    target.private_key_data = creds.private_key_data.clone();
                    target.private_key_passphrase = creds.private_key_passphrase.clone();
                }
            }
            // Fall back to DB-stored credentials when a task_id is available.
            if target.username.is_empty() {
                if let Some(task_id) = request.task_id.as_deref() {
                    if let Some(credentials) = db::resolve_task_credentials(pool, task_id)
                        .await
                        .map_err(DownloadError::Other)?
                    {
                        target.username = credentials.username;
                        target.password = credentials.password;
                        target.private_key_data = credentials.private_key_data;
                        target.private_key_passphrase = credentials.private_key_passphrase;
                    }
                }
            }
            self.probe_target(pool, target, &request.app, &request.request_id)
                .await
                .map_err(DownloadError::Other)
        })
    }

    fn download<'a>(
        &'a self,
        context: DownloadContext,
    ) -> EngineFuture<'a, Result<(), DownloadError>> {
        Box::pin(async move {
            self.run_download(context)
                .await
                .map_err(DownloadError::Other)
        })
    }
}

pub async fn probe_sftp_directory_url(
    pool: &SqlitePool,
    input_url: &str,
    proxy_config: ResolvedProxyConfig,
) -> Result<SftpDirectoryProbe, String> {
    let target = SftpTarget::parse_directory(input_url)?;
    let mut diagnostics = Vec::new();
    let connection = connect_sftp(pool, &target, &proxy_config).await?;
    let canonical = connection.session.canonicalize(&target.path).await.ok();
    if let Some(canonical) = canonical.as_deref() {
        diagnostics.push(format!("REALPATH {canonical} succeeded"));
    }
    let mut entries = Vec::new();
    let read_dir = connection
        .session
        .read_dir(&target.path)
        .await
        .map_err(|e| {
            engine_error(
                "sftp_directory_probe_failed",
                format!("Could not list SFTP directory: {e}"),
                true,
            )
        })?;
    for entry in read_dir {
        let metadata = entry.metadata();
        let name = entry.file_name();
        let raw = format!(
            "{} {}",
            if metadata.is_dir() { "dir" } else { "file" },
            name
        );
        let probable_file_url = metadata
            .is_regular()
            .then(|| sftp_file_url(&target, &entry.path()));
        entries.push(SftpDirectoryEntry {
            name,
            raw,
            probable_file_url,
        });
    }
    diagnostics.push(format!("READDIR returned {} entries", entries.len()));
    let _ = connection.session.close().await;
    Ok(SftpDirectoryProbe {
        input_url: input_url.to_string(),
        directory_url: target.sanitized_uri,
        current_directory: canonical,
        entries,
        diagnostics,
    })
}

async fn run_sftp_download(
    target: SftpTarget,
    context: DownloadContext,
    proxy_config: ResolvedProxyConfig,
) -> Result<(), String> {
    let DownloadContext {
        app,
        pool,
        task,
        cancel_token,
        speed_limiter,
        connection_limit,
        ..
    } = context;

    let temp_path = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "SFTP task is missing a temporary path.".to_string())?;
    let final_path = task
        .final_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "SFTP task is missing a final path.".to_string())?;
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create the download directory: {e}"))?;
    }

    let mut segments = db::ensure_task_segments(&pool, &task).await?;
    if segments.is_empty() {
        return Err("SFTP task segment could not be created.".to_string());
    }
    // Reset the temp file when resuming from a fresh state (no segment has
    // any downloaded bytes yet) so we never append stale bytes from a prior
    // failed attempt. Mirrors the FTP engine's temp-file reset guard.
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
        connection_limit.clamp(1, SFTP_MAX_DYNAMIC_CONNECTIONS)
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
    let mut last_checkpoint = Instant::now();
    let mut progress_gate = TaskProgressEmitGate::default();
    let mut acceleration_disabled = false;
    let mut dynamic_failures = 0_i32;
    let retry_policy = RetryPolicy::sftp_connect();

    start_next_sftp_worker(
        &target,
        &task,
        &pool,
        &temp_path,
        &cancel_token,
        &speed_limiter,
        &progress_tx,
        &mut pending,
        &mut running,
        &mut workers,
        &proxy_config,
    );
    emit_sftp_progress(
        &mut progress,
        &mut last_checkpoint,
        &mut progress_gate,
        SftpProgressInput {
            app: &app,
            pool: &pool,
            task: &task,
            active_connections: running.len(),
            force_checkpoint: true,
            force_emit: true,
        },
    )
    .await?;

    while !pending.is_empty() || !running.is_empty() {
        if cancel_token.is_cancelled() {
            emit_sftp_progress(
                &mut progress,
                &mut last_checkpoint,
                &mut progress_gate,
                SftpProgressInput {
                    app: &app,
                    pool: &pool,
                    task: &task,
                    active_connections: running.len(),
                    force_checkpoint: true,
                    force_emit: true,
                },
            )
            .await?;
            progress_gate.flush(&app);
            return Ok(());
        }

        tokio::select! {
            Some(message) = progress_rx.recv() => {
                if let Some(segment) = progress.get_mut(&message.segment_id) {
                    segment.downloaded_until = message.downloaded_until;
                    segment.speed_bps = message.speed_bps;
                    segment.status = SegmentStatus::Downloading;
                    segment.dirty = true;
                }
                emit_sftp_progress(
                    &mut progress,
                    &mut last_checkpoint,
                    &mut progress_gate,
                    SftpProgressInput {
                        app: &app,
                        pool: &pool,
                        task: &task,
                        active_connections: running.len(),
                        force_checkpoint: false,
                        force_emit: false,
                    },
                ).await?;
            }
            Some(joined) = workers.join_next(), if !running.is_empty() => {
                let finished = joined.map_err(|error| format!("A SFTP worker stopped unexpectedly: {error}"))?;
                running.remove(&finished.segment_id);
                match finished.result {
                    Ok(downloaded_until) => {
                        if let Some(segment) = progress.get_mut(&finished.segment_id) {
                            segment.downloaded_until = downloaded_until;
                            segment.speed_bps = 0;
                            segment.status = SegmentStatus::Completed;
                            segment.dirty = true;
                        }
                    }
                    Err(_error) if cancel_token.is_cancelled() => return Ok(()),
                    Err(error) => {
                        let progress_len = progress.len();
                        let Some(segment) = progress.get_mut(&finished.segment_id) else {
                            return Err(error);
                        };
                        segment.status = SegmentStatus::Pending;
                        segment.retry_count += 1;
                        segment.speed_bps = 0;
                        segment.dirty = false;
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
                                None,
                                0,
                                1,
                                Some("SFTP acceleration disabled; continuing with one connection."),
                                None,
                            ).await?;
                            db::insert_task_event(&pool, &task.id, "sftp_acceleration_disabled", Some(&error)).await?;
                        }
                        if segment.retry_count > SFTP_WORKER_RETRIES {
                            return Err(error);
                        }
                        let delay = retry_policy.delay_for_attempt(
                            u32::try_from(segment.retry_count).unwrap_or(1),
                        );
                        if !delay.is_zero() {
                            tokio::select! {
                                _ = cancel_token.cancelled() => return Ok(()),
                                _ = tokio::time::sleep(delay) => {}
                            }
                        }
                        pending.push_front(TaskSegmentRecord {
                            id: finished.segment_id.clone(),
                            task_id: task.id.clone(),
                            file_id: None,
                            unit_kind: segment_unit_kind(&task.protocol, progress_len).to_string(),
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
                emit_sftp_progress(
                    &mut progress,
                    &mut last_checkpoint,
                    &mut progress_gate,
                    SftpProgressInput {
                        app: &app,
                        pool: &pool,
                        task: &task,
                        active_connections: running.len(),
                        force_checkpoint: true,
                        force_emit: true,
                    },
                ).await?;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }

        let effective_max = if acceleration_disabled {
            1
        } else {
            max_connections
        };
        while running.len() < effective_max && !pending.is_empty() {
            start_next_sftp_worker(
                &target,
                &task,
                &pool,
                &temp_path,
                &cancel_token,
                &speed_limiter,
                &progress_tx,
                &mut pending,
                &mut running,
                &mut workers,
                &proxy_config,
            );
        }

        if !acceleration_disabled
            && task.supports_parallel
            && task.total_size > 0
            && running.len() < max_connections
            && pending.is_empty()
            && started_at.elapsed() >= SFTP_DYNAMIC_WARMUP
            && last_split_at.elapsed() >= SFTP_DYNAMIC_INTERVAL
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
                        dirty: true,
                    },
                );
                pending.push_back(tail);
                last_split_at = Instant::now();
                db::insert_task_event(&pool, &task.id, "sftp_segment_split", None).await?;
            }
        }
    }

    if cancel_token.is_cancelled() {
        emit_sftp_progress(
            &mut progress,
            &mut last_checkpoint,
            &mut progress_gate,
            SftpProgressInput {
                app: &app,
                pool: &pool,
                task: &task,
                active_connections: running.len(),
                force_checkpoint: true,
                force_emit: true,
            },
        )
        .await?;
        progress_gate.flush(&app);
        return Ok(());
    }

    emit_sftp_progress(
        &mut progress,
        &mut last_checkpoint,
        &mut progress_gate,
        SftpProgressInput {
            app: &app,
            pool: &pool,
            task: &task,
            active_connections: running.len(),
            force_checkpoint: true,
            force_emit: true,
        },
    )
    .await?;

    let downloaded = total_downloaded_from_progress(&progress, task.total_size);
    if task.total_size > 0 && downloaded < task.total_size {
        return Err(engine_error(
            "sftp_size_mismatch",
            format!(
                "SFTP file size changed while downloading. Expected {}, got {downloaded}.",
                task.total_size
            ),
            true,
        ));
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

// SFTP worker launch mirrors the FTP engine's per-worker coordination
// boundary; keep it as a single focused function until SFTP runtime state
// earns its own coordinator module.
#[allow(clippy::too_many_arguments)]
fn start_next_sftp_worker(
    target: &SftpTarget,
    task: &TaskRecord,
    pool: &SqlitePool,
    temp_path: &Path,
    cancel_token: &tokio_util::sync::CancellationToken,
    speed_limiter: &Arc<GlobalSpeedLimiter>,
    progress_tx: &mpsc::UnboundedSender<WorkerProgress>,
    pending: &mut VecDeque<TaskSegmentRecord>,
    running: &mut HashMap<String, SegmentRuntime>,
    workers: &mut JoinSet<WorkerFinished>,
    proxy_config: &ResolvedProxyConfig,
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
        cancel_token: cancel_token.clone(),
        speed_limiter: speed_limiter.clone(),
        progress_tx: progress_tx.clone(),
        proxy_config: proxy_config.clone(),
    };
    workers.spawn(async move {
        let segment_id = request.segment.id.clone();
        let result = download_sftp_segment(request).await;
        WorkerFinished { segment_id, result }
    });
}

async fn download_sftp_segment(request: WorkerRequest) -> Result<i64, String> {
    let started = Instant::now();
    let range_label = if request.total_size > 0 {
        Some(format!(
            "READ offset={} end={}",
            request.segment.downloaded_until, request.segment.range_end
        ))
    } else {
        None
    };
    let result = download_sftp_segment_inner(&request).await;
    crate::download::diagnostics::persist_engine_diagnostic(
        crate::download::diagnostics::EngineDiagnosticContext {
            pool: &request.pool,
            task_id: &request.task_id,
            method: "SFTP READ",
            url: &request.target.sanitized_uri,
            range_header: range_label,
            status_code: result.as_ref().ok().map(|_| 200),
            content_length: None,
            error: result.as_ref().err().map(String::as_str),
            retry_count: 0,
            duration: started.elapsed(),
        },
    )
    .await;
    result
}

async fn download_sftp_segment_inner(request: &WorkerRequest) -> Result<i64, String> {
    // Each worker establishes its own SSH channel + SFTP subsystem so that
    // multiple segments can transfer in parallel (Path A from the PoC).
    let connection = connect_sftp(&request.pool, &request.target, &request.proxy_config).await?;
    let session = &connection.session;

    let mut offset = request
        .segment
        .downloaded_until
        .max(request.segment.range_start);
    db::update_segment_status(
        &request.pool,
        &request.segment.id,
        SegmentStatus::Downloading,
        None,
        None,
    )
    .await?;

    let mut remote = session.open(&request.target.path).await.map_err(|e| {
        engine_error(
            "sftp_open_failed",
            format!("Could not open SFTP file: {e}"),
            true,
        )
    })?;
    if offset > 0 {
        remote
            .seek(SeekFrom::Start(u64::try_from(offset).unwrap_or(0)))
            .await
            .map_err(|e| {
                engine_error(
                    "sftp_resume_failed",
                    format!("Could not seek SFTP file: {e}"),
                    true,
                )
            })?;
    }

    // Open the temp file for writing and seek to this segment's offset. Each
    // worker writes its own byte range; we never truncate because concurrent
    // workers may already be writing to other offsets in the same file.
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&request.temp_path)
        .await
        .map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not open SFTP temp file: {e}"))
                .command_error()
        })?;
    let mut file = BufWriter::with_capacity(SFTP_WORKER_BUFFER_SIZE, file);

    let mut buffer = vec![0_u8; SFTP_READ_BUFFER_SIZE];
    let mut last_emit = Instant::now();
    let mut last_emit_bytes = offset;

    loop {
        if request.cancel_token.is_cancelled() {
            let _ = file.flush().await;
            let _ = connection.session.close().await;
            return Err("Download canceled.".to_string());
        }

        let current_end = request.range_end.load(Ordering::SeqCst);
        let read_len = if request.total_size > 0 {
            if offset > current_end {
                break;
            }
            usize::try_from((current_end - offset + 1).min(SFTP_READ_BUFFER_SIZE as i64))
                .unwrap_or(SFTP_READ_BUFFER_SIZE)
        } else {
            SFTP_READ_BUFFER_SIZE
        };
        // E-1: wrap the read with an idle timeout so a stalled SFTP data
        // channel cannot hold the worker, connection slot, and queue slot
        // forever. The cancel token is raced alongside so a user-initiated
        // pause/cancel interrupts an in-flight (and potentially stalled)
        // read immediately. AsyncRead::read returns Result<usize, io::Error>;
        // map 0 → None (EOF) so the shared helper's Option<T> contract holds.
        let read_future = async {
            remote
                .read(&mut buffer[..read_len])
                .await
                .map(|n| if n == 0 { None } else { Some(n) })
        };
        let outcome = tokio::select! {
            _ = request.cancel_token.cancelled() => {
                let _ = file.flush().await;
                let _ = connection.session.close().await;
                return Err("Download canceled.".to_string());
            }
            outcome = read_with_idle_timeout(read_future, READ_IDLE_TIMEOUT) => outcome,
        };
        let read = match outcome {
            IdleReadOutcome::Data(n) => n,
            IdleReadOutcome::End => {
                if request.total_size > 0 && offset <= current_end {
                    let _ = connection.session.close().await;
                    return Err(engine_error(
                        "sftp_short_read",
                        "The SFTP download ended before all bytes were received.".to_string(),
                        true,
                    ));
                }
                break;
            }
            IdleReadOutcome::Error(e) => {
                let _ = connection.session.close().await;
                return Err(engine_error(
                    "sftp_read_failed",
                    format!("SFTP file read failed: {e}"),
                    true,
                ));
            }
            IdleReadOutcome::IdleTimeout => {
                let _ = connection.session.close().await;
                return Err(engine_error(
                    "sftp_read_timeout",
                    "SFTP connection stalled: no data received for 60 seconds.",
                    true,
                ));
            }
        };

        request.speed_limiter.throttle(read).await;
        file.write_all(&buffer[..read]).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write SFTP temp file: {e}"))
                .command_error()
        })?;
        offset += i64::try_from(read).unwrap_or(0);

        if last_emit.elapsed() >= SFTP_PROGRESS_INTERVAL {
            let elapsed = last_emit.elapsed().as_secs_f64().max(0.001);
            let speed = ((offset - last_emit_bytes).max(0) as f64 / elapsed).round() as i64;
            let _ = request.progress_tx.send(WorkerProgress {
                segment_id: request.segment.id.clone(),
                downloaded_until: offset,
                speed_bps: speed,
            });
            last_emit = Instant::now();
            last_emit_bytes = offset;
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Could not flush the SFTP temp file: {e}"))?;
    let _ = connection.session.close().await;
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

struct SftpProgressInput<'a> {
    app: &'a AppHandle,
    pool: &'a SqlitePool,
    task: &'a TaskRecord,
    active_connections: usize,
    force_checkpoint: bool,
    force_emit: bool,
}

async fn emit_sftp_progress(
    progress: &mut HashMap<String, SegmentProgress>,
    last_checkpoint: &mut Instant,
    progress_gate: &mut TaskProgressEmitGate,
    input: SftpProgressInput<'_>,
) -> Result<(), String> {
    let SftpProgressInput {
        app,
        pool,
        task,
        active_connections,
        force_checkpoint,
        force_emit,
    } = input;
    let downloaded = total_downloaded_from_progress(progress, task.total_size);
    let speed = progress.values().map(|segment| segment.speed_bps).sum();
    let connection_count = i32::try_from(active_connections).unwrap_or(i32::MAX);
    let dirty_segments = progress.values().any(|segment| segment.dirty);
    if force_checkpoint
        || (dirty_segments && last_checkpoint.elapsed() >= SFTP_PROGRESS_CHECKPOINT_INTERVAL)
    {
        db::update_task_progress(
            pool,
            &task.id,
            downloaded,
            speed,
            connection_count,
            TaskStatus::Downloading,
        )
        .await?;
        for (segment_id, segment) in progress.iter_mut().filter(|(_, segment)| segment.dirty) {
            db::update_segment_runtime_progress(
                pool,
                segment_id,
                segment.downloaded_until,
                segment.speed_bps,
                segment.status,
            )
            .await?;
            segment.dirty = false;
        }
        *last_checkpoint = Instant::now();
    }
    progress_gate.emit_or_store(
        app,
        TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: downloaded.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: speed.to_string(),
            connection_count,
            status: TaskStatus::Downloading,
        },
        force_emit,
    );
    Ok(())
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
                    dirty: false,
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

/// Pick the unit_kind for a re-queued segment based on the protocol and the
/// current segment count. Multi-segment SFTP uses `sftp_range`; the
/// single-segment fallback uses `sftp_file`.
fn segment_unit_kind(protocol: &str, segment_count: usize) -> &'static str {
    if protocol == PROTOCOL_SFTP && segment_count > 1 {
        "sftp_range"
    } else if protocol == PROTOCOL_SFTP {
        "sftp_file"
    } else {
        "http_range"
    }
}

fn split_largest_remaining_segment(
    task: &TaskRecord,
    progress: &mut HashMap<String, SegmentProgress>,
) -> Option<(TaskSegmentRecord, TaskSegmentRecord)> {
    let (segment_id, split) = progress
        .iter()
        .filter(|(_, segment)| segment.status == SegmentStatus::Downloading)
        .filter_map(|(id, segment)| planned_sftp_split(segment).map(|split| (id.clone(), split)))
        .max_by_key(|(_, split)| split.tail_end - split.tail_start)?;
    let unit_kind = segment_unit_kind(&task.protocol, progress.len());

    let head_progress = progress.get_mut(&segment_id)?;
    let original_end = head_progress.range_end;
    head_progress.range_end = split.head_end;
    let head = TaskSegmentRecord {
        id: segment_id.clone(),
        task_id: task.id.clone(),
        file_id: None,
        unit_kind: unit_kind.to_string(),
        range_start: head_progress.range_start,
        range_end: split.head_end,
        downloaded_until: head_progress.downloaded_until,
        speed_bps: head_progress.speed_bps,
        status: SegmentStatus::Downloading,
        retry_count: head_progress.retry_count,
        last_error: None,
    };
    let tail = TaskSegmentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        file_id: None,
        unit_kind: unit_kind.to_string(),
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
struct SftpSplit {
    head_end: i64,
    tail_start: i64,
    tail_end: i64,
}

fn planned_sftp_split(segment: &SegmentProgress) -> Option<SftpSplit> {
    let current = segment
        .downloaded_until
        .clamp(segment.range_start, segment.range_end.saturating_add(1));
    let remaining = segment.range_end.saturating_sub(current).saturating_add(1);
    if remaining < SFTP_MIN_SPLIT_REMAINING.saturating_mul(2) {
        return None;
    }
    let tail_start = current + remaining / 2;
    if tail_start <= current || tail_start > segment.range_end {
        return None;
    }
    Some(SftpSplit {
        head_end: tail_start - 1,
        tail_start,
        tail_end: segment.range_end,
    })
}

async fn connect_sftp(
    pool: &SqlitePool,
    target: &SftpTarget,
    proxy_config: &ResolvedProxyConfig,
) -> Result<SftpConnection, String> {
    if target.username.is_empty() {
        return Err(engine_error(
            "sftp_credentials_required",
            "SFTP username and password are required. Include credentials in the URL when creating the task.",
            true,
        ));
    }
    if matches!(proxy_config.mode, AppProxyMode::Custom) && !proxy_config.is_custom_socks5() {
        return Err(engine_error(
            "sftp_proxy_unsupported",
            "SFTP tasks only support SOCKS5 custom proxies.",
            true,
        ));
    }
    let handle = with_retry_if(
        &RetryPolicy::sftp_connect(),
        |_attempt| {
            let pool = pool.clone();
            let target_host = target.host.clone();
            let target_port = target.port;
            let proxy_config = proxy_config.clone();
            async move {
                let failure = Arc::new(Mutex::new(None));
                let handler = SftpHostKeyHandler {
                    pool,
                    host: target_host.clone(),
                    port: target_port,
                    failure: failure.clone(),
                };
                let config = Config {
                    nodelay: true,
                    ..Default::default()
                };
                let config = Arc::new(config);
                let handle_result = if proxy_config.is_custom_socks5() {
                    let proxy_url = proxy_config
                        .custom_socks5_url_with_auth()
                        .ok_or_else(|| "SOCKS5 proxy URL is not configured.".to_string())?;
                    let stream = socks5_connect(
                        &proxy_url,
                        proxy_config.username.as_deref(),
                        proxy_config.password.as_deref(),
                        &target_host,
                        target_port,
                    )
                    .await
                    .map_err(|e| {
                        engine_error(
                            "sftp_proxy_connect_failed",
                            format!("SFTP proxy connection failed: {e}"),
                            true,
                        )
                    })?;
                    client::connect_stream(config, stream, handler).await
                } else {
                    client::connect(config, (target_host.as_str(), target_port), handler).await
                };
                match handle_result {
                    Ok(handle) => Ok(handle),
                    Err(error) => {
                        let failure = failure.lock().await.clone();
                        Err(failure.unwrap_or_else(|| {
                            engine_error(
                                "sftp_connect_failed",
                                format!("Could not connect to SFTP server: {error}"),
                                true,
                            )
                        }))
                    }
                }
            }
        },
        |error| {
            // Don't retry permanent errors. Host-key mismatches (sftp_host_key_changed)
            // will never succeed on retry — the key won't change between attempts.
            // The error is a JSON-serialized AppErrorPayload; check for the code.
            !error.contains("sftp_host_key_changed")
        },
    )
    .await?;

    let mut handle = handle;

    // Try public key authentication first if a private key was provided.
    let mut authenticated = false;
    if let Some(key_pem) = target.private_key_data.as_deref() {
        let ssh_key_result = PrivateKey::from_openssh(key_pem);
        match ssh_key_result {
            Ok(mut key) => {
                if key.is_encrypted() {
                    let passphrase = target.private_key_passphrase.as_deref().unwrap_or("");
                    match key.decrypt(passphrase) {
                        Ok(decrypted) => key = decrypted,
                        Err(e) => {
                            return Err(engine_error(
                                "sftp_auth_failed",
                                format!("Failed to decrypt SSH private key: {e}"),
                                true,
                            ));
                        }
                    }
                }
                let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key), None);
                match handle
                    .authenticate_publickey(target.username.clone(), key_with_hash)
                    .await
                {
                    Ok(AuthResult::Success) => {
                        authenticated = true;
                    }
                    Ok(AuthResult::Failure { .. }) => {
                        tracing::debug!("SFTP public key auth failed, falling back to password");
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "SFTP public key auth error, falling back to password");
                    }
                }
            }
            Err(e) => {
                return Err(engine_error(
                    "sftp_auth_failed",
                    format!("Failed to parse SSH private key: {e}"),
                    true,
                ));
            }
        }
    }

    // Fall back to password authentication.
    if !authenticated {
        match handle
            .authenticate_password(target.username.clone(), target.password.clone())
            .await
            .map_err(|e| {
                engine_error(
                    "sftp_auth_failed",
                    format!("SFTP password authentication failed: {e}"),
                    true,
                )
            })? {
            AuthResult::Success => {}
            AuthResult::Failure { .. } => {
                return Err(engine_error(
                    "sftp_auth_failed",
                    "SFTP authentication failed.",
                    true,
                ));
            }
        }
    }
    let channel = handle.channel_open_session().await.map_err(|e| {
        engine_error(
            "sftp_channel_failed",
            format!("Could not open SFTP channel: {e}"),
            true,
        )
    })?;
    channel.request_subsystem(true, "sftp").await.map_err(|e| {
        engine_error(
            "sftp_subsystem_failed",
            format!("Could not start SFTP subsystem: {e}"),
            true,
        )
    })?;
    let session = SftpSession::new(channel.into_stream()).await.map_err(|e| {
        engine_error(
            "sftp_subsystem_failed",
            format!("Could not initialize SFTP session: {e}"),
            true,
        )
    })?;
    Ok(SftpConnection {
        session,
        _handle: handle,
    })
}

impl SftpTarget {
    fn parse_file(input: &str) -> Result<Self, String> {
        let target = Self::parse(input)?;
        if target.path == "/" || target.path.ends_with('/') {
            return Err(engine_error(
                "sftp_directory_not_file",
                "SFTP URL points to a directory. Probe the directory and choose a file.",
                true,
            ));
        }
        Ok(target)
    }

    fn parse_directory(input: &str) -> Result<Self, String> {
        let mut target = Self::parse(input)?;
        if !target.path.ends_with('/') {
            target.path.push('/');
        }
        target.file_name = target
            .path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("/")
            .to_string();
        Ok(target)
    }

    fn parse(input: &str) -> Result<Self, String> {
        let mut parsed = reqwest::Url::parse(input.trim())
            .map_err(|_| engine_error("sftp_invalid_url", "SFTP URL is invalid.", true))?;
        if parsed.scheme() != PROTOCOL_SFTP {
            return Err(format!(
                "The {} protocol is not supported by the SFTP engine.",
                parsed.scheme()
            ));
        }
        let host = parsed
            .host_str()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| engine_error("sftp_invalid_url", "SFTP URL is missing a host.", true))?;
        let port = parsed.port().unwrap_or(DEFAULT_SFTP_PORT);
        let path = percent_decode_lossy(parsed.path());
        if path.trim().is_empty() || !path.starts_with('/') {
            return Err(engine_error(
                "sftp_invalid_url",
                "SFTP URL must include an absolute remote path.",
                true,
            ));
        }
        let username = if parsed.username().is_empty() {
            String::new()
        } else {
            percent_decode_lossy(parsed.username())
        };
        let password = parsed
            .password()
            .map(percent_decode_lossy)
            .unwrap_or_default();
        if parsed.set_username("").is_err() {
            return Err(engine_error(
                "sftp_invalid_url",
                "Could not sanitize SFTP URL credentials.",
                true,
            ));
        }
        let _ = parsed.set_password(None);
        let raw_name = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("download")
            .to_string();
        let file_name = crate::download::sanitize::sanitize_single_file_name(&raw_name);
        Ok(Self {
            sanitized_uri: parsed.to_string(),
            host: host.clone(),
            port,
            username,
            password,
            path,
            file_name,
            source_key: format!("sftp://{host}:{port}"),
            private_key_data: None,
            private_key_passphrase: None,
        })
    }
}

fn sftp_file_url(target: &SftpTarget, path: &str) -> String {
    let encoded = encode_remote_path(path);
    let credentials = if target.username.is_empty() {
        String::new()
    } else if target.password.is_empty() {
        format!("{}@", percent_encode_segment(&target.username))
    } else {
        format!(
            "{}:{}@",
            percent_encode_segment(&target.username),
            percent_encode_segment(&target.password)
        )
    };
    if target.port == DEFAULT_SFTP_PORT {
        format!("sftp://{}{}{}", credentials, target.host, encoded)
    } else {
        format!(
            "sftp://{}{}:{}{}",
            credentials, target.host, target.port, encoded
        )
    }
}

fn encode_remote_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_segment(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}

fn percent_decode_lossy(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sftp_url_and_sanitizes_credentials() {
        let target =
            SftpTarget::parse_file("sftp://alice:s3cret@example.com:2222/dir/file%20name.bin")
                .expect("target");
        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 2222);
        assert_eq!(target.username, "alice");
        assert_eq!(target.password, "s3cret");
        assert_eq!(target.path, "/dir/file name.bin");
        assert_eq!(target.file_name, "file name.bin");
        assert_eq!(target.source_key, "sftp://example.com:2222");
        assert_eq!(
            target.sanitized_uri,
            "sftp://example.com:2222/dir/file%20name.bin"
        );
    }

    #[test]
    fn sftp_defaults_to_port_22_and_rejects_directories_as_files() {
        let target =
            SftpTarget::parse_file("sftp://alice:pass@example.com/file.bin").expect("target");
        assert_eq!(target.port, 22);
        assert_eq!(target.source_key, "sftp://example.com:22");

        let error = SftpTarget::parse_file("sftp://alice:pass@example.com/dir/")
            .expect_err("directory should not parse as file");
        assert!(error.contains("sftp_directory_not_file"));
    }

    #[test]
    fn encodes_directory_entry_urls_with_credentials_for_selection() {
        let target =
            SftpTarget::parse_directory("sftp://alice:pass@example.com:2200/dir/").expect("target");
        assert_eq!(
            sftp_file_url(&target, "/dir/file name.bin"),
            "sftp://alice:pass@example.com:2200/dir/file%20name.bin"
        );
    }
}
