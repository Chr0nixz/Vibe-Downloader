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
const CHROMIUM_DEV_EXTENSION_ID: &str = "abcdefghijklmnopabcdefghijklmnop";
const CHROMIUM_RELEASE_EXTENSION_ID: &str = "replace-with-chrome-web-store-id";
const EDGE_RELEASE_EXTENSION_ID: &str = "replace-with-edge-addons-id";
const FIREFOX_DEV_EXTENSION_ID: &str = "vibe-downloader@local";
const FIREFOX_RELEASE_EXTENSION_ID: &str = "vibe-downloader@example.invalid";
const SETTING_BROWSER_CAPTURE: &str = "browser_capture_settings";
const DEFAULT_BROWSER_MIN_SIZE_BYTES: &str = "0";
const DEFAULT_BROWSER_EXTENSIONS: &[&str] = &[
    "zip", "7z", "rar", "exe", "msi", "dmg", "pkg", "iso", "tar", "gz", "bz2", "xz", "pdf", "mp4",
    "mkv", "mp3", "flac", "m3u8",
];
const FORWARDED_HEADER_ALLOWLIST: &[&str] = &[
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

fn browser_experimental_capture_enabled() -> bool {
    std::env::var("VIBE_BROWSER_EXPERIMENTAL_CAPTURE")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

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
    let forwarded_headers = if browser_experimental_capture_enabled() {
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

    let task_result = validate_handoff(&input, &state.engine_registry).map(|url| CreateTaskInput {
        url,
        save_dir: None,
        file_name: sanitize_suggested_file_name(input.suggested_file_name.as_deref()),
        expected_hash_sha256: None,
        task_speed_limit_bps: None,
        priority: None,
        category_key: None,
        probe_snapshot: None,
        selected_file_paths: None,
        allow_duplicate: Some(false),
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

pub fn read_handoff_file(path: &Path) -> Result<BrowserHandoffInput, String> {
    let raw = fs::read_to_string(path)
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
    let experimental = browser_experimental_capture_enabled();
    BrowserCaptureSettings {
        auto_intercept: experimental,
        forward_headers: false,
        forward_headers_mode: if experimental {
            BrowserForwardHeadersMode::Ask
        } else {
            BrowserForwardHeadersMode::Disabled
        },
        min_size_bytes: DEFAULT_BROWSER_MIN_SIZE_BYTES.to_string(),
        file_extensions: DEFAULT_BROWSER_EXTENSIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        site_rules: Vec::new(),
    }
}

pub fn enforce_browser_capture_settings_policy(
    mut settings: BrowserCaptureSettings,
) -> BrowserCaptureSettings {
    if !browser_experimental_capture_enabled() {
        settings.auto_intercept = false;
        settings.forward_headers = false;
        settings.forward_headers_mode = BrowserForwardHeadersMode::Disabled;
    }
    settings
}

fn parse_browser_capture_settings(raw: &str) -> BrowserCaptureSettings {
    let mut settings = default_browser_capture_settings();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return settings;
    };
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
    let output_dir = PathBuf::from(settings.default_save_dir)
        .join("Vibe Downloader Extensions")
        .join(format!("v{}", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Could not create extension export folder: {e}"))?;

    let manifest_template = fs::read_to_string(&template_path)
        .map_err(|e| format!("Could not read extension manifest template: {e}"))?;
    let background_template = fs::read_to_string(source_dir.join("background.js"))
        .map_err(|e| format!("Could not read extension background script: {e}"))?;
    let shared_files = ["logger.js", "popup.html", "popup.js", "popup.css"];
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

        let manifest = extension_manifest(&manifest_template, &variant)?;
        let background = extension_background(&background_template, variant.browser_kind);
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
    let native_host_path = native_host_path().map(|path| path.to_string_lossy().to_string());
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
    Ok(BrowserIntegrationStatus {
        native_host_name: NATIVE_HOST_NAME.to_string(),
        native_host_path,
        extension_core_path,
        experimental_capture_enabled: browser_experimental_capture_enabled(),
        realtime: BrowserRealtimeStatus {
            ws_url: realtime.ws_url,
            connected: realtime.connected,
        },
        capture: browser_capture_settings(&state.pool).await?,
        browsers,
    })
}

fn validate_handoff(
    input: &BrowserHandoffInput,
    registry: &EngineRegistry,
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
    registry.engine_for_uri(parsed.as_str())?;
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("Browser handoff URLs must not contain embedded credentials.".to_string());
    }
    Ok(parsed.to_string())
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

fn extension_package_variants() -> [ExtensionPackageVariant; 4] {
    let release = integration_profile() == "release";
    [
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
            firefox_id: Some(if release {
                FIREFOX_RELEASE_EXTENSION_ID
            } else {
                FIREFOX_DEV_EXTENSION_ID
            }),
            install_note: "Firefox release builds require a signed XPI. Use this local package for development profiles.",
        },
        ExtensionPackageVariant {
            id: "opera",
            target: "Opera",
            browser_kind: "opera",
            extension: "zip",
            firefox_id: None,
            install_note: "Load the package from Opera's extensions page after enabling developer mode.",
        },
    ]
}

fn extension_manifest(template: &str, variant: &ExtensionPackageVariant) -> Result<String, String> {
    let mut manifest: serde_json::Value = serde_json::from_str(template)
        .map_err(|e| format!("Invalid extension manifest template: {e}"))?;
    manifest["version"] = serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string());
    apply_extension_capture_permissions(&mut manifest);
    if variant.id == "firefox" {
        manifest["name"] = serde_json::Value::String("Vibe Downloader (Firefox)".to_string());
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

fn extension_background(template: &str, browser_kind: &str) -> String {
    template
        .replace("__VIBE_BROWSER_KIND__", browser_kind)
        .replace(
            "__VIBE_EXPERIMENTAL_CAPTURE__",
            if browser_experimental_capture_enabled() {
                "true"
            } else {
                "false"
            },
        )
}

fn apply_extension_capture_permissions(manifest: &mut serde_json::Value) {
    if browser_experimental_capture_enabled() {
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

    zip.finish()
        .map_err(|e| format!("Could not finalize extension package: {e}"))?;
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
    let manifest_path = manifest_path(app, browser)?;
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create native host manifest folder: {e}"))?;
    }
    fs::write(&manifest_path, manifest_json(browser)?)
        .map_err(|e| format!("Could not write native host manifest: {e}"))?;

    #[cfg(target_os = "windows")]
    install_windows_registry(browser, &manifest_path)?;

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

fn manifest_json(browser: BrowserKind) -> Result<String, String> {
    let path = native_host_path()
        .ok_or_else(|| "Native host executable path could not be resolved.".to_string())?;
    let path = path.to_string_lossy().replace('\\', "\\\\");
    let value = if matches!(browser, BrowserKind::Firefox) {
        serde_json::json!({
            "name": NATIVE_HOST_NAME,
            "description": "Vibe Downloader browser handoff host",
            "path": path,
            "type": "stdio",
            "allowed_extensions": [extension_id(browser).unwrap_or(FIREFOX_DEV_EXTENSION_ID)]
        })
    } else {
        let extension_id = extension_id(browser).unwrap_or(CHROMIUM_DEV_EXTENSION_ID);
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
    match option_env!("VIBE_BROWSER_PROFILE") {
        Some("release") => "release",
        _ if cfg!(debug_assertions) => "dev",
        _ => "release",
    }
}

fn extension_id(browser: BrowserKind) -> Option<&'static str> {
    let release = integration_profile() == "release";
    match browser {
        BrowserKind::Firefox => Some(if release {
            FIREFOX_RELEASE_EXTENSION_ID
        } else {
            FIREFOX_DEV_EXTENSION_ID
        }),
        BrowserKind::Edge => Some(if release {
            EDGE_RELEASE_EXTENSION_ID
        } else {
            CHROMIUM_DEV_EXTENSION_ID
        }),
        BrowserKind::Safari => None,
        _ => Some(if release {
            CHROMIUM_RELEASE_EXTENSION_ID
        } else {
            CHROMIUM_DEV_EXTENSION_ID
        }),
    }
}

fn native_host_path() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let exe_name = if cfg!(target_os = "windows") {
        "vibe-native-host.exe"
    } else {
        "vibe-native-host"
    };
    Some(current.with_file_name(exe_name))
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
    !matches!(browser, BrowserKind::Safari) || cfg!(target_os = "macos")
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
