#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "windows")]
use std::process::Command;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use reqwest::Url;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tauri::{path::BaseDirectory, AppHandle, Manager, State};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{
    db,
    download::EngineRegistry,
    events::{
        emit_browser_handoff_failed, emit_browser_handoff_received,
        emit_browser_integration_changed,
    },
    logging::sanitize_url,
    models::{
        BrowserCaptureSettings, BrowserCaptureSettingsInput, BrowserExtensionExportResult,
        BrowserExtensionPackage, BrowserForwardHeadersMode, BrowserForwardedHeader,
        BrowserHandoffInput, BrowserHandoffResult, BrowserIntegrationEntry,
        BrowserIntegrationStatus, BrowserIntegrationUpdateInput, BrowserKind,
        BrowserRealtimeStatus,
    },
    AppState,
};

use super::tasks::{create_task_with_state_and_headers, CreateTaskInput};

const NATIVE_HOST_NAME: &str = "com.vibe_downloader.native_host";
const CHROME_EXTENSION_ID: &str = env!("VIBE_CHROME_EXTENSION_ID_RESOLVED");
const EDGE_EXTENSION_ID: &str = env!("VIBE_EDGE_EXTENSION_ID_RESOLVED");
const FIREFOX_EXTENSION_ID: &str = env!("VIBE_FIREFOX_EXTENSION_ID_RESOLVED");
const CHROMIUM_PUBLIC_KEY: &str = env!("VIBE_CHROMIUM_PUBLIC_KEY_RESOLVED");
const SETTING_BROWSER_CAPTURE: &str = "browser_capture_settings";
const DEFAULT_BROWSER_MIN_SIZE_BYTES: &str = "0";
const DEFAULT_BROWSER_EXTENSIONS: &[&str] = &[
    "zip", "7z", "rar", "exe", "msi", "dmg", "pkg", "iso", "tar", "gz", "bz2", "xz", "pdf", "mp4",
    "mkv", "mp3", "flac", "m3u8", "meta4", "metalink",
];
pub const FORWARDED_HEADER_ALLOWLIST: &[&str] = &[
    "cookie",
    "user-agent",
    "referer",
    "origin",
    "accept",
    "accept-language",
    "dnt",
    "cache-control",
    "pragma",
];

