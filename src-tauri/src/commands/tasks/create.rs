use std::{collections::HashSet, path::PathBuf, time::Duration};

use base64::Engine as _;
use reqwest::{Client, Url};
use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::{
    db,
    download::{probe_error::ensure_structured_error, ProbeOutput, ProbeRequest},
    events::{emit_queue_changed_with_ids, emit_task_updated},
    logging::sanitize_url,
    models::{
        task::now_iso, AppErrorPayload, BatchImportItem, BatchImportResult, BrowserKind,
        ChecksumAlgorithm, HashVerificationStatus, MetalinkProbeData, ProbeTaskPayload, Task,
        TaskChecksumRecord, TaskFileRecord, TaskPriority, TaskProxyMode, TaskProxySettingsInput,
        TaskRecord, TaskStatus,
    },
    AppState,
};

use crate::commands::task_file_planning::{
    normalize_expected_hash, normalize_sha256, normalized_probe_files,
    sanitize_probe_relative_path, task_file_records_from_probe, task_stored_temp_path,
    unique_final_path_among,
};

use super::{is_bt_protocol, is_dash_url, is_metalink_url, is_torrent_url, task_payload};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub url: String,
    pub save_dir: Option<String>,
    pub file_name: Option<String>,
    /// Legacy SHA-256-only field. Prefer `expected_hash` + `expected_hash_algorithm`
    /// for multi-algorithm manual verification (F-5). Kept for backward compatibility
    /// with older callers and browser handoff payloads.
    pub expected_hash_sha256: Option<String>,
    /// New (F-5): expected hash digest in any supported algorithm. When provided,
    /// takes precedence over `expected_hash_sha256`. `expected_hash_algorithm`
    /// defaults to Sha256 when None.
    pub expected_hash: Option<String>,
    pub expected_hash_algorithm: Option<ChecksumAlgorithm>,
    pub task_speed_limit_bps: Option<String>,
    pub priority: Option<TaskPriority>,
    pub category_key: Option<String>,
    pub probe_snapshot: Option<ProbeTaskPayload>,
    pub selected_file_paths: Option<Vec<String>>,
    pub allow_duplicate: Option<bool>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub private_key_data: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub selected_hls_variant_uri: Option<String>,
    /// F-6: Selected audio track URIs from `#EXT-X-MEDIA TYPE=AUDIO`.
    /// When non-empty, the HLS engine downloads these alongside the video variant
    /// and muxes them into the final output via ffmpeg `-map`.
    pub selected_hls_audio_track_uris: Option<Vec<String>>,
    /// F-6: Selected subtitle track URIs from `#EXT-X-MEDIA TYPE=SUBTITLES`.
    pub selected_hls_subtitle_track_uris: Option<Vec<String>>,
    /// FUN-02: Optional per-task proxy override at create time.
    pub proxy_mode: Option<TaskProxyMode>,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub proxy_no_proxy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTaskInput {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub private_key_data: Option<String>,
    pub private_key_passphrase: Option<String>,
    /// UX-6: Frontend-generated correlation ID for probe-phase events.
    /// When `Some`, engines emit `probe-phase` events keyed by this ID.
    pub request_id: Option<String>,
    /// FUN-02: Optional per-task proxy override at probe time.
    pub proxy_mode: Option<TaskProxyMode>,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub proxy_no_proxy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportUrlsInput {
    pub input: String,
    pub save_dir: Option<String>,
    pub probe: Option<bool>,
    pub create: Option<bool>,
}

#[tauri::command]
#[specta::specta]
pub async fn probe_task(
    state: State<'_, AppState>,
    app: AppHandle,
    input: ProbeTaskInput,
) -> Result<ProbeTaskPayload, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a download URL.".to_string());
    }

    let request_headers = basic_auth_headers(input.username.as_deref(), input.password.as_deref());

    tracing::debug!(url = %sanitize_url(url), "probing download url");
    let engine = state.engine_registry.engine_for_uri(url)?;
    let global_proxy = state.engine_registry.proxy_config().await;
    let proxy_config = db::resolve_probe_proxy_config(
        &global_proxy,
        engine.id(),
        input.proxy_mode,
        input.proxy_url.as_deref(),
        input.proxy_username.as_deref(),
        input.proxy_password.as_deref(),
        input.proxy_no_proxy.as_deref(),
    )?;
    let credentials = if input.username.is_some()
        || input.password.is_some()
        || input.private_key_data.is_some()
        || input.private_key_passphrase.is_some()
    {
        Some(db::TaskCredentials {
            username: input.username.clone().unwrap_or_default(),
            password: input.password.clone().unwrap_or_default(),
            private_key_data: input.private_key_data.clone(),
            private_key_passphrase: input.private_key_passphrase.clone(),
        })
    } else {
        None
    };
    let probe = engine
        .probe(ProbeRequest {
            uri: url.to_string(),
            source: None,
            request_headers,
            pool: Some(state.pool.clone()),
            task_id: None,
            credentials,
            proxy_config: Some(proxy_config),
            app: Some(app),
            request_id: input.request_id,
        })
        .await
        .map_err(|e| ensure_structured_error(e.to_string()))?;
    tracing::debug!(
        url = %sanitize_url(url),
        total_size = probe.total_size,
        supports_parallel = probe.capabilities.supports_parallel,
        source_key = %probe.source_key,
        "probe completed"
    );
    Ok(probe_payload_from_output(url, probe))
}

