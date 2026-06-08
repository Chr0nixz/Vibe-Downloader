#[cfg(target_os = "windows")]
use std::process::Command;
use std::{
    fs,
    path::{Path, PathBuf},
};

use reqwest::Url;
use tauri::{AppHandle, Manager, State};

use crate::{
    db,
    download::EngineRegistry,
    events::{
        emit_browser_handoff_failed, emit_browser_handoff_received,
        emit_browser_integration_changed,
    },
    logging::sanitize_url,
    models::{
        BrowserHandoffInput, BrowserHandoffResult, BrowserIntegrationEntry,
        BrowserIntegrationStatus, BrowserIntegrationUpdateInput, BrowserKind,
    },
    AppState,
};

use super::tasks::{create_task_with_state, CreateTaskInput};

const NATIVE_HOST_NAME: &str = "com.vibe_downloader.native_host";
const CHROMIUM_DEV_EXTENSION_ID: &str = "abcdefghijklmnopabcdefghijklmnop";
const CHROMIUM_RELEASE_EXTENSION_ID: &str = "replace-with-chrome-web-store-id";
const EDGE_RELEASE_EXTENSION_ID: &str = "replace-with-edge-addons-id";
const FIREFOX_DEV_EXTENSION_ID: &str = "vibe-downloader@local";
const FIREFOX_RELEASE_EXTENSION_ID: &str = "vibe-downloader@example.invalid";

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

    let task_result = validate_handoff(&input, &state.engine_registry).map(|url| CreateTaskInput {
        url,
        save_dir: None,
        file_name: sanitize_suggested_file_name(input.suggested_file_name.as_deref()),
        expected_hash_sha256: None,
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

    match create_task_with_state(app.clone(), state, create_input).await {
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

async fn integration_status(
    app: &AppHandle,
    state: &AppState,
) -> Result<BrowserIntegrationStatus, String> {
    let native_host_path = native_host_path().map(|path| path.to_string_lossy().to_string());
    let extension_core_path = extension_core_path()
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().to_string());
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

    Ok(BrowserIntegrationStatus {
        native_host_name: NATIVE_HOST_NAME.to_string(),
        native_host_path,
        extension_core_path,
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
    registry.engine_for_uri(parsed.as_str())?;
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("Browser handoff URLs must not contain embedded credentials.".to_string());
    }
    Ok(parsed.to_string())
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