#[tauri::command]
#[specta::specta]
pub async fn get_browser_integration_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BrowserIntegrationStatus, String> {
    integration_status(&app, state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn install_browser_integration(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BrowserIntegrationUpdateInput,
) -> Result<BrowserIntegrationStatus, String> {
    for browser in input.browsers {
        if !browser_supported_on_platform(browser) {
            continue;
        }
        tracing::info!(browser = %browser.display_name(), "installing browser integration");
        install_manifest(&app, browser)?;
    }
    emit_browser_integration_changed(&app);
    integration_status(&app, state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn uninstall_browser_integration(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BrowserIntegrationUpdateInput,
) -> Result<BrowserIntegrationStatus, String> {
    for browser in input.browsers {
        tracing::info!(browser = %browser.display_name(), "uninstalling browser integration");
        uninstall_manifest(&app, browser)?;
    }
    emit_browser_integration_changed(&app);
    integration_status(&app, state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn export_browser_extension_packages(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BrowserExtensionExportResult, String> {
    export_extension_packages(&app, state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_browser_capture_settings(
    state: State<'_, AppState>,
) -> Result<BrowserCaptureSettings, String> {
    browser_capture_settings(&state.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_browser_capture_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BrowserCaptureSettingsInput,
) -> Result<BrowserCaptureSettings, String> {
    let current = browser_capture_settings(&state.pool).await?;
    let forward_headers_mode = input
        .forward_headers_mode
        .or_else(|| {
            input.forward_headers.map(|enabled| {
                if enabled {
                    BrowserForwardHeadersMode::Enabled
                } else {
                    BrowserForwardHeadersMode::Disabled
                }
            })
        })
        .unwrap_or(current.forward_headers_mode);
    let next = enforce_browser_capture_settings_policy(BrowserCaptureSettings {
        experimental_capture_enabled: input
            .experimental_capture_enabled
            .unwrap_or(current.experimental_capture_enabled),
        auto_intercept: input.auto_intercept.unwrap_or(current.auto_intercept),
        forward_headers: matches!(forward_headers_mode, BrowserForwardHeadersMode::Enabled),
        forward_headers_mode,
        min_size_bytes: input
            .min_size_bytes
            .as_deref()
            .map(normalize_non_negative_i64_string)
            .transpose()?
            .unwrap_or(current.min_size_bytes),
        file_extensions: input
            .file_extensions
            .map(normalize_extensions)
            .unwrap_or(current.file_extensions),
        site_rules: input.site_rules.unwrap_or(current.site_rules),
        allow_intranet_handoff: input
            .allow_intranet_handoff
            .unwrap_or(current.allow_intranet_handoff),
    });
    upsert_browser_capture_settings(&state.pool, &next).await?;
    if matches!(
        next.forward_headers_mode,
        BrowserForwardHeadersMode::Disabled
    ) {
        db::clear_all_task_request_headers(&state.pool).await?;
        state.request_headers.lock().await.clear();
    }
    emit_browser_integration_changed(&app);
    Ok(next)
}

#[tauri::command]
#[specta::specta]
pub async fn create_browser_handoff_task(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BrowserHandoffInput,
) -> Result<BrowserHandoffResult, String> {
    create_browser_handoff_task_with_state(app, state.inner(), input).await
}

pub async fn create_browser_handoff_task_with_state(
    app: AppHandle,
    state: &AppState,
    input: BrowserHandoffInput,
) -> Result<BrowserHandoffResult, String> {
    let request_id = input.request_id.trim().to_string();
    if request_id.is_empty() {
        return Err("Browser handoff request id is required.".to_string());
    }
    tracing::info!(
        request_id = %request_id,
        browser = %input.browser.display_name(),
        url = %sanitize_url(&input.url),
        "browser handoff received"
    );
    if db::browser_message_exists(&state.pool, &request_id).await? {
        tracing::info!(request_id = %request_id, "duplicate browser handoff ignored");
        return Ok(BrowserHandoffResult {
            request_id,
            status: "duplicate".to_string(),
            task: None,
            error_message: None,
        });
    }

    let capture_settings = browser_capture_settings(&state.pool).await?;
    let forwarded_headers = if capture_settings.experimental_capture_enabled {
        sanitize_forwarded_headers(input.forwarded_headers.as_deref())
    } else {
        Vec::new()
    };
    let request_headers = if matches!(
        capture_settings.forward_headers_mode,
        BrowserForwardHeadersMode::Enabled
    ) || input.header_consent_state.as_deref() == Some("allowed")
    {
        forwarded_headers
    } else {
        Vec::new()
    };

    let task_result = validate_handoff(
        &input,
        &state.engine_registry,
        capture_settings.allow_intranet_handoff,
    )
    .await
    .map(|url| CreateTaskInput {
        url,
        save_dir: None,
        file_name: sanitize_suggested_file_name(input.suggested_file_name.as_deref()),
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
    });

    let create_input = match task_result {
        Ok(input) => input,
        Err(error) => {
            tracing::error!(
                request_id = %request_id,
                error = %error,
                "browser handoff validation failed"
            );
            db::insert_browser_message(
                &state.pool,
                &request_id,
                input.browser,
                input.url.trim(),
                "failed",
                Some(&error),
            )
            .await?;
            emit_browser_handoff_failed(&app);
            return Ok(BrowserHandoffResult {
                request_id,
                status: "failed".to_string(),
                task: None,
                error_message: Some(error),
            });
        }
    };

    db::insert_browser_message(
        &state.pool,
        &request_id,
        input.browser,
        &create_input.url,
        "received",
        None,
    )
    .await?;

    match create_task_with_state_and_headers(
        app.clone(),
        state,
        create_input,
        request_headers,
        Some(input.browser),
    )
    .await
    {
        Ok(task) => {
            tracing::info!(
                request_id = %request_id,
                task_id = %task.id,
                "browser handoff task created"
            );
            emit_browser_handoff_received(&app);
            Ok(BrowserHandoffResult {
                request_id,
                status: "received".to_string(),
                task: Some(task),
                error_message: None,
            })
        }
        Err(error) => {
            tracing::error!(
                request_id = %request_id,
                error = %error,
                "browser handoff task creation failed"
            );
            db::update_browser_message_status(&state.pool, &request_id, "failed", Some(&error))
                .await?;
            emit_browser_handoff_failed(&app);
            Ok(BrowserHandoffResult {
                request_id,
                status: "failed".to_string(),
                task: None,
                error_message: Some(error),
            })
        }
    }
}

/// S-2.1: Maximum handoff file size: 1 MiB (audit-recommended value). The JSON payload written by the native host
/// is far smaller than this; 1 MiB is enough for any legitimate handoff while preventing memory pressure from large files.
const HANDOFF_MAX_BYTES: u64 = 1024 * 1024;

/// S-2.1: Maximum handoff file name length (consistent with native host `safe_file_stem` output).
const HANDOFF_FILE_NAME_MAX_LEN: usize = 128;

/// S-2.1: Resolve the handoff directory. Reads the same environment variable as the native host
/// `VIBE_DOWNLOADER_HANDOFF_DIR` first, falling back to `temp_dir/vibe-downloader-handoff` if unset.
/// Tests isolate by setting this environment variable to a temp directory.
fn resolve_handoff_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VIBE_DOWNLOADER_HANDOFF_DIR") {
        return PathBuf::from(dir);
    }
    std::env::temp_dir().join("vibe-downloader-handoff")
}

/// S-2.1: Validate that the handoff file path is inside `handoff_dir`, the file name conforms to
/// `safe_file_stem` rules (alphanumeric + `-` + `_`, non-empty, `.json` extension),
/// and the size does not exceed `HANDOFF_MAX_BYTES`.
///
/// Returns the canonicalized path so callers can re-validate before deletion (TOCTOU protection).
pub fn validate_handoff_file_path(path: &Path) -> Result<PathBuf, String> {
    let handoff_dir = resolve_handoff_dir();
    let handoff_dir_canon = handoff_dir
        .canonicalize()
        .map_err(|e| format!("handoff dir canonicalize failed: {e}"))?;

    let path_canon = path
        .canonicalize()
        .map_err(|e| format!("handoff file canonicalize failed: {e}"))?;

    if !path_canon.starts_with(&handoff_dir_canon) {
        return Err(format!(
            "handoff file path must be inside {}, got {}",
            handoff_dir_canon.display(),
            path_canon.display()
        ));
    }

    let file_name = path_canon
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "handoff file name is not valid UTF-8".to_string())?;
    if !file_name.ends_with(".json") {
        return Err(format!(
            "handoff file name must end with '.json', got '{}'",
            file_name
        ));
    }
    let stem = &file_name[..file_name.len() - ".json".len()];
    if stem.is_empty() || stem.len() > HANDOFF_FILE_NAME_MAX_LEN {
        return Err(format!(
            "handoff file name stem is empty or exceeds {} chars, got '{}'",
            HANDOFF_FILE_NAME_MAX_LEN, stem
        ));
    }
    // Consistent with native host `safe_file_stem`: only alphanumeric + `-` + `_` allowed.
    if !stem
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!(
            "handoff file name stem contains invalid characters, got '{}'",
            stem
        ));
    }

    let metadata =
        fs::metadata(&path_canon).map_err(|e| format!("handoff file metadata failed: {e}"))?;
    let size = metadata.len();
    if size > HANDOFF_MAX_BYTES {
        return Err(format!(
            "handoff file size {} bytes exceeds max {} bytes",
            size, HANDOFF_MAX_BYTES
        ));
    }

    Ok(path_canon)
}

pub fn read_handoff_file(path: &Path) -> Result<BrowserHandoffInput, String> {
    let canon = validate_handoff_file_path(path)?;
    let raw = fs::read_to_string(&canon)
        .map_err(|e| format!("Could not read browser handoff file: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid browser handoff payload: {e}"))
}

pub async fn browser_capture_settings(
    pool: &sqlx::SqlitePool,
) -> Result<BrowserCaptureSettings, String> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(SETTING_BROWSER_CAPTURE)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(raw) = row.map(|row| row.get::<String, _>("value")) else {
        return Ok(default_browser_capture_settings());
    };
    let mut parsed = parse_browser_capture_settings(&raw);
    parsed.min_size_bytes = normalize_non_negative_i64_string(&parsed.min_size_bytes)?;
    parsed.file_extensions = normalize_extensions(parsed.file_extensions);
    Ok(enforce_browser_capture_settings_policy(parsed))
}

pub async fn upsert_browser_capture_settings(
    pool: &sqlx::SqlitePool,
    settings: &BrowserCaptureSettings,
) -> Result<(), String> {
    let settings = enforce_browser_capture_settings_policy(settings.clone());
    let raw = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        INSERT INTO settings (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(SETTING_BROWSER_CAPTURE)
    .bind(raw)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn default_browser_capture_settings() -> BrowserCaptureSettings {
    BrowserCaptureSettings {
        experimental_capture_enabled: false,
        auto_intercept: false,
        forward_headers: false,
        forward_headers_mode: BrowserForwardHeadersMode::Disabled,
        min_size_bytes: DEFAULT_BROWSER_MIN_SIZE_BYTES.to_string(),
        file_extensions: DEFAULT_BROWSER_EXTENSIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        site_rules: Vec::new(),
        allow_intranet_handoff: false,
    }
}

pub fn enforce_browser_capture_settings_policy(
    mut settings: BrowserCaptureSettings,
) -> BrowserCaptureSettings {
    if !capture_available() {
        settings.experimental_capture_enabled = false;
        settings.auto_intercept = false;
        settings.forward_headers = false;
        settings.forward_headers_mode = BrowserForwardHeadersMode::Disabled;
    }
    settings
}

/// S-1.1: Sensitive fields that the WS browser realtime bridge must not allow modifying via `updateSettings`.
///
/// These fields affect the browser capture security boundary (Cookie/header forwarding, intranet handoff,
/// experimental capture) and must be explicitly operated by the user in the main window UI (via Tauri commands),
/// not directly by a local process holding the WS bootstrap token.
///
/// Field names are the JSON (camelCase) serialization names of `BrowserCaptureSettingsInput`,
/// matching the payload keys sent by the browser extension.
pub const SENSITIVE_BROWSER_SETTINGS: &[&str] = &[
    "forwardHeaders",             // Cookie/header forwarding toggle
    "forwardHeadersMode",         // Forwarding mode (enabled/disabled)
    "experimentalCaptureEnabled", // Experimental capture
    "allowIntranetHandoff",       // Intranet handoff (highest risk)
];

/// S-1.1: Check whether the `updateSettings` payload contains sensitive fields.
/// Returns the list of conflicting fields (empty means no conflict). Callers should invoke this function
/// before deserializing into `BrowserCaptureSettingsInput`, so that if the payload
/// contains sensitive fields it is rejected immediately without entering the merge/upsert flow.
pub fn is_sensitive_settings_update(
    payload: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    payload
        .keys()
        .filter(|k| SENSITIVE_BROWSER_SETTINGS.contains(&k.as_str()))
        .cloned()
        .collect()
}

fn parse_browser_capture_settings(raw: &str) -> BrowserCaptureSettings {
    let mut settings = default_browser_capture_settings();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return settings;
    };
    settings.experimental_capture_enabled = value
        .get("experimentalCaptureEnabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(settings.experimental_capture_enabled);
    settings.auto_intercept = value
        .get("autoIntercept")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(settings.auto_intercept);
    settings.forward_headers_mode = value
        .get("forwardHeadersMode")
        .and_then(serde_json::Value::as_str)
        .and_then(|mode| match mode {
            "enabled" => Some(BrowserForwardHeadersMode::Enabled),
            "disabled" => Some(BrowserForwardHeadersMode::Disabled),
            "ask" => Some(BrowserForwardHeadersMode::Ask),
            _ => None,
        })
        .or_else(|| {
            value
                .get("forwardHeaders")
                .and_then(serde_json::Value::as_bool)
                .map(|enabled| {
                    if enabled {
                        BrowserForwardHeadersMode::Enabled
                    } else {
                        BrowserForwardHeadersMode::Disabled
                    }
                })
        })
        .unwrap_or(BrowserForwardHeadersMode::Ask);
    settings.forward_headers = matches!(
        settings.forward_headers_mode,
        BrowserForwardHeadersMode::Enabled
    );
    settings.min_size_bytes = value
        .get("minSizeBytes")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&settings.min_size_bytes)
        .to_string();
    if let Some(file_extensions) = value.get("fileExtensions").cloned() {
        if let Ok(values) = serde_json::from_value::<Vec<String>>(file_extensions) {
            settings.file_extensions = values;
        }
    }
    if let Some(site_rules) = value.get("siteRules").cloned() {
        if let Ok(values) = serde_json::from_value(site_rules) {
            settings.site_rules = values;
        }
    }
    settings.allow_intranet_handoff = value
        .get("allowIntranetHandoff")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(settings.allow_intranet_handoff);
    settings
}

fn normalize_non_negative_i64_string(value: &str) -> Result<String, String> {
    let parsed = value
        .trim()
        .parse::<i64>()
        .map_err(|_| "Browser rule size must be a number.".to_string())?;
    Ok(parsed.max(0).to_string())
}

fn normalize_extensions(values: Vec<String>) -> Vec<String> {
    let mut out = values
        .into_iter()
        .map(|value| value.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|value| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        })
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

async fn export_extension_packages(
    app: &AppHandle,
    state: &AppState,
) -> Result<BrowserExtensionExportResult, String> {
    let extension_root = extension_core_path_for_app(app)
        .ok_or_else(|| "Browser extension source folder could not be found.".to_string())?;
    let source_dir = extension_root.join("src");
    let template_path = extension_root.join("manifest.template.json");
    if !source_dir.exists() || !template_path.exists() {
        return Err("Browser extension source files are incomplete.".to_string());
    }

    let default_dir = super::settings::default_download_dir(app)?;
    let settings = db::get_settings(&state.pool, default_dir).await?;
    let capture_settings = browser_capture_settings(&state.pool).await?;
    let experimental = capture_available() && capture_settings.experimental_capture_enabled;
    let output_dir = PathBuf::from(settings.default_save_dir)
        .join("Vibe Downloader Extensions")
        .join(format!("v{}", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Could not create extension export folder: {e}"))?;

    let manifest_template = fs::read_to_string(&template_path)
        .map_err(|e| format!("Could not read extension manifest template: {e}"))?;
    let background_template = fs::read_to_string(source_dir.join("background.js"))
        .map_err(|e| format!("Could not read extension background script: {e}"))?;
    let shared_files = [
        "logger.js",
        "popup.html",
        "popup.js",
        "popup.css",
        "options.html",
        "options.js",
        "options.css",
    ];
    for file in shared_files {
        if !source_dir.join(file).exists() {
            return Err(format!("Browser extension source file is missing: {file}"));
        }
    }

    let mut packages = Vec::new();
    for variant in extension_package_variants() {
        let package_path = output_dir.join(format!(
            "vibe-downloader-{}-v{}.{}",
            variant.id,
            env!("CARGO_PKG_VERSION"),
            variant.extension
        ));
        if package_path.exists() {
            fs::remove_file(&package_path)
                .map_err(|e| format!("Could not replace existing extension package: {e}"))?;
        }

        let manifest = extension_manifest(&manifest_template, &variant, experimental)?;
        let background =
            extension_background(&background_template, variant.browser_kind, experimental);
        write_extension_package(
            &package_path,
            &source_dir,
            &manifest,
            &background,
            &shared_files,
        )?;
        let sha256 = file_sha256(&package_path)?;
        packages.push(BrowserExtensionPackage {
            target: variant.target.to_string(),
            package_path: package_path.to_string_lossy().to_string(),
            sha256,
            install_note: variant.install_note.to_string(),
        });
    }

    let install_guide_path = output_dir.join("INSTALL.md");
    fs::write(&install_guide_path, install_guide(&packages))
        .map_err(|e| format!("Could not write extension install guide: {e}"))?;
    let sums_path = output_dir.join("SHA256SUMS.txt");
    fs::write(&sums_path, sha256_sums(&packages))
        .map_err(|e| format!("Could not write extension checksums: {e}"))?;

    tracing::info!(
        output_dir = %output_dir.display(),
        package_count = packages.len(),
        "browser extension packages exported"
    );

    Ok(BrowserExtensionExportResult {
        output_dir: output_dir.to_string_lossy().to_string(),
        install_guide_path: install_guide_path.to_string_lossy().to_string(),
        packages,
    })
}

async fn integration_status(
    app: &AppHandle,
    state: &AppState,
) -> Result<BrowserIntegrationStatus, String> {
    let (native_host_path, native_host_ready, native_host_error) = match native_host_path() {
        Ok(path) => (Some(path.to_string_lossy().to_string()), true, None),
        Err(error) => (None, false, Some(error)),
    };
    let extension_core_path =
        extension_core_path_for_app(app).map(|path| path.to_string_lossy().to_string());
    let mut browsers = Vec::new();

    for browser in BrowserKind::all() {
        let manifest_path = manifest_path(app, browser).ok();
        let manifest_installed = manifest_path.as_ref().is_some_and(|path| path.exists());
        let last_error = db::latest_browser_error(&state.pool, browser).await?;
        browsers.push(BrowserIntegrationEntry {
            browser,
            display_name: browser.display_name().to_string(),
            supported_on_platform: browser_supported_on_platform(browser),
            detected: browser_detected(app, browser),
            manifest_installed,
            manifest_path: manifest_path.map(|path| path.to_string_lossy().to_string()),
            extension_load_path: extension_core_path.clone(),
            extension_id: extension_id(browser).map(str::to_string),
            profile: integration_profile().to_string(),
            last_error,
        });
    }

    let realtime = state.browser_realtime.status().await;
    let capture = browser_capture_settings(&state.pool).await?;
    Ok(BrowserIntegrationStatus {
        native_host_name: NATIVE_HOST_NAME.to_string(),
        native_host_path,
        native_host_ready,
        native_host_error,
        extension_core_path,
        capture_available: capture_available(),
        experimental_capture_enabled: capture.experimental_capture_enabled,
        realtime: BrowserRealtimeStatus {
            ws_url: realtime.ws_url,
            connected: realtime.connected,
        },
        capture,
        browsers,
    })
}

async fn validate_handoff(
    input: &BrowserHandoffInput,
    registry: &EngineRegistry,
    allow_intranet: bool,
) -> Result<String, String> {
    if input.version != 1 {
        return Err("Unsupported browser handoff payload version.".to_string());
    }
    if input.action != "download_url" {
        return Err("Unsupported browser handoff action.".to_string());
    }

    let url = input.url.trim();
    let parsed = Url::parse(url).map_err(|_| "Browser handoff URL is invalid.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Browser handoff only supports HTTP and HTTPS URLs.".to_string());
    }
    if !allow_intranet && is_private_or_reserved_url(&parsed) {
        return Err(
            "Browser handoff URL points to a private or reserved address. \
             Enable \"Allow intranet handoff\" in settings to override."
                .to_string(),
        );
    }
    // A-2: DNS rebinding defense — if the host is a hostname (not a literal
    // IP), resolve it and reject if any resolved IP is private/reserved.
    // The literal-IP check above only catches IP addresses embedded in the
    // URL; a public hostname that rebinds to 127.0.0.1 / 169.254.169.254
    // would bypass the guard without this pre-flight lookup.
    if !allow_intranet {
        if let Some(host) = parsed.host_str() {
            let host = host
                .strip_prefix('[')
                .and_then(|h| h.strip_suffix(']'))
                .unwrap_or(host);
            // Only do DNS lookup for non-IP hostnames (literal IPs are
            // already handled by is_private_or_reserved_url above).
            if host.parse::<std::net::IpAddr>().is_err()
                && crate::download::ssrf::is_hostname_private_via_dns(host).await
            {
                return Err(
                    "Browser handoff URL resolves to a private or reserved IP address. \
                     Enable \"Allow intranet handoff\" in settings to override."
                        .to_string(),
                );
            }
        }
    }
    registry.engine_for_uri(parsed.as_str())?;
    // Reject embedded credentials at the handoff boundary: the handoff JSON is written to
    // a temp dir and logged, so credentials would leak. Direct UI/clipboard creation is
    // allowed to extract them because it encrypts and sanitizes — this asymmetry is
    // intentional, not a gap.
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("Browser handoff URLs must not contain embedded credentials.".to_string());
    }
    Ok(parsed.to_string())
}

pub fn is_private_or_reserved_url(url: &Url) -> bool {
    crate::download::ssrf::is_private_or_reserved_url(url)
}

pub fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    crate::download::ssrf::is_private_ip(ip)
}

fn sanitize_forwarded_headers(headers: Option<&[BrowserForwardedHeader]>) -> Vec<(String, String)> {
    let Some(headers) = headers else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for header in headers {
        let name = header.name.trim().to_ascii_lowercase();
        if !FORWARDED_HEADER_ALLOWLIST.contains(&name.as_str()) {
            continue;
        }
        // Block headers that could break download semantics (range, accept-encoding), enable
        // spoofing (host), leak credentials (authorization, set-cookie, proxy-authorization),
        // or inject HTTP framing (values with CR/LF). sec-* are browser-internal and meaningless
        // to the download engine.
        if name.starts_with("sec-")
            || matches!(
                name.as_str(),
                "authorization"
                    | "proxy-authorization"
                    | "set-cookie"
                    | "range"
                    | "accept-encoding"
                    | "host"
                    | "connection"
            )
        {
            continue;
        }
        let value = header.value.trim();
        if value.is_empty() || value.contains('\n') || value.contains('\r') {
            continue;
        }
        out.push((name, value.to_string()));
    }
    out
}

struct ExtensionPackageVariant {
    id: &'static str,
    target: &'static str,
    browser_kind: &'static str,
    extension: &'static str,
    firefox_id: Option<&'static str>,
    install_note: &'static str,
}

fn extension_package_variants() -> Vec<ExtensionPackageVariant> {
    let mut variants = vec![
        ExtensionPackageVariant {
            id: "chromium",
            target: "Chrome, Brave, Vivaldi, Chromium",
            browser_kind: "chrome",
            extension: "zip",
            firefox_id: None,
            install_note: "Load the unpacked or zipped Chromium package from the browser extensions page.",
        },
        ExtensionPackageVariant {
            id: "edge",
            target: "Microsoft Edge",
            browser_kind: "edge",
            extension: "zip",
            firefox_id: None,
            install_note: "Load the package from edge://extensions after enabling developer mode.",
        },
        ExtensionPackageVariant {
            id: "firefox",
            target: "Mozilla Firefox",
            browser_kind: "firefox",
            extension: "xpi",
            firefox_id: Some(FIREFOX_EXTENSION_ID),
            install_note: "Firefox release builds require a signed XPI. Use this local package for development profiles.",
        },
    ];
    if integration_profile() == "dev" {
        variants.push(ExtensionPackageVariant {
            id: "opera",
            target: "Opera",
            browser_kind: "opera",
            extension: "zip",
            firefox_id: None,
            install_note:
                "Load the package from Opera's extensions page after enabling developer mode.",
        });
    }
    variants
}

fn extension_manifest(
    template: &str,
    variant: &ExtensionPackageVariant,
    experimental: bool,
) -> Result<String, String> {
    let mut manifest: serde_json::Value = serde_json::from_str(template)
        .map_err(|e| format!("Invalid extension manifest template: {e}"))?;
    manifest["version"] = serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string());
    apply_extension_capture_permissions(&mut manifest, experimental);
    if variant.id == "firefox" {
        manifest["name"] = serde_json::Value::String("Vibe Downloader (Firefox)".to_string());
    } else if !CHROMIUM_PUBLIC_KEY.is_empty() {
        manifest["key"] = serde_json::Value::String(CHROMIUM_PUBLIC_KEY.to_string());
    } else if let Some(object) = manifest.as_object_mut() {
        object.remove("key");
    }
    if let Some(firefox_id) = variant.firefox_id {
        manifest["browser_specific_settings"] = serde_json::json!({
            "gecko": {
                "id": firefox_id,
                "strict_min_version": "109.0"
            }
        });
    }
    serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())
}

fn extension_background(template: &str, browser_kind: &str, experimental: bool) -> String {
    template
        .replace("__VIBE_BROWSER_KIND__", browser_kind)
        .replace(
            "__VIBE_EXPERIMENTAL_CAPTURE__",
            if experimental { "true" } else { "false" },
        )
}

fn apply_extension_capture_permissions(manifest: &mut serde_json::Value, experimental: bool) {
    if experimental {
        let permissions = manifest
            .get_mut("permissions")
            .and_then(serde_json::Value::as_array_mut)
            .expect("extension manifest permissions must be an array");
        for permission in ["downloads", "cookies", "webRequest"] {
            if !permissions
                .iter()
                .any(|value| value.as_str() == Some(permission))
            {
                permissions.push(serde_json::Value::String(permission.to_string()));
            }
        }
        manifest["host_permissions"] = serde_json::json!(["http://*/*", "https://*/*"]);
    } else if let Some(object) = manifest.as_object_mut() {
        object.remove("host_permissions");
        if let Some(permissions) = object
            .get_mut("permissions")
            .and_then(serde_json::Value::as_array_mut)
        {
            permissions.retain(|value| {
                !matches!(value.as_str(), Some("downloads" | "cookies" | "webRequest"))
            });
        }
    }
}

fn write_extension_package(
    package_path: &Path,
    source_dir: &Path,
    manifest: &str,
    background: &str,
    shared_files: &[&str],
) -> Result<(), String> {
    let file = File::create(package_path)
        .map_err(|e| format!("Could not create extension package: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("manifest.json", options)
        .map_err(|e| format!("Could not write extension package manifest: {e}"))?;
    zip.write_all(format!("{manifest}\n").as_bytes())
        .map_err(|e| format!("Could not write extension package manifest: {e}"))?;
    zip.start_file("background.js", options)
        .map_err(|e| format!("Could not write extension background script: {e}"))?;
    zip.write_all(background.as_bytes())
        .map_err(|e| format!("Could not write extension background script: {e}"))?;

    for file_name in shared_files {
        zip.start_file(*file_name, options)
            .map_err(|e| format!("Could not add extension file {file_name}: {e}"))?;
        let mut file = File::open(source_dir.join(file_name))
            .map_err(|e| format!("Could not read extension file {file_name}: {e}"))?;
        std::io::copy(&mut file, &mut zip)
            .map_err(|e| format!("Could not write extension file {file_name}: {e}"))?;
    }
    write_extension_directory(
        &mut zip,
        &source_dir.join("_locales"),
        Path::new("_locales"),
        options,
    )?;

    zip.finish()
        .map_err(|e| format!("Could not finalize extension package: {e}"))?;
    Ok(())
}

fn write_extension_directory(
    zip: &mut ZipWriter<File>,
    source: &Path,
    archive_prefix: &Path,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let mut entries = fs::read_dir(source)
        .map_err(|e| {
            format!(
                "Could not read extension directory {}: {e}",
                source.display()
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            format!(
                "Could not enumerate extension directory {}: {e}",
                source.display()
            )
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry.file_type().map_err(|e| {
            format!(
                "Could not inspect extension file {}: {e}",
                entry.path().display()
            )
        })?;
        let archive_path = archive_prefix.join(entry.file_name());
        if file_type.is_dir() {
            write_extension_directory(zip, &entry.path(), &archive_path, options)?;
        } else if file_type.is_file() {
            let archive_name = archive_path.to_string_lossy().replace('\\', "/");
            zip.start_file(&archive_name, options)
                .map_err(|e| format!("Could not add extension file {archive_name}: {e}"))?;
            let mut file = File::open(entry.path())
                .map_err(|e| format!("Could not read extension file {archive_name}: {e}"))?;
            std::io::copy(&mut file, zip)
                .map_err(|e| format!("Could not write extension file {archive_name}: {e}"))?;
        }
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("Could not read extension package: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Could not hash extension package: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn install_guide(packages: &[BrowserExtensionPackage]) -> String {
    let mut lines = vec![
        "# Vibe Downloader Browser Extensions".to_string(),
        String::new(),
        format!(
            "Generated by Vibe Downloader v{}.",
            env!("CARGO_PKG_VERSION")
        ),
        String::new(),
        "Install the Native Messaging host from Vibe Settings before loading these packages."
            .to_string(),
        "Local browser packages still require browser-side confirmation or developer mode."
            .to_string(),
        String::new(),
        "## Packages".to_string(),
    ];
    for package in packages {
        let file_name = Path::new(&package.package_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&package.package_path);
        lines.push(format!("- {}: `{}`", package.target, file_name));
        lines.push(format!("  - SHA-256: `{}`", package.sha256));
        lines.push(format!("  - Note: {}", package.install_note));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn sha256_sums(packages: &[BrowserExtensionPackage]) -> String {
    packages
        .iter()
        .map(|package| {
            let file_name = Path::new(&package.package_path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&package.package_path);
            format!("{}  {}", package.sha256, file_name)
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn sanitize_suggested_file_name(value: Option<&str>) -> Option<String> {
    let cleaned: String = value?
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn install_manifest(app: &AppHandle, browser: BrowserKind) -> Result<(), String> {
    let native_host_path = native_host_path()?;
    let manifest = manifest_json(browser, &native_host_path)?;
    let manifest_path = manifest_path(app, browser)?;
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create native host manifest folder: {e}"))?;
    }
    fs::write(&manifest_path, manifest)
        .map_err(|e| format!("Could not write native host manifest: {e}"))?;

    #[cfg(target_os = "windows")]
    if let Err(error) = install_windows_registry(browser, &manifest_path) {
        let _ = fs::remove_file(&manifest_path);
        return Err(error);
    }

    Ok(())
}

fn uninstall_manifest(app: &AppHandle, browser: BrowserKind) -> Result<(), String> {
    if let Ok(path) = manifest_path(app, browser) {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Could not remove native host manifest: {e}"))?;
        }
    }

    #[cfg(target_os = "windows")]
    uninstall_windows_registry(browser)?;

    Ok(())
}

fn manifest_json(browser: BrowserKind, native_host_path: &Path) -> Result<String, String> {
    let path = native_host_path.to_string_lossy().to_string();
    let extension_id = extension_id(browser).ok_or_else(|| {
        format!(
            "{} is not supported by this browser integration profile.",
            browser.display_name()
        )
    })?;
    let value = if matches!(browser, BrowserKind::Firefox) {
        serde_json::json!({
            "name": NATIVE_HOST_NAME,
            "description": "Vibe Downloader browser handoff host",
            "path": path,
            "type": "stdio",
            "allowed_extensions": [extension_id]
        })
    } else {
        serde_json::json!({
            "name": NATIVE_HOST_NAME,
            "description": "Vibe Downloader browser handoff host",
            "path": path,
            "type": "stdio",
            "allowed_origins": [format!("chrome-extension://{extension_id}/")]
        })
    };
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

fn integration_profile() -> &'static str {
    env!("VIBE_BROWSER_PROFILE_RESOLVED")
}

fn capture_available() -> bool {
    env!("VIBE_BROWSER_CAPTURE_AVAILABLE") == "true"
}

fn extension_id(browser: BrowserKind) -> Option<&'static str> {
    match browser {
        BrowserKind::Firefox => Some(FIREFOX_EXTENSION_ID),
        BrowserKind::Edge => Some(EDGE_EXTENSION_ID),
        BrowserKind::Safari => None,
        BrowserKind::Opera if integration_profile() != "dev" => None,
        _ => Some(CHROME_EXTENSION_ID),
    }
}

fn native_host_path() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|e| format!("Could not resolve the Vibe Downloader executable: {e}"))?;
    let exe_name = if cfg!(target_os = "windows") {
        "vibe-native-host.exe"
    } else {
        "vibe-native-host"
    };
    validate_native_host_path(&current.with_file_name(exe_name))
}

fn validate_native_host_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = path.canonicalize().map_err(|e| {
        format!(
            "Native Messaging host was not found at {}: {e}",
            path.display()
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|e| {
        format!(
            "Could not inspect Native Messaging host {}: {e}",
            canonical.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "Native Messaging host is not a file: {}",
            canonical.display()
        ));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "Native Messaging host is not executable: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn extension_core_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .join("browser")
            .join("extension-core"),
    )
}

fn extension_core_path_for_app(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve("browser/extension-core", BaseDirectory::Resource)
        .ok()
        .filter(|path| path.exists())
        .or_else(|| extension_core_path().filter(|path| path.exists()))
}

fn manifest_path(app: &AppHandle, browser: BrowserKind) -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        app.path()
            .app_config_dir()
            .map(|path| {
                path.join("native-messaging")
                    .join(browser.as_str())
                    .join(format!("{NATIVE_HOST_NAME}.json"))
            })
            .map_err(|e| e.to_string())
    }

    #[cfg(target_os = "macos")]
    {
        let home = app.path().home_dir().map_err(|e| e.to_string())?;
        let base = match browser {
            BrowserKind::Chrome => "Library/Application Support/Google/Chrome/NativeMessagingHosts",
            BrowserKind::Edge => "Library/Application Support/Microsoft Edge/NativeMessagingHosts",
            BrowserKind::Firefox => "Library/Application Support/Mozilla/NativeMessagingHosts",
            BrowserKind::Safari => "Library/Application Support/Vibe Downloader/Safari",
            BrowserKind::Brave => {
                "Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"
            }
            BrowserKind::Opera => {
                "Library/Application Support/com.operasoftware.Opera/NativeMessagingHosts"
            }
            BrowserKind::Vivaldi => "Library/Application Support/Vivaldi/NativeMessagingHosts",
            BrowserKind::Chromium => "Library/Application Support/Chromium/NativeMessagingHosts",
        };
        Ok(home.join(base).join(format!("{NATIVE_HOST_NAME}.json")))
    }

    #[cfg(target_os = "linux")]
    {
        let home = app.path().home_dir().map_err(|e| e.to_string())?;
        let base = match browser {
            BrowserKind::Chrome => ".config/google-chrome/NativeMessagingHosts",
            BrowserKind::Edge => ".config/microsoft-edge/NativeMessagingHosts",
            BrowserKind::Firefox => ".mozilla/native-messaging-hosts",
            BrowserKind::Safari => ".config/vibe-downloader/safari-unsupported",
            BrowserKind::Brave => ".config/BraveSoftware/Brave-Browser/NativeMessagingHosts",
            BrowserKind::Opera => ".config/opera/NativeMessagingHosts",
            BrowserKind::Vivaldi => ".config/vivaldi/NativeMessagingHosts",
            BrowserKind::Chromium => ".config/chromium/NativeMessagingHosts",
        };
        Ok(home.join(base).join(format!("{NATIVE_HOST_NAME}.json")))
    }
}

fn browser_supported_on_platform(browser: BrowserKind) -> bool {
    !(matches!(browser, BrowserKind::Safari)
        || matches!(browser, BrowserKind::Opera) && integration_profile() != "dev")
}

fn browser_detected(app: &AppHandle, browser: BrowserKind) -> bool {
    if !browser_supported_on_platform(browser) {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = app;
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let program_files = std::env::var("ProgramFiles").unwrap_or_default();
        let candidates = match browser {
            BrowserKind::Chrome => {
                vec![format!("{local}\\Google\\Chrome\\Application\\chrome.exe")]
            }
            BrowserKind::Edge => vec![format!(
                "{program_files}\\Microsoft\\Edge\\Application\\msedge.exe"
            )],
            BrowserKind::Firefox => vec![format!("{program_files}\\Mozilla Firefox\\firefox.exe")],
            BrowserKind::Brave => vec![format!(
                "{local}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"
            )],
            BrowserKind::Opera => vec![format!("{local}\\Programs\\Opera\\opera.exe")],
            BrowserKind::Vivaldi => vec![format!("{local}\\Vivaldi\\Application\\vivaldi.exe")],
            BrowserKind::Chromium => vec![format!("{local}\\Chromium\\Application\\chrome.exe")],
            BrowserKind::Safari => vec![],
        };
        candidates.iter().any(|path| Path::new(path).exists())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = app.path().home_dir().ok();
        let marker = match browser {
            BrowserKind::Safari => "Applications/Safari.app",
            BrowserKind::Chrome => ".config/google-chrome",
            BrowserKind::Edge => ".config/microsoft-edge",
            BrowserKind::Firefox => ".mozilla",
            BrowserKind::Brave => ".config/BraveSoftware/Brave-Browser",
            BrowserKind::Opera => ".config/opera",
            BrowserKind::Vivaldi => ".config/vivaldi",
            BrowserKind::Chromium => ".config/chromium",
        };
        home.is_some_and(|home| home.join(marker).exists())
    }
}

#[cfg(target_os = "windows")]
fn install_windows_registry(browser: BrowserKind, manifest_path: &Path) -> Result<(), String> {
    let key = windows_registry_key(browser);
    let status = Command::new("reg")
        .args([
            "add",
            &key,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            &manifest_path.to_string_lossy(),
            "/f",
        ])
        .status()
        .map_err(|e| format!("Could not run reg.exe: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not write native messaging registry key.".to_string())
    }
}

#[cfg(target_os = "windows")]
fn uninstall_windows_registry(browser: BrowserKind) -> Result<(), String> {
    let key = windows_registry_key(browser);
    let status = Command::new("reg")
        .args(["delete", &key, "/f"])
        .status()
        .map_err(|e| format!("Could not run reg.exe: {e}"))?;
    let _ = status;
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_registry_key(browser: BrowserKind) -> String {
    let vendor = match browser {
        BrowserKind::Chrome => "Google\\Chrome",
        BrowserKind::Edge => "Microsoft\\Edge",
        BrowserKind::Firefox => "Mozilla",
        BrowserKind::Brave => "BraveSoftware\\Brave-Browser",
        BrowserKind::Opera => "Opera Software",
        BrowserKind::Vivaldi => "Vivaldi",
        BrowserKind::Chromium => "Chromium",
        BrowserKind::Safari => "Apple\\Safari",
    };
    format!("HKCU\\Software\\{vendor}\\NativeMessagingHosts\\{NATIVE_HOST_NAME}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_fixture_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "vibe-browser-command-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create fixture directory");
        path
    }

    #[test]
    fn native_host_validation_rejects_missing_files() {
        let path = temp_fixture_dir().join("missing-native-host");
        let error = validate_native_host_path(&path).expect_err("missing host must fail");
        assert!(error.contains("was not found"), "unexpected error: {error}");
        let _ = fs::remove_dir_all(path.parent().expect("fixture parent"));
    }

    #[test]
    fn native_host_validation_returns_a_canonical_file() {
        let dir = temp_fixture_dir();
        let path = dir.join(if cfg!(target_os = "windows") {
            "vibe-native-host.exe"
        } else {
            "vibe-native-host"
        });
        fs::write(&path, b"native-host").expect("write fixture");
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("set executable bit");
        assert_eq!(
            validate_native_host_path(&path).expect("valid host"),
            path.canonicalize().expect("canonical fixture")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn native_host_validation_rejects_non_executable_unix_files() {
        let dir = temp_fixture_dir();
        let path = dir.join("vibe-native-host");
        fs::write(&path, b"native-host").expect("write fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("clear executable bit");
        let error = validate_native_host_path(&path).expect_err("non-executable host must fail");
        assert!(
            error.contains("not executable"),
            "unexpected error: {error}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn manifest_json_uses_the_verified_path_and_compiled_identity() {
        let path = if cfg!(target_os = "windows") {
            PathBuf::from(r"C:\Program Files\Vibe Downloader\vibe-native-host.exe")
        } else {
            PathBuf::from("/opt/vibe-downloader/vibe-native-host")
        };
        let raw = manifest_json(BrowserKind::Chrome, &path).expect("manifest json");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parse manifest");
        let expected_origin = format!("chrome-extension://{CHROME_EXTENSION_ID}/");
        assert_eq!(value["path"].as_str(), path.to_str());
        assert_eq!(
            value["allowed_origins"][0].as_str(),
            Some(expected_origin.as_str())
        );
    }

    #[test]
    fn minimal_profiles_force_capture_settings_off() {
        if capture_available() {
            return;
        }
        let mut settings = default_browser_capture_settings();
        settings.experimental_capture_enabled = true;
        settings.auto_intercept = true;
        settings.forward_headers = true;
        settings.forward_headers_mode = BrowserForwardHeadersMode::Enabled;
        let enforced = enforce_browser_capture_settings_policy(settings);
        assert!(!enforced.experimental_capture_enabled);
        assert!(!enforced.auto_intercept);
        assert!(!enforced.forward_headers);
        assert!(matches!(
            enforced.forward_headers_mode,
            BrowserForwardHeadersMode::Disabled
        ));
    }
}