#[allow(clippy::too_many_arguments)]
async fn resolve_create_probe(
    state: &AppState,
    url: &str,
    snapshot: Option<&ProbeTaskPayload>,
    request_headers: &[(String, String)],
    proxy_mode: Option<TaskProxyMode>,
    proxy_url: Option<&str>,
    proxy_username: Option<&str>,
    proxy_password: Option<&str>,
    proxy_no_proxy: Option<&str>,
) -> Result<ProbeOutput, String> {
    if let Some(probe) = snapshot.and_then(|snapshot| probe_output_from_snapshot(url, snapshot)) {
        return Ok(probe);
    }

    let engine = state.engine_registry.engine_for_uri(url)?;
    let global_proxy = state.engine_registry.proxy_config().await;
    let proxy_config = db::resolve_probe_proxy_config(
        &global_proxy,
        engine.id(),
        proxy_mode,
        proxy_url,
        proxy_username,
        proxy_password,
        proxy_no_proxy,
    )?;
    engine
        .probe(ProbeRequest {
            uri: url.to_string(),
            source: None,
            request_headers: request_headers.to_vec(),
            pool: Some(state.pool.clone()),
            task_id: None,
            credentials: None,
            proxy_config: Some(proxy_config),
            app: None,
            request_id: None,
        })
        .await
        .map_err(|e| ensure_structured_error(e.to_string()))
}

fn probe_payload_from_output(input_url: &str, probe: ProbeOutput) -> ProbeTaskPayload {
    ProbeTaskPayload {
        input_url: input_url.to_string(),
        final_url: probe.resolved_uri,
        file_name: probe.display_name,
        protocol: probe.protocol,
        task_kind: probe.task_kind,
        capabilities: probe.capabilities,
        files: probe.files,
        total_size: probe.total_size.to_string(),
        source_key: probe.source_key,
        content_type: probe.content_type,
        etag: probe.etag,
        last_modified: probe.last_modified,
        hls_variants: probe.hls_variants,
        hls_audio_tracks: probe.hls_audio_tracks,
        hls_subtitle_tracks: probe.hls_subtitle_tracks,
        probed_at: now_iso(),
    }
}

fn probe_output_from_snapshot(url: &str, snapshot: &ProbeTaskPayload) -> Option<ProbeOutput> {
    let snapshot_url = snapshot.input_url.trim();
    if snapshot_url != url.trim() {
        return None;
    }

    let probed_at = chrono::DateTime::parse_from_rfc3339(&snapshot.probed_at)
        .ok()?
        .with_timezone(&chrono::Utc);
    if chrono::Utc::now().signed_duration_since(probed_at) > chrono::Duration::minutes(5) {
        return None;
    }
    if snapshot.protocol == "metalink" {
        return None;
    }

    Some(ProbeOutput {
        protocol: snapshot.protocol.clone(),
        task_kind: snapshot.task_kind,
        resolved_uri: snapshot.final_url.clone(),
        display_name: snapshot.file_name.clone(),
        total_size: snapshot.total_size.parse::<i64>().ok()?,
        source_key: snapshot.source_key.clone(),
        capabilities: snapshot.capabilities.clone(),
        files: snapshot.files.clone(),
        etag: snapshot.etag.clone(),
        last_modified: snapshot.last_modified.clone(),
        content_type: snapshot.content_type.clone(),
        hls_variants: snapshot.hls_variants.clone(),
        hls_audio_tracks: snapshot.hls_audio_tracks.clone(),
        hls_subtitle_tracks: snapshot.hls_subtitle_tracks.clone(),
        metalink: None,
    })
}

fn selected_relative_paths_for_probe(
    input: Option<&[String]>,
    protocol: &str,
    files: &[crate::models::ProbedFile],
) -> Result<Option<HashSet<String>>, String> {
    if !(is_bt_protocol(protocol) || is_metalink_protocol(protocol)) || files.len() <= 1 {
        return Ok(None);
    }

    let Some(input) = input else {
        return Ok(None);
    };
    if input.is_empty() {
        return Err("Select at least one file to download.".to_string());
    }

    let available = files
        .iter()
        .map(|file| sanitize_probe_relative_path(&file.relative_path))
        .collect::<HashSet<_>>();
    let selected = input
        .iter()
        .map(|path| sanitize_probe_relative_path(path))
        .collect::<HashSet<_>>();

    if selected.is_empty() {
        return Err("Select at least one file to download.".to_string());
    }

    if let Some(unknown) = selected.iter().find(|path| !available.contains(*path)) {
        return Err(format!(
            "Selected file is not in the probe result: {unknown}"
        ));
    }

    Ok(Some(selected))
}

fn selected_total_size(
    files: &[crate::models::ProbedFile],
    selected_paths: Option<&HashSet<String>>,
) -> Option<i64> {
    let selected_paths = selected_paths?;
    Some(
        files
            .iter()
            .filter(|file| {
                let relative = sanitize_probe_relative_path(&file.relative_path);
                selected_paths.contains(&relative)
            })
            .map(|file| file.size.parse::<i64>().unwrap_or(0))
            .sum(),
    )
}

fn storage_url_for_input(url: &str) -> String {
    db::legacy_credentials_from_url(url)
        .map(|credentials| credentials.sanitized_url)
        .unwrap_or_else(|| url.to_string())
}

fn storage_url_for_probe(probe: &ProbeOutput) -> String {
    db::legacy_credentials_from_url(&probe.resolved_uri)
        .map(|credentials| credentials.sanitized_url)
        .unwrap_or_else(|| probe.resolved_uri.clone())
}

fn duplicate_source_key_for_probe(probe: &ProbeOutput) -> Option<&str> {
    probe
        .source_key
        .starts_with("bt:")
        .then_some(probe.source_key.as_str())
}

async fn duplicate_task_for_probe(
    state: &AppState,
    storage_url: &str,
    storage_final_url: &str,
    probe: &ProbeOutput,
) -> Result<Option<TaskRecord>, String> {
    db::find_duplicate_task_record(
        &state.pool,
        storage_url,
        Some(storage_final_url),
        duplicate_source_key_for_probe(probe),
    )
    .await
}

