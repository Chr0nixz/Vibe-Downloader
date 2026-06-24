use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
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
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::Mutex,
};

use super::{
    engine::EngineFuture,
    http::file::{finalize_download_file, persist_completed_path},
    DownloadContext, DownloadEngine, ProbeOutput, ProbeRequest,
};
use crate::{
    db,
    download::retry::{with_retry_if, RetryPolicy},
    events::{emit_task_progress, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        AppErrorPayload, EngineCapabilities, ProbedFile, RequestDiagnosticRecord, SegmentStatus,
        SftpDirectoryEntry, SftpDirectoryProbe, TaskKind, TaskProgressPayload, TaskRecord,
        TaskSegmentRecord, TaskStatus,
    },
    proxy::{socks5_connect, AppProxyMode, ResolvedProxyConfig, SharedProxyConfig},
};

const PROTOCOL_SFTP: &str = "sftp";
const DEFAULT_SFTP_PORT: u16 = 22;
const SFTP_PROGRESS_INTERVAL: Duration = Duration::from_millis(300);
const SFTP_READ_BUFFER_SIZE: usize = 64 * 1024;

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

impl SftpEngine {
    pub fn new(proxy_config: SharedProxyConfig) -> Self {
        Self { proxy_config }
    }

    async fn probe_target(
        &self,
        pool: &SqlitePool,
        target: SftpTarget,
    ) -> Result<ProbeOutput, String> {
        let proxy_config = self.proxy_config.read().await.clone();
        let connection = connect_sftp(pool, &target, &proxy_config).await?;
        let metadata = connection
            .session
            .metadata(&target.path)
            .await
            .map_err(|e| {
                sftp_error(
                    "sftp_stat_failed",
                    format!("Could not inspect SFTP file: {e}"),
                    true,
                )
            })?;
        if metadata.is_dir() {
            return Err(sftp_error(
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

        Ok(ProbeOutput {
            protocol: PROTOCOL_SFTP.to_string(),
            task_kind: TaskKind::SingleFile,
            resolved_uri: target.sanitized_uri,
            display_name: target.file_name.clone(),
            total_size,
            source_key: target.source_key,
            capabilities: EngineCapabilities {
                supports_resume: total_size > 0,
                supports_parallel: false,
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
            return Err(sftp_error(
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

    fn probe<'a>(&'a self, request: ProbeRequest) -> EngineFuture<'a, Result<ProbeOutput, String>> {
        Box::pin(async move {
            let mut target = SftpTarget::parse_file(&request.uri)?;
            let pool = request.pool.as_ref().ok_or_else(|| {
                sftp_error(
                    "sftp_probe_state_unavailable",
                    "SFTP probe requires database state for host key verification.",
                    true,
                )
            })?;
            // Prefer credentials passed directly in the request (from the dialog).
            if let Some(user) = request.username.as_deref() {
                if !user.is_empty() {
                    target.username = user.to_string();
                }
            }
            if request.password.is_some() {
                target.password = request.password.clone().unwrap_or_default();
            }
            if request.private_key_data.is_some() {
                target.private_key_data = request.private_key_data.clone();
                target.private_key_passphrase = request.private_key_passphrase.clone();
            }
            // Fall back to DB-stored credentials when a task_id is available.
            if target.username.is_empty() {
                if let Some(task_id) = request.task_id.as_deref() {
                    if let Some(credentials) = db::resolve_task_credentials(pool, task_id).await? {
                        target.username = credentials.username;
                        target.password = credentials.password;
                        target.private_key_data = credentials.private_key_data;
                        target.private_key_passphrase = credentials.private_key_passphrase;
                    }
                }
            }
            self.probe_target(pool, target).await
        })
    }

    fn download<'a>(&'a self, context: DownloadContext) -> EngineFuture<'a, Result<(), String>> {
        Box::pin(async move { self.run_download(context).await })
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
            sftp_error(
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
        cancel,
        speed_limiter,
        ..
    } = context;
    let connection = connect_sftp(&pool, &target, &proxy_config).await?;
    let metadata = connection
        .session
        .metadata(&target.path)
        .await
        .map_err(|e| {
            sftp_error(
                "sftp_stat_failed",
                format!("Could not inspect SFTP file: {e}"),
                true,
            )
        })?;
    let remote_size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    let segment = db::get_first_segment_record(&pool, &task.id)
        .await?
        .ok_or_else(|| "SFTP task is missing a work unit.".to_string())?;
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
    let downloaded = download_sftp_file(
        &app,
        &pool,
        &task,
        &segment,
        &connection.session,
        &target,
        remote_size,
        &temp_path,
        &cancel,
        &speed_limiter,
    )
    .await?;

    if cancel.load(Ordering::SeqCst) {
        pause_sftp_task(&app, &pool, &task, &segment, downloaded).await?;
        let _ = connection.session.close().await;
        return Ok(());
    }
    if remote_size > 0 && downloaded != remote_size {
        return Err(sftp_error(
            "sftp_size_mismatch",
            format!("SFTP file size changed while downloading. Expected {remote_size}, got {downloaded}."),
            true,
        ));
    }
    let completed_path = finalize_download_file(&temp_path, &final_path).await?;
    persist_completed_path(&pool, &task.id, &completed_path).await?;
    db::complete_task_segment(&pool, &task.id, &segment.id).await?;
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }
    let _ = connection.session.close().await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_sftp_file(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    segment: &TaskSegmentRecord,
    session: &SftpSession,
    target: &SftpTarget,
    remote_size: i64,
    temp_path: &Path,
    cancel: &Arc<AtomicBool>,
    speed_limiter: &Arc<crate::download::GlobalSpeedLimiter>,
) -> Result<i64, String> {
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not create SFTP temp folder: {e}"))
                .command_error()
        })?;
    }
    let mut resume_from = fs::metadata(temp_path)
        .await
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    if remote_size > 0 && resume_from > remote_size {
        fs::remove_file(temp_path).await.ok();
        resume_from = 0;
    }
    let started = Instant::now();
    let mut remote = session.open(&target.path).await.map_err(|e| {
        sftp_error(
            "sftp_open_failed",
            format!("Could not open SFTP file: {e}"),
            true,
        )
    })?;
    if resume_from > 0 {
        remote
            .seek(SeekFrom::Start(u64::try_from(resume_from).unwrap_or(0)))
            .await
            .map_err(|e| {
                sftp_error(
                    "sftp_resume_failed",
                    format!("Could not seek SFTP file: {e}"),
                    true,
                )
            })?;
    }
    persist_sftp_diagnostic(
        pool,
        task,
        "READ",
        (resume_from > 0).then(|| format!("bytes={resume_from}-")),
        None,
        None,
        started.elapsed(),
    )
    .await;

    let mut out = if resume_from > 0 {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(temp_path)
            .await
    } else {
        fs::File::create(temp_path).await
    }
    .map_err(|e| {
        AppErrorPayload::disk_write_failed(format!("Could not open SFTP temp file: {e}"))
            .command_error()
    })?;
    let mut downloaded = resume_from;
    let mut last_emit = Instant::now();
    let mut last_bytes = downloaded;
    let mut buffer = vec![0_u8; SFTP_READ_BUFFER_SIZE];

    loop {
        if cancel.load(Ordering::SeqCst) {
            out.flush()
                .await
                .map_err(|e| format!("Could not flush SFTP temp file: {e}"))?;
            return Ok(downloaded);
        }
        let read = remote.read(&mut buffer).await.map_err(|e| {
            sftp_error(
                "sftp_read_failed",
                format!("SFTP file read failed: {e}"),
                true,
            )
        })?;
        if read == 0 {
            break;
        }
        speed_limiter.throttle(read).await;
        out.write_all(&buffer[..read]).await.map_err(|e| {
            AppErrorPayload::disk_write_failed(format!("Could not write SFTP temp file: {e}"))
                .command_error()
        })?;
        downloaded = downloaded.saturating_add(i64::try_from(read).unwrap_or(0));
        if last_emit.elapsed() >= SFTP_PROGRESS_INTERVAL {
            let elapsed = last_emit.elapsed().as_secs_f64().max(0.001);
            let speed = ((downloaded - last_bytes).max(0) as f64 / elapsed).round() as i64;
            emit_sftp_progress(app, pool, task, segment, downloaded, speed).await?;
            last_emit = Instant::now();
            last_bytes = downloaded;
        }
    }
    out.flush()
        .await
        .map_err(|e| format!("Could not flush SFTP temp file: {e}"))?;
    emit_sftp_progress(app, pool, task, segment, downloaded, 0).await?;
    persist_sftp_diagnostic(
        pool,
        task,
        "READ",
        (resume_from > 0).then(|| format!("bytes={resume_from}-")),
        Some(downloaded.saturating_sub(resume_from)),
        None,
        started.elapsed(),
    )
    .await;
    Ok(downloaded)
}

async fn emit_sftp_progress(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    segment: &TaskSegmentRecord,
    downloaded: i64,
    speed_bps: i64,
) -> Result<(), String> {
    db::update_task_and_segment_progress(
        pool,
        &task.id,
        &segment.id,
        downloaded,
        speed_bps,
        1,
        TaskStatus::Downloading,
    )
    .await?;
    emit_task_progress(
        app,
        &TaskProgressPayload {
            task_id: task.id.clone(),
            downloaded_bytes: downloaded.to_string(),
            total_size: task.total_size.to_string(),
            speed_bps: speed_bps.to_string(),
            connection_count: 1,
            status: TaskStatus::Downloading,
        },
    );
    Ok(())
}

async fn pause_sftp_task(
    app: &AppHandle,
    pool: &SqlitePool,
    task: &TaskRecord,
    segment: &TaskSegmentRecord,
    downloaded: i64,
) -> Result<(), String> {
    db::update_task_progress(pool, &task.id, downloaded, 0, 0, TaskStatus::Paused).await?;
    db::update_segment_status(
        pool,
        &segment.id,
        SegmentStatus::Pending,
        Some(downloaded),
        None,
    )
    .await?;
    if let Some(current) = db::get_task_record(pool, &task.id).await? {
        emit_task_updated_record(app, pool, &current).await;
    }
    Ok(())
}

async fn connect_sftp(
    pool: &SqlitePool,
    target: &SftpTarget,
    proxy_config: &ResolvedProxyConfig,
) -> Result<SftpConnection, String> {
    if target.username.is_empty() {
        return Err(sftp_error(
            "sftp_credentials_required",
            "SFTP username and password are required. Include credentials in the URL when creating the task.",
            true,
        ));
    }
    if matches!(proxy_config.mode, AppProxyMode::Custom) && !proxy_config.is_custom_socks5() {
        return Err(sftp_error(
            "sftp_proxy_unsupported",
            "SFTP tasks only support SOCKS5 custom proxies.",
            true,
        ));
    }
    let handle = with_retry_if(&RetryPolicy::sftp_connect(), |_attempt| {
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
                    sftp_error(
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
                        sftp_error(
                            "sftp_connect_failed",
                            format!("Could not connect to SFTP server: {error}"),
                            true,
                        )
                    }))
                }
            }
        }
    }, |error| {
        // Don't retry permanent errors. Host-key mismatches (sftp_host_key_changed)
        // will never succeed on retry — the key won't change between attempts.
        // The error is a JSON-serialized AppErrorPayload; check for the code.
        !error.contains("sftp_host_key_changed")
    })
    .await?;

    let mut handle = handle;

    // Try public key authentication first if a private key was provided.
    let mut authenticated = false;
    if let Some(key_pem) = target.private_key_data.as_deref() {
        let ssh_key_result = PrivateKey::from_openssh(key_pem);
        match ssh_key_result {
            Ok(mut key) => {
                if key.is_encrypted() {
                    let passphrase = target
                        .private_key_passphrase
                        .as_deref()
                        .unwrap_or("");
                    match key.decrypt(passphrase) {
                        Ok(decrypted) => key = decrypted,
                        Err(e) => {
                            return Err(sftp_error(
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
                return Err(sftp_error(
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
                sftp_error(
                    "sftp_auth_failed",
                    format!("SFTP password authentication failed: {e}"),
                    true,
                )
            })? {
            AuthResult::Success => {}
            AuthResult::Failure { .. } => {
                return Err(sftp_error(
                    "sftp_auth_failed",
                    "SFTP authentication failed.",
                    true,
                ));
            }
        }
    }
    let channel = handle.channel_open_session().await.map_err(|e| {
        sftp_error(
            "sftp_channel_failed",
            format!("Could not open SFTP channel: {e}"),
            true,
        )
    })?;
    channel.request_subsystem(true, "sftp").await.map_err(|e| {
        sftp_error(
            "sftp_subsystem_failed",
            format!("Could not start SFTP subsystem: {e}"),
            true,
        )
    })?;
    let session = SftpSession::new(channel.into_stream()).await.map_err(|e| {
        sftp_error(
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
            return Err(sftp_error(
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
            .map_err(|_| sftp_error("sftp_invalid_url", "SFTP URL is invalid.", true))?;
        if parsed.scheme() != PROTOCOL_SFTP {
            return Err(format!(
                "The {} protocol is not supported by the SFTP engine.",
                parsed.scheme()
            ));
        }
        let host = parsed
            .host_str()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| sftp_error("sftp_invalid_url", "SFTP URL is missing a host.", true))?;
        let port = parsed.port().unwrap_or(DEFAULT_SFTP_PORT);
        let path = percent_decode_lossy(parsed.path());
        if path.trim().is_empty() || !path.starts_with('/') {
            return Err(sftp_error(
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
            return Err(sftp_error(
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

async fn persist_sftp_diagnostic(
    pool: &SqlitePool,
    task: &TaskRecord,
    method: &str,
    range_header: Option<String>,
    content_length: Option<i64>,
    error: Option<&str>,
    duration: Duration,
) {
    let record = RequestDiagnosticRecord {
        task_id: task.id.clone(),
        method: method.to_string(),
        url: sanitize_url(task.final_url.as_deref().unwrap_or(&task.url)),
        range_header,
        if_range_header: None,
        status_code: None,
        etag: None,
        last_modified: None,
        content_length,
        error_message: error.map(str::to_string),
        retry_count: 0,
        duration_ms: i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
    };
    if let Err(error) = db::insert_request_diagnostic(pool, &record).await {
        tracing::warn!(error = %error, "failed to persist sftp diagnostic");
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

fn sftp_error(code: &str, message: impl Into<String>, recoverable: bool) -> String {
    AppErrorPayload::new(
        code,
        message,
        recoverable,
        if recoverable {
            vec!["retry", "check_url"]
        } else {
            vec!["check_url"]
        },
    )
    .command_error()
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
