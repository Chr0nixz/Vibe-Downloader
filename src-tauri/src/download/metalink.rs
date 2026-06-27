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
};

use super::{
    engine::EngineFuture,
    file_ops::finalize_download_file,
    http::build_client,
    DownloadContext, DownloadEngine, DownloadError, ProbeOutput, ProbeRequest,
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
        TaskFileRecord, TaskKind, TaskProgressPayload, TaskRecord,
        TaskStatus,
    },
    proxy::SharedProxyConfig,
};

const PROTOCOL_METALINK: &str = "metalink";
const METALINK_CONTENT_TYPE: &str = "application/metalink4+xml";
const DEFAULT_RESOURCE_PRIORITY: i64 = 999_999;

#[derive(Debug, Clone)]
pub struct MetalinkEngine {
    proxy_config: SharedProxyConfig,
}

impl MetalinkEngine {
    pub fn new(proxy_config: SharedProxyConfig) -> Self {
        Self { proxy_config }
    }

    async fn client(&self) -> Result<Client, String> {
        let config = self.proxy_config.read().await;
        build_client(&config)
    }

    async fn probe_metalink(
        &self,
        url: &str,
        request_headers: &[(String, String)],
    ) -> Result<MetalinkProbeData, String> {
        let bytes = fetch_manifest_bytes(&self.client().await?, url, request_headers).await?;
        let text = String::from_utf8(bytes).map_err(|_| {
            engine_error(
                "metalink_invalid_manifest",
                "Metalink manifest is not valid UTF-8.",
                false,
            )
        })?;
        parse_metalink_manifest(url, &text)
    }
}

impl DownloadEngine for MetalinkEngine {
    fn id(&self) -> &'static str {
        PROTOCOL_METALINK
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        matches!(scheme, "http" | "https" | "file")
    }

    fn probe<'a>(
        &'a self,
        request: ProbeRequest,
    ) -> EngineFuture<'a, Result<ProbeOutput, DownloadError>> {
        Box::pin(async move {
            let data = self
                .probe_metalink(&request.uri, &request.request_headers)
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
            Ok(ProbeOutput {
                protocol: PROTOCOL_METALINK.to_string(),
                task_kind: TaskKind::Manifest,
                resolved_uri: data.manifest_url.clone(),
                display_name: display_name_for_manifest(&request.uri, &data.files),
                total_size,
                source_key: format!("metalink:{}", data.manifest_url),
                capabilities: EngineCapabilities {
                    supports_resume: true,
                    supports_parallel: false,
                    supports_multi_file: data.files.len() > 1,
                },
                files,
                etag: None,
                last_modified: None,
                content_type: Some(METALINK_CONTENT_TYPE.to_string()),
                hls_variants: Vec::new(),
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