fn duplicate_import_item(
    raw_url: &str,
    normalized_url: String,
    task: &TaskRecord,
) -> BatchImportItem {
    BatchImportItem {
        input_url: raw_url.to_string(),
        normalized_url: Some(normalized_url),
        duplicate: true,
        valid: true,
        file_name: Some(task.file_name.clone()),
        total_size: Some(task.total_size.to_string()),
        content_type: task.content_type.clone(),
        supports_resume: task.supports_resume,
        error_message: Some(format!("Task already exists: {}", task.file_name)),
        task: None,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn create_task(
    app: AppHandle,
    state: State<'_, AppState>,
    input: CreateTaskInput,
) -> Result<Task, String> {
    create_task_with_state(app, state.inner(), input).await
}

#[tauri::command]
#[specta::specta]
pub async fn import_urls(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ImportUrlsInput,
) -> Result<BatchImportResult, String> {
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    let mut created_count = 0_i32;
    let mut failed_count = 0_i32;
    let mut duplicate_count = 0_i32;
    let should_probe = input.probe.unwrap_or(true);
    let should_create = input.create.unwrap_or(false);

    for raw_url in input
        .input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let parsed = match Url::parse(raw_url) {
            Ok(url)
                if matches!(
                    url.scheme(),
                    "http" | "https" | "ftp" | "ftps" | "sftp" | "webdav" | "webdavs" | "magnet"
                ) || is_torrent_url(&url)
                    || is_metalink_url(&url)
                    || is_dash_url(&url) =>
            {
                url
            }
            Ok(_) => {
                failed_count += 1;
                items.push(BatchImportItem {
                    input_url: raw_url.to_string(),
                    normalized_url: None,
                    duplicate: false,
                    valid: false,
                    file_name: None,
                    total_size: None,
                    content_type: None,
                    supports_resume: false,
                    error_message: Some(
                        "Only HTTP, HTTPS, FTP, FTPS, SFTP, WebDAV, magnet links, .torrent URLs, Metalink manifests, and MPEG-DASH MPDs are supported."
                            .to_string(),
                    ),
                    task: None,
                });
                continue;
            }
            Err(_) => {
                failed_count += 1;
                items.push(BatchImportItem {
                    input_url: raw_url.to_string(),
                    normalized_url: None,
                    duplicate: false,
                    valid: false,
                    file_name: None,
                    total_size: None,
                    content_type: None,
                    supports_resume: false,
                    error_message: Some("URL is invalid.".to_string()),
                    task: None,
                });
                continue;
            }
        };
        let normalized_url = parsed.to_string();
        if !seen.insert(normalized_url.clone()) {
            duplicate_count += 1;
            items.push(BatchImportItem {
                input_url: raw_url.to_string(),
                normalized_url: Some(normalized_url),
                duplicate: true,
                valid: true,
                file_name: None,
                total_size: None,
                content_type: None,
                supports_resume: false,
                error_message: Some("Duplicate URL in this import.".to_string()),
                task: None,
            });
            continue;
        }
        let storage_url = storage_url_for_input(&normalized_url);

        let mut item = BatchImportItem {
            input_url: raw_url.to_string(),
            normalized_url: Some(normalized_url.clone()),
            duplicate: false,
            valid: true,
            file_name: None,
            total_size: None,
            content_type: None,
            supports_resume: false,
            error_message: None,
            task: None,
        };

        if should_probe {
            let probe_result = match state.engine_registry.engine_for_uri(&normalized_url) {
                Ok(engine) => engine
                    .probe(ProbeRequest {
                        uri: normalized_url.clone(),
                        source: None,
                        request_headers: Vec::new(),
                        pool: Some(state.pool.clone()),
                        task_id: None,
                        credentials: None,
                        proxy_config: None,
                        app: None,
                        request_id: None,
                    })
                    .await
                    .map_err(|e| ensure_structured_error(e.to_string())),
                Err(error) => Err(ensure_structured_error(error)),
            };
            match probe_result {
                Ok(probe) => {
                    item.file_name = Some(probe.display_name.clone());
                    item.total_size = Some(probe.total_size.to_string());
                    item.content_type = probe.content_type.clone();
                    item.supports_resume = probe.capabilities.supports_resume;
                    let storage_final_url = storage_url_for_probe(&probe);
                    if let Some(duplicate) = duplicate_task_for_probe(
                        state.inner(),
                        &storage_url,
                        &storage_final_url,
                        &probe,
                    )
                    .await?
                    {
                        duplicate_count += 1;
                        items.push(duplicate_import_item(raw_url, normalized_url, &duplicate));
                        continue;
                    }
                }
                Err(error) => {
                    item.valid = false;
                    item.error_message = Some(error);
                    failed_count += 1;
                    items.push(item);
                    continue;
                }
            }
        } else if let Some(duplicate) =
            db::find_duplicate_task_record(&state.pool, &storage_url, None, None).await?
        {
            duplicate_count += 1;
            items.push(duplicate_import_item(raw_url, normalized_url, &duplicate));
            continue;
        }

        if should_create {
            match create_task_with_state(
                app.clone(),
                state.inner(),
                CreateTaskInput {
                    url: normalized_url,
                    save_dir: input.save_dir.clone(),
                    file_name: item.file_name.clone(),
                    expected_hash_sha256: None,
                    expected_hash: None,
                    expected_hash_algorithm: None,
                    task_speed_limit_bps: None,
                    priority: None,
                    category_key: None,
                    probe_snapshot: None,
                    selected_file_paths: None,
                    allow_duplicate: Some(false),
                    username: None,
                    password: None,
                    private_key_data: None,
                    private_key_passphrase: None,
                    selected_hls_variant_uri: None,
                    selected_hls_audio_track_uris: None,
                    selected_hls_subtitle_track_uris: None,
                    proxy_mode: None,
                    proxy_url: None,
                    proxy_username: None,
                    proxy_password: None,
                    proxy_no_proxy: None,
                },
            )
            .await
            {
                Ok(task) => {
                    created_count += 1;
                    item.task = Some(task);
                }
                Err(error) => {
                    failed_count += 1;
                    item.valid = false;
                    item.error_message = Some(error);
                }
            }
        }

        items.push(item);
    }

    Ok(BatchImportResult {
        items,
        created_count,
        failed_count,
        duplicate_count,
    })
}

pub(crate) async fn create_task_with_state(
    app: AppHandle,
    state: &AppState,
    input: CreateTaskInput,
) -> Result<Task, String> {
    create_task_with_state_and_headers(app, state, input, Vec::new(), None).await
}

pub(crate) async fn create_task_with_state_and_headers(
    app: AppHandle,
    state: &AppState,
    input: CreateTaskInput,
    request_headers: Vec<(String, String)>,
    source_browser: Option<BrowserKind>,
) -> Result<Task, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a download URL.".to_string());
    }
    let captured_credentials = db::legacy_credentials_from_url(url);
    let has_credentials =
        captured_credentials.is_some() || input.username.as_deref().is_some_and(|u| !u.is_empty());
    if has_credentials {
        crate::secure_headers::ensure_secret_encryption_available()?;
    }

    // Build auth headers from explicit credentials or URL-embedded credentials
    let auth_headers = {
        let mut headers = request_headers.clone();
        if let (Some(user), Some(pass)) = (input.username.as_deref(), input.password.as_deref()) {
            if !user.is_empty() {
                headers.extend(basic_auth_headers(Some(user), Some(pass)));
            }
        } else if let Some(ref creds) = captured_credentials {
            headers.extend(basic_auth_headers(
                Some(creds.username.as_str()),
                Some(creds.password.as_str()),
            ));
        }
        headers
    };

    let probe = resolve_create_probe(
        state,
        url,
        input.probe_snapshot.as_ref(),
        &auth_headers,
        input.proxy_mode,
        input.proxy_url.as_deref(),
        input.proxy_username.as_deref(),
        input.proxy_password.as_deref(),
        input.proxy_no_proxy.as_deref(),
    )
    .await?;
    let storage_url = captured_credentials
        .as_ref()
        .map(|credentials| credentials.sanitized_url.clone())
        .unwrap_or_else(|| url.to_string());
    let storage_final_url = storage_url_for_probe(&probe);
    if !input.allow_duplicate.unwrap_or(false) {
        if let Some(duplicate) =
            duplicate_task_for_probe(state, &storage_url, &storage_final_url, &probe).await?
        {
            return Err(AppErrorPayload::duplicate_task(&duplicate.file_name).command_error());
        }
    }
    let settings = db::get_settings(
        &state.pool,
        super::super::settings::default_download_dir(&app)?,
    )
    .await?;
    let save_dir = match input
        .save_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(&settings.default_save_dir),
    };
    std::fs::create_dir_all(&save_dir)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;

    let probe_files = normalized_probe_files(&probe);
    let selected_relative_paths = selected_relative_paths_for_probe(
        input.selected_file_paths.as_deref(),
        &probe.protocol,
        &probe_files,
    )?;
    let task_total_size = selected_total_size(&probe_files, selected_relative_paths.as_ref())
        .unwrap_or(probe.total_size);
    let uses_single_output_file = probe_files.len() == 1;
    let requested_file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| uses_single_output_file)
        .unwrap_or(&probe.display_name);
    let now = now_iso();
    // F-5: Multi-algorithm manual hash verification. `expected_hash` (new) takes
    // precedence over legacy `expected_hash_sha256`. The chosen algorithm flows
    // into the `task_checksums` row below; `tasks.expected_hash_sha256` is only
    // populated for Sha256 to preserve the legacy verify path and UI columns.
    let manual_hash_algorithm = input
        .expected_hash_algorithm
        .unwrap_or(ChecksumAlgorithm::Sha256);
    let manual_hash: Option<(ChecksumAlgorithm, String)> = if is_bt_protocol(&probe.protocol)
        || probe.protocol == "hls"
        || probe.protocol == "dash"
        || is_metalink_protocol(&probe.protocol)
    {
        None
    } else if let Some(normalized) =
        normalize_expected_hash(input.expected_hash.as_deref(), manual_hash_algorithm)?
    {
        Some((manual_hash_algorithm, normalized))
    } else {
        // Fall back to legacy SHA-256-only field for backward compat with older
        // callers (browser handoff, CLI, older UI builds).
        normalize_sha256(input.expected_hash_sha256.as_deref())?
            .map(|hash| (ChecksumAlgorithm::Sha256, hash))
    };
    // Legacy column: only populated when algorithm is Sha256 (preserves the
    // existing verify_task_hash_with_pool fast path and TaskRecord UI fields).
    let expected_hash_sha256 = match &manual_hash {
        Some((ChecksumAlgorithm::Sha256, hash)) => Some(hash.clone()),
        _ => None,
    };
    let hash_status = if manual_hash.is_some() {
        HashVerificationStatus::Pending
    } else {
        HashVerificationStatus::NotRequested
    };
    let task_speed_limit_bps = input
        .task_speed_limit_bps
        .as_deref()
        .and_then(db::normalize_speed_limit_bps);
    let priority = input.priority.unwrap_or(TaskPriority::Normal);
    let mut category_key = input
        .category_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    // Auto-classification: when the user has not explicitly specified a category_key, match by classification rules
    if category_key.is_none() {
        if let Ok(rules) = db::list_classification_rules(&state.pool).await {
            if let Some(target_subdir) =
                db::apply_classification_rules(&storage_url, requested_file_name, "", &rules)
            {
                category_key = Some(target_subdir);
            }
        }
    }
    // Security check: category_key is used as a subdirectory name; path separators and parent directory references are forbidden
    if let Some(ref key) = category_key {
        if key.contains('/') || key.contains('\\') || key.contains("..") || key.is_empty() {
            log::warn!(
                "Classification rule returned invalid target_subdir {:?}, ignoring",
                key
            );
            category_key = None;
        }
    }
    // Compute the actual save directory: if a classification rule matched or the user specified a category_key, append the subdirectory
    let effective_save_dir = if let Some(ref subdir) = category_key {
        let target = save_dir.join(subdir);
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("Could not create the classification subdirectory: {e}"))?;
        // Path traversal check: after canonicalize, confirm target is under save_dir
        let canonical_save = save_dir.canonicalize().unwrap_or_else(|_| save_dir.clone());
        let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
        if !canonical_target.starts_with(&canonical_save) {
            return Err(format!(
                "Classification target directory escapes the save directory: {}",
                target.display()
            ));
        }
        target
    } else {
        save_dir.clone()
    };
    let queue_position = db::next_queue_position(&state.pool).await?;
    let task_id = Uuid::new_v4().to_string();

    let task_url = match input.selected_hls_variant_uri.as_deref() {
        Some(variant_uri) if probe.protocol == "hls" => reqwest::Url::parse(&storage_url)
            .ok()
            .and_then(|base| base.join(variant_uri).ok())
            .map(|u| u.to_string())
            .unwrap_or_else(|| storage_url.clone()),
        _ => storage_url.clone(),
    };

    // ARC-02: reserve final/temp paths inside the create transaction so concurrent
    // same-name tasks cannot share an output path. Partial UNIQUE indexes enforce
    // the reservation; retry when a concurrent creator wins the same path.
    let (record, final_path, temp_path, file_name) = {
        const MAX_PATH_RESERVE_ATTEMPTS: usize = 32;
        let mut last_error = None;
        let mut outcome = None;
        for _ in 0..MAX_PATH_RESERVE_ATTEMPTS {
            let mut tx = state.pool.begin().await.map_err(|e| e.to_string())?;
            let reserved = db::list_reserved_final_paths(&mut *tx).await?;
            let final_path =
                unique_final_path_among(&effective_save_dir, requested_file_name, &reserved);
            let file_name = final_path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    crate::download::sanitize::sanitize_single_file_name(requested_file_name)
                });
            let temp_path =
                task_stored_temp_path(&probe.protocol, &effective_save_dir, &final_path, &task_id);

            let record = TaskRecord {
                id: task_id.clone(),
                url: task_url.clone(),
                final_url: Some(storage_final_url.clone()),
                protocol: probe.protocol.clone(),
                task_kind: probe.task_kind,
                file_name: file_name.clone(),
                save_dir: save_dir.to_string_lossy().to_string(),
                temp_path: Some(temp_path.to_string_lossy().to_string()),
                final_path: Some(final_path.to_string_lossy().to_string()),
                total_size: task_total_size,
                downloaded_bytes: 0,
                status: TaskStatus::Queued,
                etag: probe.etag.clone(),
                last_modified: probe.last_modified.clone(),
                content_type: probe.content_type.clone(),
                supports_resume: probe.capabilities.supports_resume,
                supports_parallel: probe.capabilities.supports_parallel,
                supports_multi_file: probe.capabilities.supports_multi_file,
                source_key: probe.source_key.clone(),
                connection_count: 0,
                speed_bps: 0,
                task_speed_limit_bps: task_speed_limit_bps.clone(),
                priority,
                queue_position,
                category_key: category_key.clone(),
                obey_schedule: true,
                health_summary: Some("Queued".to_string()),
                error_message: None,
                error_code: None,
                recovery_actions: Vec::new(),
                retry_after_at: None,
                expected_hash_sha256: expected_hash_sha256.clone(),
                actual_hash_sha256: None,
                hash_status,
                hash_error: None,
                hash_verified_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
                files_version: 0,
            };

            if !input.allow_duplicate.unwrap_or(false) {
                let bt_source_key = if record.source_key.starts_with("bt:") {
                    Some(record.source_key.as_str())
                } else {
                    None
                };
                if let Some(duplicate) = db::find_duplicate_task_record(
                    &mut *tx,
                    &record.url,
                    record.final_url.as_deref(),
                    bt_source_key,
                )
                .await?
                {
                    return Err(
                        AppErrorPayload::duplicate_task(&duplicate.file_name).command_error()
                    );
                }
            }

            match db::insert_task_record_in_tx(&mut tx, &record).await {
                Ok(()) => match tx.commit().await {
                    Ok(()) => {
                        outcome = Some((record, final_path, temp_path, file_name));
                        break;
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if is_final_path_unique_conflict(&message) {
                            last_error = Some(message);
                            continue;
                        }
                        return Err(message);
                    }
                },
                Err(error) if is_final_path_unique_conflict(&error) => {
                    last_error = Some(error);
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        outcome.ok_or_else(|| {
            last_error.unwrap_or_else(|| {
                "Could not reserve a unique output path for the download.".to_string()
            })
        })?
    };
    // F-6: Store the user's selected HLS audio/subtitle track URIs. Uses
    // `upsert_hls_task` with placeholder values for fields that will be
    // filled in by `run_hls_download` when it probes the media playlist.
    // The COALESCE in the ON CONFLICT clause preserves these values when
    // `run_hls_download` passes `None`.
    if probe.protocol == "hls" {
        let audio_json = input
            .selected_hls_audio_track_uris
            .as_ref()
            .filter(|uris| !uris.is_empty())
            .and_then(|uris| serde_json::to_string(uris).ok());
        let subtitle_json = input
            .selected_hls_subtitle_track_uris
            .as_ref()
            .filter(|uris| !uris.is_empty())
            .and_then(|uris| serde_json::to_string(uris).ok());
        if audio_json.is_some() || subtitle_json.is_some() {
            let staging = temp_path.to_string_lossy();
            db::upsert_hls_task(
                &state.pool,
                db::HlsTaskUpsert {
                    task_id: &record.id,
                    input_url: &record.url,
                    media_url: &probe.resolved_uri,
                    playlist_kind: "vod",
                    selected_bandwidth: None,
                    selected_resolution: None,
                    target_duration: 0,
                    last_media_sequence: None,
                    output_format: "mp4",
                    staging_dir: &staging,
                    selected_audio_track_uris: audio_json.as_deref(),
                    selected_subtitle_track_uris: subtitle_json.as_deref(),
                },
            )
            .await?;
        }
    }
    if let Some((algorithm, expected_hash)) = manual_hash.as_ref() {
        let checksum = TaskChecksumRecord {
            id: Uuid::new_v4().to_string(),
            task_id: record.id.clone(),
            file_id: None,
            algorithm: *algorithm,
            expected_hash: expected_hash.clone(),
            actual_hash: None,
            status: HashVerificationStatus::Pending,
            source_kind: "manual".to_string(),
            source_url: None,
            source_label: None,
            is_primary: true,
            weak: algorithm.is_weak(),
            error_message: None,
            discovered_at: None,
            verified_at: None,
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
        };
        db::insert_task_checksum_record(&state.pool, &checksum).await?;
    }
    spawn_checksum_sidecar_discovery(
        &app,
        &state.pool,
        &record.id,
        probe.resolved_uri.as_str(),
        &request_headers,
        &record.created_at,
    );
    if let (Some(user), Some(pass)) = (input.username.as_deref(), input.password.as_deref()) {
        if !user.is_empty() {
            db::upsert_task_credentials(
                &state.pool,
                &record.id,
                &record.protocol,
                user,
                pass,
                input.private_key_data.as_deref(),
                input.private_key_passphrase.as_deref(),
            )
            .await?;
        }
    } else if let Some(credentials) = captured_credentials {
        db::upsert_task_credentials(
            &state.pool,
            &record.id,
            &record.protocol,
            &credentials.username,
            &credentials.password,
            None,
            None,
        )
        .await?;
    }
    if input.proxy_mode.is_some() {
        db::upsert_task_proxy_settings(
            &state.pool,
            TaskProxySettingsInput {
                task_id: record.id.clone(),
                mode: input.proxy_mode.unwrap_or(TaskProxyMode::Inherit),
                proxy_url: input.proxy_url.clone(),
                proxy_username: input.proxy_username.clone(),
                proxy_password: input.proxy_password.clone(),
                clear_proxy_password: None,
                no_proxy: input.proxy_no_proxy.clone(),
            },
        )
        .await?;
    }
    db::insert_task_event(&state.pool, &record.id, "created", None).await?;
    let mut reserved_final_paths = db::list_reserved_final_paths(&state.pool).await?;
    let file_records = task_file_records_from_probe(
        &record,
        &probe_files,
        &save_dir,
        &final_path,
        &temp_path,
        &file_name,
        selected_relative_paths.as_ref(),
        &mut reserved_final_paths,
    )?;
    for file_record in &file_records {
        db::insert_task_file_record(&state.pool, file_record).await?;
    }
    if let Some(metalink) = probe.metalink.as_ref() {
        persist_metalink_probe(&state.pool, &record, &file_records, metalink).await?;
    }
    db::ensure_task_segments_with_settings(&state.pool, &record, &settings).await?;
    if is_bt_protocol(&record.protocol) {
        if let Some(info_hash) = record.source_key.strip_prefix("bt:") {
            db::upsert_torrent_task(
                &state.pool,
                &record.id,
                db::TorrentTaskUpsert {
                    info_hash,
                    name: &record.file_name,
                    magnet_uri: record
                        .url
                        .starts_with("magnet:")
                        .then_some(record.url.as_str()),
                    torrent_blob: None,
                    piece_length: 0,
                    piece_count: 0,
                    private: false,
                    trackers_json: None,
                    seeding_enabled: false,
                    seed_ratio_limit: None,
                    seed_time_limit_seconds: None,
                },
            )
            .await?;
        }
    }
    if !request_headers.is_empty() {
        state
            .request_headers
            .lock()
            .await
            .insert(record.id.clone(), request_headers.clone());
        if let Err(error) = db::upsert_task_request_headers(
            &state.pool,
            &record.id,
            &request_headers,
            source_browser,
        )
        .await
        {
            tracing::warn!(
                task_id = %record.id,
                error = %error,
                "browser request headers kept in memory only"
            );
            db::insert_task_event(
                &state.pool,
                &record.id,
                "headers_not_persisted",
                Some(&error),
            )
            .await?;
        }
    }
    tracing::info!(
        task_id = %record.id,
        url = %sanitize_url(url),
        file_name = %record.file_name,
        total_size = record.total_size,
        "task created"
    );
    emit_queue_changed_with_ids(&app, Some(vec![record.id.clone()]));
    state
        .scheduler
        .clone()
        .dispatch(app.clone(), state.pool.clone())
        .await;

    let task = task_payload(&state.pool, &record.id).await?;
    emit_task_updated(&app, &task);
    Ok(task)
}

fn spawn_checksum_sidecar_discovery(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    task_id: &str,
    final_url: &str,
    request_headers: &[(String, String)],
    timestamp: &str,
) {
    let app = app.clone();
    let pool = pool.clone();
    let task_id = task_id.to_string();
    let final_url = final_url.to_string();
    let request_headers = request_headers.to_vec();
    let timestamp = timestamp.to_string();

    tauri::async_runtime::spawn(async move {
        let mut headers = request_headers;
        if let Ok(Some(credentials)) = db::resolve_task_credentials(&pool, &task_id).await {
            headers.extend(basic_auth_headers(
                Some(credentials.username.as_str()),
                Some(credentials.password.as_str()),
            ));
        }
        let checksums =
            discover_checksum_sidecars(&task_id, &final_url, &headers, &timestamp).await;
        if checksums.is_empty() {
            return;
        }

        let mut inserted = 0usize;
        for checksum in checksums {
            match db::insert_task_checksum_record(&pool, &checksum).await {
                Ok(()) => inserted += 1,
                Err(error) => {
                    tracing::warn!(
                        task_id = %task_id,
                        error = %error,
                        "failed to persist discovered checksum sidecar"
                    );
                }
            }
        }

        if inserted == 0 {
            return;
        }

        let _ = db::insert_task_event(
            &pool,
            &task_id,
            "checksums_discovered",
            Some(&inserted.to_string()),
        )
        .await;
        if let Some(app_state) = app.try_state::<crate::AppState>() {
            match super::task_payload(&app_state.pool, &task_id).await {
                Ok(task) => crate::events::emit_task_updated(&app, &task),
                Err(error) => tracing::warn!(
                    task_id = %task_id,
                    error = %error,
                    "failed to emit checksum sidecar update"
                ),
            }
        }
    });
}

async fn persist_metalink_probe(
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
    file_records: &[TaskFileRecord],
    metalink: &MetalinkProbeData,
) -> Result<(), String> {
    db::upsert_metalink_task(
        pool,
        db::MetalinkTaskUpsert {
            task_id: &task.id,
            manifest_url: &metalink.manifest_url,
            manifest_format: &metalink.manifest_format,
            file_count: i64::try_from(metalink.files.len()).unwrap_or(i64::MAX),
        },
    )
    .await?;
    let now = now_iso();
    for file in &metalink.files {
        let Some(record) = file_records.iter().find(|record| {
            sanitize_probe_relative_path(&record.relative_path) == file.relative_path
        }) else {
            continue;
        };
        for resource in &file.resources {
            let resource_id = Uuid::new_v4().to_string();
            db::insert_metalink_resource(
                pool,
                db::MetalinkResourceInsert {
                    id: &resource_id,
                    task_id: &task.id,
                    file_id: &record.id,
                    url: &resource.url,
                    priority: resource.priority,
                    location: resource.location.as_deref(),
                },
            )
            .await?;
        }
        let mut checksums = file.checksums.clone();
        checksums.sort_by_key(|checksum| match checksum.algorithm {
            ChecksumAlgorithm::Sha256 => 0,
            ChecksumAlgorithm::Sha512 => 1,
            ChecksumAlgorithm::Sha1 => 2,
            ChecksumAlgorithm::Md5 => 3,
        });
        for (index, checksum) in checksums.iter().enumerate() {
            db::insert_task_checksum_record(
                pool,
                &TaskChecksumRecord {
                    id: Uuid::new_v4().to_string(),
                    task_id: task.id.clone(),
                    file_id: Some(record.id.clone()),
                    algorithm: checksum.algorithm,
                    expected_hash: checksum.value.clone(),
                    actual_hash: None,
                    status: HashVerificationStatus::Pending,
                    source_kind: "metalink".to_string(),
                    source_url: Some(metalink.manifest_url.clone()),
                    source_label: Some(record.relative_path.clone()),
                    is_primary: index == 0,
                    weak: checksum.weak,
                    error_message: None,
                    discovered_at: Some(now.clone()),
                    verified_at: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn discover_checksum_sidecars(
    task_id: &str,
    final_url: &str,
    request_headers: &[(String, String)],
    timestamp: &str,
) -> Vec<TaskChecksumRecord> {
    let Ok(parsed) = Url::parse(final_url) else {
        return Vec::new();
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return Vec::new();
    }

    let Ok(client) = Client::builder().timeout(Duration::from_secs(3)).build() else {
        return Vec::new();
    };
    let mut checksums = Vec::new();
    for (algorithm, suffix) in [
        (ChecksumAlgorithm::Sha256, ".sha256"),
        (ChecksumAlgorithm::Sha512, ".sha512"),
        (ChecksumAlgorithm::Sha1, ".sha1"),
        (ChecksumAlgorithm::Md5, ".md5"),
    ] {
        let source_url = format!("{final_url}{suffix}");
        let mut request = client.get(&source_url);
        for (name, value) in request_headers {
            request = request.header(name, value);
        }
        let Ok(response) = request.send().await else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(text) = response.text().await else {
            continue;
        };
        let Some(expected_hash) = first_checksum_token(&text, algorithm) else {
            continue;
        };
        checksums.push(TaskChecksumRecord {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            file_id: None,
            algorithm,
            expected_hash,
            actual_hash: None,
            status: HashVerificationStatus::Pending,
            source_kind: "sidecar".to_string(),
            source_url: Some(source_url),
            source_label: Some(suffix.trim_start_matches('.').to_string()),
            is_primary: algorithm == ChecksumAlgorithm::Sha256,
            weak: algorithm.is_weak(),
            error_message: None,
            discovered_at: Some(timestamp.to_string()),
            verified_at: None,
            created_at: timestamp.to_string(),
            updated_at: timestamp.to_string(),
        });
    }
    checksums
}

fn first_checksum_token(text: &str, algorithm: ChecksumAlgorithm) -> Option<String> {
    let expected_len = match algorithm {
        ChecksumAlgorithm::Md5 => 32,
        ChecksumAlgorithm::Sha1 => 40,
        ChecksumAlgorithm::Sha256 => 64,
        ChecksumAlgorithm::Sha512 => 128,
    };
    text.split(|ch: char| !ch.is_ascii_hexdigit())
        .find(|token| token.len() == expected_len)
        .map(|token| token.to_ascii_lowercase())
}

fn is_metalink_protocol(protocol: &str) -> bool {
    protocol == "metalink"
}

/// Build an `Authorization: Basic` header from optional username and password.
/// Returns an empty vec if username is None or empty.
fn basic_auth_headers(username: Option<&str>, password: Option<&str>) -> Vec<(String, String)> {
    let user = username.unwrap_or("").trim();
    if user.is_empty() {
        return Vec::new();
    }
    let pass = password.unwrap_or("");
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
    vec![("Authorization".to_string(), format!("Basic {encoded}"))]
}

fn is_final_path_unique_conflict(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("unique")
        && (lower.contains("final_path")
            || lower.contains("idx_tasks_final_path_active")
            || lower.contains("idx_task_files_final_path_selected"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{EngineCapabilities, ProbedFile, TaskKind};

    #[test]
    fn fresh_probe_snapshot_for_same_url_is_reused() {
        let snapshot = probe_snapshot("https://example.com/file.bin", chrono::Utc::now());

        let probe = probe_output_from_snapshot("https://example.com/file.bin", &snapshot)
            .expect("fresh snapshot should be accepted");

        assert_eq!(probe.resolved_uri, "https://cdn.example.com/file.bin");
        assert_eq!(probe.total_size, 1024);
        assert_eq!(probe.etag.as_deref(), Some("\"strong\""));
    }

    #[test]
    fn probe_snapshot_is_ignored_when_url_differs_or_is_expired() {
        let fresh = probe_snapshot("https://example.com/file.bin", chrono::Utc::now());
        assert!(probe_output_from_snapshot("https://example.com/other.bin", &fresh).is_none());

        let expired = probe_snapshot(
            "https://example.com/file.bin",
            chrono::Utc::now() - chrono::Duration::minutes(6),
        );
        assert!(probe_output_from_snapshot("https://example.com/file.bin", &expired).is_none());
    }

    #[test]
    fn selected_file_paths_only_apply_to_bt_multifile_tasks() {
        let files = vec![
            ProbedFile {
                relative_path: "dir/keep.bin".to_string(),
                size: "100".to_string(),
                content_type: None,
            },
            ProbedFile {
                relative_path: "dir/skip.bin".to_string(),
                size: "200".to_string(),
                content_type: None,
            },
        ];
        let selected = vec!["dir/keep.bin".to_string()];

        let bt_selection = selected_relative_paths_for_probe(Some(&selected), "bt", &files)
            .expect("valid bt selection")
            .expect("selection");
        assert!(bt_selection.contains("dir/keep.bin"));
        assert_eq!(selected_total_size(&files, Some(&bt_selection)), Some(100));

        assert!(
            selected_relative_paths_for_probe(Some(&selected), "https", &files)
                .expect("http ignores selection")
                .is_none()
        );
    }

    #[test]
    fn selected_file_paths_reject_empty_or_unknown_bt_selection() {
        let files = vec![
            ProbedFile {
                relative_path: "known.bin".to_string(),
                size: "100".to_string(),
                content_type: None,
            },
            ProbedFile {
                relative_path: "other.bin".to_string(),
                size: "200".to_string(),
                content_type: None,
            },
        ];

        let empty: Vec<String> = Vec::new();
        assert_eq!(
            selected_relative_paths_for_probe(Some(&empty), "bt", &files).unwrap_err(),
            "Select at least one file to download."
        );

        let unknown = vec!["missing.bin".to_string()];
        assert!(
            selected_relative_paths_for_probe(Some(&unknown), "bt", &files)
                .unwrap_err()
                .contains("Selected file is not in the probe result")
        );
    }

    #[test]
    fn storage_url_for_input_strips_ftp_credentials() {
        assert_eq!(
            storage_url_for_input("ftp://alice:s3cret@example.com/file.bin"),
            "ftp://example.com/file.bin"
        );
        assert_eq!(
            storage_url_for_input("https://example.com/file.bin"),
            "https://example.com/file.bin"
        );
    }

    #[test]
    fn duplicate_source_key_only_uses_bt_info_hashes() {
        let bt = probe_output("bt", "bt:0123456789abcdef");
        let http = probe_output("https", "example.com");
        let ftp = probe_output("ftp", "ftp://example.com:21");

        assert_eq!(
            duplicate_source_key_for_probe(&bt),
            Some("bt:0123456789abcdef")
        );
        assert_eq!(duplicate_source_key_for_probe(&http), None);
        assert_eq!(duplicate_source_key_for_probe(&ftp), None);
    }

    fn probe_snapshot(
        input_url: &str,
        probed_at: chrono::DateTime<chrono::Utc>,
    ) -> ProbeTaskPayload {
        ProbeTaskPayload {
            input_url: input_url.to_string(),
            final_url: "https://cdn.example.com/file.bin".to_string(),
            file_name: "file.bin".to_string(),
            protocol: "https".to_string(),
            task_kind: TaskKind::SingleFile,
            capabilities: EngineCapabilities {
                supports_resume: true,
                supports_parallel: true,
                supports_multi_file: false,
            },
            files: vec![ProbedFile {
                relative_path: "file.bin".to_string(),
                size: "1024".to_string(),
                content_type: Some("application/octet-stream".to_string()),
            }],
            total_size: "1024".to_string(),
            source_key: "example.com".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            etag: Some("\"strong\"".to_string()),
            last_modified: Some("Tue, 02 Jan 2024 00:00:00 GMT".to_string()),
            hls_variants: Vec::new(),
            hls_audio_tracks: Vec::new(),
            hls_subtitle_tracks: Vec::new(),
            probed_at: probed_at.to_rfc3339(),
        }
    }

    fn probe_output(protocol: &str, source_key: &str) -> ProbeOutput {
        ProbeOutput {
            protocol: protocol.to_string(),
            task_kind: TaskKind::SingleFile,
            resolved_uri: "https://example.com/file.bin".to_string(),
            display_name: "file.bin".to_string(),
            total_size: 1024,
            source_key: source_key.to_string(),
            capabilities: EngineCapabilities {
                supports_resume: true,
                supports_parallel: true,
                supports_multi_file: false,
            },
            files: vec![ProbedFile {
                relative_path: "file.bin".to_string(),
                size: "1024".to_string(),
                content_type: Some("application/octet-stream".to_string()),
            }],
            etag: None,
            last_modified: None,
            content_type: Some("application/octet-stream".to_string()),
            hls_variants: Vec::new(),
            hls_audio_tracks: Vec::new(),
            hls_subtitle_tracks: Vec::new(),
            metalink: None,
        }
    }
}
