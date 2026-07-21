//! Environment health checks for Settings → Environment.
//!
//! Aggregates native host, browser manifests, ffmpeg, proxy reachability,
//! save-dir writability, disk space, and database backup status into one report.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::Url;
use tauri::{AppHandle, State};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::{
    commands::{browser, settings::default_download_dir},
    db,
    download::ffmpeg::{ffmpeg_path, probe_ffmpeg_version_at_path},
    events::emit_browser_integration_changed,
    models::{
        BrowserKind, EnvironmentFixAction, EnvironmentFixInput, EnvironmentFixKind,
        EnvironmentFixResult, EnvironmentHealthItem, EnvironmentHealthReport,
        EnvironmentHealthStatus,
    },
    platform,
    proxy::{AppProxyMode, ResolvedProxyConfig},
    AppState,
};

const PROXY_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const DISK_WARN_BYTES: u64 = 512 * 1024 * 1024;
const DISK_ERROR_BYTES: u64 = 64 * 1024 * 1024;
const WRITE_PROBE_NAME: &str = ".vibe-write-probe";

#[tauri::command]
#[specta::specta]
pub async fn get_environment_health(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<EnvironmentHealthReport, String> {
    build_environment_health(&app, state.inner()).await
}

#[tauri::command]
#[specta::specta]
pub async fn run_environment_fix(
    app: AppHandle,
    state: State<'_, AppState>,
    input: EnvironmentFixInput,
) -> Result<EnvironmentFixResult, String> {
    match input.kind {
        EnvironmentFixKind::InstallNativeHost => {
            let status = browser::integration_status(&app, state.inner()).await?;
            if !status.native_host_ready {
                return Ok(EnvironmentFixResult {
                    ok: false,
                    message: status.native_host_error.unwrap_or_else(|| {
                        "Native Messaging host binary is missing. Reinstall the desktop app."
                            .to_string()
                    }),
                    focus_section: Some("browser-integration".into()),
                    refresh: true,
                });
            }

            let targets: Vec<BrowserKind> = if let Some(browser) = input.browser {
                vec![browser]
            } else {
                status
                    .browsers
                    .iter()
                    .filter(|entry| {
                        entry.detected && entry.supported_on_platform && !entry.manifest_installed
                    })
                    .map(|entry| entry.browser)
                    .collect()
            };

            if targets.is_empty() {
                return Ok(EnvironmentFixResult {
                    ok: true,
                    message: "No browsers need a Native Messaging manifest install.".into(),
                    focus_section: Some("browser-integration".into()),
                    refresh: true,
                });
            }

            for browser_kind in &targets {
                if !browser::browser_supported_on_platform(*browser_kind) {
                    continue;
                }
                browser::install_manifest(&app, *browser_kind)?;
            }
            emit_browser_integration_changed(&app);
            Ok(EnvironmentFixResult {
                ok: true,
                message: format!(
                    "Installed Native Messaging manifests for {} browser(s).",
                    targets.len()
                ),
                focus_section: Some("browser-integration".into()),
                refresh: true,
            })
        }
        EnvironmentFixKind::OpenPath => {
            let kind = input.path_kind.as_deref().unwrap_or("data");
            let path = resolve_open_path(&app, state.inner(), kind).await?;
            platform::open_path(&path)?;
            Ok(EnvironmentFixResult {
                ok: true,
                message: format!("Opened {}", path.display()),
                focus_section: None,
                refresh: false,
            })
        }
        EnvironmentFixKind::FocusSetting => {
            let section = input
                .section
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "environment".into());
            Ok(EnvironmentFixResult {
                ok: true,
                message: format!("Focus settings section: {section}"),
                focus_section: Some(section),
                refresh: false,
            })
        }
        EnvironmentFixKind::ExportBackup => Ok(EnvironmentFixResult {
            ok: true,
            message: "Choose a destination to export a database backup.".into(),
            focus_section: Some("data-backup".into()),
            refresh: false,
        }),
        EnvironmentFixKind::CheckForUpdate => Ok(EnvironmentFixResult {
            ok: true,
            message: "Check for updates from the frontend updater.".into(),
            focus_section: Some("about-updates".into()),
            refresh: false,
        }),
    }
}

async fn build_environment_health(
    app: &AppHandle,
    state: &AppState,
) -> Result<EnvironmentHealthReport, String> {
    let settings = db::get_settings(&state.pool, default_download_dir(app)?).await?;
    let browser_status = browser::integration_status(app, state).await?;

    let mut items = Vec::with_capacity(7);
    items.push(check_native_host(&browser_status));
    items.push(check_browser(&browser_status));
    items.push(check_ffmpeg(state).await);
    items.push(check_proxy(&ResolvedProxyConfig::from_settings(&settings)).await);
    items.push(check_save_dir(&settings.default_save_dir));
    items.push(check_disk(&settings.default_save_dir));
    items.push(check_database(app, &state.pool).await?);

    Ok(EnvironmentHealthReport {
        checked_at_ms: now_ms().to_string(),
        app_version: app.package_info().version.to_string(),
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        items,
    })
}

fn check_native_host(status: &crate::models::BrowserIntegrationStatus) -> EnvironmentHealthItem {
    if status.native_host_ready {
        EnvironmentHealthItem {
            id: "native_host".into(),
            status: EnvironmentHealthStatus::Ok,
            summary: "Native Messaging host binary is ready.".into(),
            detail: status.native_host_path.clone(),
            suggested_actions: vec![],
        }
    } else {
        EnvironmentHealthItem {
            id: "native_host".into(),
            status: EnvironmentHealthStatus::Error,
            summary: "Native Messaging host binary is missing or not executable.".into(),
            detail: status.native_host_error.clone(),
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::FocusSetting,
                browser: None,
                path_kind: None,
                section: Some("browser-integration".into()),
            }],
        }
    }
}

fn check_browser(status: &crate::models::BrowserIntegrationStatus) -> EnvironmentHealthItem {
    let detected: Vec<_> = status
        .browsers
        .iter()
        .filter(|entry| entry.detected && entry.supported_on_platform)
        .collect();
    let missing_manifest: Vec<_> = detected
        .iter()
        .filter(|entry| !entry.manifest_installed)
        .copied()
        .collect();
    let last_errors: Vec<String> = status
        .browsers
        .iter()
        .filter_map(|entry| {
            entry
                .last_error
                .as_ref()
                .map(|err| format!("{}: {err}", entry.display_name))
        })
        .collect();

    let ws_detail = match (status.realtime.ws_url.as_deref(), status.realtime.connected) {
        (Some(url), true) => format!("Realtime bridge connected ({url})."),
        (Some(url), false) => {
            format!("Realtime bridge listening but no extension connected ({url}).")
        }
        (None, _) => "Realtime bridge is not available.".to_string(),
    };

    let mut detail_parts = vec![ws_detail];
    if !last_errors.is_empty() {
        detail_parts.push(format!("Recent handoff errors: {}", last_errors.join("; ")));
    }

    if detected.is_empty() {
        return EnvironmentHealthItem {
            id: "browser".into(),
            status: EnvironmentHealthStatus::Unknown,
            summary: "No supported browsers were detected on this machine.".into(),
            detail: Some(detail_parts.join(" ")),
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::FocusSetting,
                browser: None,
                path_kind: None,
                section: Some("browser-integration".into()),
            }],
        };
    }

    if !status.native_host_ready {
        return EnvironmentHealthItem {
            id: "browser".into(),
            status: EnvironmentHealthStatus::Error,
            summary: "Browser integration cannot install manifests without the native host.".into(),
            detail: Some(detail_parts.join(" ")),
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::FocusSetting,
                browser: None,
                path_kind: None,
                section: Some("browser-integration".into()),
            }],
        };
    }

    if !missing_manifest.is_empty() {
        let names = missing_manifest
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut actions = vec![EnvironmentFixAction {
            kind: EnvironmentFixKind::InstallNativeHost,
            browser: None,
            path_kind: None,
            section: None,
        }];
        for entry in &missing_manifest {
            actions.push(EnvironmentFixAction {
                kind: EnvironmentFixKind::InstallNativeHost,
                browser: Some(entry.browser),
                path_kind: None,
                section: None,
            });
        }
        return EnvironmentHealthItem {
            id: "browser".into(),
            status: EnvironmentHealthStatus::Warn,
            summary: format!(
                "{} detected browser(s) are missing Native Messaging manifests: {names}.",
                missing_manifest.len()
            ),
            detail: Some(detail_parts.join(" ")),
            suggested_actions: actions,
        };
    }

    let status_level = if status.realtime.connected {
        EnvironmentHealthStatus::Ok
    } else {
        EnvironmentHealthStatus::Warn
    };
    let summary = if status.realtime.connected {
        format!(
            "{} detected browser(s) have Native Messaging manifests installed.",
            detected.len()
        )
    } else {
        format!(
            "{} browser manifest(s) installed; extension realtime bridge is not connected yet.",
            detected.len()
        )
    };

    EnvironmentHealthItem {
        id: "browser".into(),
        status: status_level,
        summary,
        detail: Some(detail_parts.join(" ")),
        suggested_actions: vec![],
    }
}

async fn check_ffmpeg(state: &AppState) -> EnvironmentHealthItem {
    match ffmpeg_path(Some(&state.pool)).await {
        Some(path) => match probe_ffmpeg_version_at_path(&path).await {
            Ok(version) => EnvironmentHealthItem {
                id: "ffmpeg".into(),
                status: EnvironmentHealthStatus::Ok,
                summary: "ffmpeg is available.".into(),
                detail: Some(format!("{} ({})", version, path.display())),
                suggested_actions: vec![],
            },
            Err(error) => EnvironmentHealthItem {
                id: "ffmpeg".into(),
                status: EnvironmentHealthStatus::Error,
                summary: "ffmpeg path is set but the binary could not be probed.".into(),
                detail: Some(error),
                suggested_actions: vec![EnvironmentFixAction {
                    kind: EnvironmentFixKind::FocusSetting,
                    browser: None,
                    path_kind: None,
                    section: Some("external-tools".into()),
                }],
            },
        },
        None => EnvironmentHealthItem {
            id: "ffmpeg".into(),
            status: EnvironmentHealthStatus::Error,
            summary: "ffmpeg was not found. HLS/DASH remuxing will fail.".into(),
            detail: None,
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::FocusSetting,
                browser: None,
                path_kind: None,
                section: Some("external-tools".into()),
            }],
        },
    }
}

async fn check_proxy(config: &ResolvedProxyConfig) -> EnvironmentHealthItem {
    match config.mode {
        AppProxyMode::Off => EnvironmentHealthItem {
            id: "proxy".into(),
            status: EnvironmentHealthStatus::Ok,
            summary: "Proxy is disabled.".into(),
            detail: None,
            suggested_actions: vec![],
        },
        AppProxyMode::System => EnvironmentHealthItem {
            id: "proxy".into(),
            status: EnvironmentHealthStatus::Warn,
            summary: "System proxy mode cannot be probed reliably from the app.".into(),
            detail: Some(
                "Vibe inherits the OS proxy; use a custom proxy URL if you need a handshake check."
                    .into(),
            ),
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::FocusSetting,
                browser: None,
                path_kind: None,
                section: Some("network".into()),
            }],
        },
        AppProxyMode::Custom => match probe_custom_proxy(config).await {
            Ok(detail) => EnvironmentHealthItem {
                id: "proxy".into(),
                status: EnvironmentHealthStatus::Ok,
                summary: "Custom proxy endpoint accepted a short handshake.".into(),
                detail: Some(detail),
                suggested_actions: vec![],
            },
            Err(error) => EnvironmentHealthItem {
                id: "proxy".into(),
                status: EnvironmentHealthStatus::Error,
                summary: "Custom proxy handshake failed.".into(),
                detail: Some(error),
                suggested_actions: vec![EnvironmentFixAction {
                    kind: EnvironmentFixKind::FocusSetting,
                    browser: None,
                    path_kind: None,
                    section: Some("network".into()),
                }],
            },
        },
    }
}

/// Probe reachability of a custom proxy without tunneling to an external business URL.
pub async fn probe_custom_proxy(config: &ResolvedProxyConfig) -> Result<String, String> {
    let url_raw = config
        .url
        .as_deref()
        .ok_or_else(|| "Custom proxy URL is empty.".to_string())?;
    let url = Url::parse(url_raw).map_err(|e| format!("Invalid proxy URL: {e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "Proxy URL is missing a host.".to_string())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Proxy URL is missing a port.".to_string())?;
    let addr = format!("{host}:{port}");

    match url.scheme() {
        "socks5" => {
            timeout(
                PROXY_PROBE_TIMEOUT,
                socks5_greeting_handshake(&addr, config),
            )
            .await
            .map_err(|_| "Proxy handshake timed out.".to_string())??;
            Ok(format!("socks5://{addr} greeting ok"))
        }
        "http" | "https" => {
            timeout(PROXY_PROBE_TIMEOUT, TcpStream::connect(&addr))
                .await
                .map_err(|_| "Proxy TCP connect timed out.".to_string())?
                .map_err(|e| format!("Proxy TCP connect failed: {e}"))?;
            Ok(format!("{}://{addr} tcp ok", url.scheme()))
        }
        other => Err(format!("Unsupported proxy scheme: {other}")),
    }
}

async fn socks5_greeting_handshake(addr: &str, config: &ResolvedProxyConfig) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| format!("SOCKS5 TCP connect failed: {e}"))?;

    let has_auth = config
        .username
        .as_deref()
        .is_some_and(|value| !value.is_empty());
    if has_auth {
        // Offer username/password only.
        stream
            .write_all(&[0x05, 0x01, 0x02])
            .await
            .map_err(|e| format!("SOCKS5 greeting write failed: {e}"))?;
    } else {
        stream
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .map_err(|e| format!("SOCKS5 greeting write failed: {e}"))?;
    }

    let mut method = [0_u8; 2];
    stream
        .read_exact(&mut method)
        .await
        .map_err(|e| format!("SOCKS5 greeting read failed: {e}"))?;
    if method[0] != 0x05 {
        return Err("Proxy did not speak SOCKS5.".into());
    }
    if has_auth {
        if method[1] != 0x02 {
            return Err("Proxy rejected username/password authentication.".into());
        }
        let user = config.username.as_deref().unwrap_or("");
        let pass = config.password.as_deref().unwrap_or("");
        if user.len() > 255 || pass.len() > 255 {
            return Err("Proxy username or password is too long.".into());
        }
        let mut auth = Vec::with_capacity(3 + user.len() + pass.len());
        auth.push(0x01);
        auth.push(user.len() as u8);
        auth.extend_from_slice(user.as_bytes());
        auth.push(pass.len() as u8);
        auth.extend_from_slice(pass.as_bytes());
        stream
            .write_all(&auth)
            .await
            .map_err(|e| format!("SOCKS5 auth write failed: {e}"))?;
        let mut auth_resp = [0_u8; 2];
        stream
            .read_exact(&mut auth_resp)
            .await
            .map_err(|e| format!("SOCKS5 auth read failed: {e}"))?;
        if auth_resp[1] != 0x00 {
            return Err("SOCKS5 proxy authentication failed.".into());
        }
    } else if method[1] != 0x00 {
        return Err(format!(
            "SOCKS5 proxy selected unsupported auth method {:#04x}.",
            method[1]
        ));
    }
    // Stop after greeting/auth — do not issue CONNECT to an external host.
    Ok(())
}

/// Create the directory if needed, write and delete a temp probe file.
pub fn probe_directory_writable(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;
    let probe = path.join(WRITE_PROBE_NAME);
    std::fs::write(&probe, b"vibe-write-probe")
        .map_err(|e| format!("Could not write to the download directory: {e}"))?;
    std::fs::remove_file(&probe)
        .map_err(|e| format!("Could not remove the write probe file: {e}"))?;
    Ok(())
}

fn check_save_dir(save_dir: &str) -> EnvironmentHealthItem {
    let path = PathBuf::from(save_dir);
    match probe_directory_writable(&path) {
        Ok(()) => EnvironmentHealthItem {
            id: "save_dir".into(),
            status: EnvironmentHealthStatus::Ok,
            summary: "Default save directory is writable.".into(),
            detail: Some(path.to_string_lossy().into_owned()),
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::OpenPath,
                browser: None,
                path_kind: Some("save_dir".into()),
                section: None,
            }],
        },
        Err(error) => EnvironmentHealthItem {
            id: "save_dir".into(),
            status: EnvironmentHealthStatus::Error,
            summary: "Default save directory is not writable.".into(),
            detail: Some(error),
            suggested_actions: vec![
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::FocusSetting,
                    browser: None,
                    path_kind: None,
                    section: Some("downloads".into()),
                },
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::OpenPath,
                    browser: None,
                    path_kind: Some("save_dir".into()),
                    section: None,
                },
            ],
        },
    }
}

/// Map available bytes to a health status.
pub fn disk_status_for_available(available_bytes: u64) -> EnvironmentHealthStatus {
    if available_bytes < DISK_ERROR_BYTES {
        EnvironmentHealthStatus::Error
    } else if available_bytes < DISK_WARN_BYTES {
        EnvironmentHealthStatus::Warn
    } else {
        EnvironmentHealthStatus::Ok
    }
}

fn check_disk(save_dir: &str) -> EnvironmentHealthItem {
    match query_disk_space_for_path(Path::new(save_dir)) {
        Ok((probe_path, total, available)) => {
            let status = disk_status_for_available(available);
            let summary = match status {
                EnvironmentHealthStatus::Ok => {
                    "Default save volume has sufficient free space.".to_string()
                }
                EnvironmentHealthStatus::Warn => {
                    "Default save volume is running low on free space.".to_string()
                }
                EnvironmentHealthStatus::Error => {
                    "Default save volume is critically low on free space.".to_string()
                }
                EnvironmentHealthStatus::Unknown => "Could not classify disk space.".to_string(),
            };
            EnvironmentHealthItem {
                id: "disk".into(),
                status,
                summary,
                detail: Some(format!(
                    "path={} available={} total={}",
                    probe_path.display(),
                    available,
                    total
                )),
                suggested_actions: vec![EnvironmentFixAction {
                    kind: EnvironmentFixKind::OpenPath,
                    browser: None,
                    path_kind: Some("save_dir".into()),
                    section: None,
                }],
            }
        }
        Err(error) => EnvironmentHealthItem {
            id: "disk".into(),
            status: EnvironmentHealthStatus::Unknown,
            summary: "Could not query disk space for the default save directory.".into(),
            detail: Some(error),
            suggested_actions: vec![EnvironmentFixAction {
                kind: EnvironmentFixKind::FocusSetting,
                browser: None,
                path_kind: None,
                section: Some("downloads".into()),
            }],
        },
    }
}

fn query_disk_space_for_path(path: &Path) -> Result<(PathBuf, u64, u64), String> {
    let mut probe = path;
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return Err("path does not exist and has no parent".into()),
        }
    }
    let total = fs4::free_space(probe).map_err(|e| format!("failed to read disk space: {e}"))?;
    let available =
        fs4::available_space(probe).map_err(|e| format!("failed to read available space: {e}"))?;
    Ok((probe.to_path_buf(), total, available))
}

/// Find the newest sibling backup created as `<db>.bak-*` / `*.db.bak-*`.
pub fn find_latest_db_backup(db_path: &Path) -> Option<PathBuf> {
    let parent = db_path.parent()?;
    let file_name = db_path.file_name()?.to_string_lossy();
    // connection.rs uses `with_extension(format!("db.bak-{timestamp}"))`,
    // which turns `vibe.db` into `vibe.db.bak-<ts>`.
    let expected_prefix = format!("{file_name}.bak-");

    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(parent).ok()?.flatten() {
        let path = entry.path();
        let name = match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => continue,
        };
        if !(name.starts_with(&expected_prefix) || name.contains(".db.bak-")) {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if best.as_ref().is_none_or(|(secs, _)| modified >= *secs) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, path)| path)
}

async fn check_database(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
) -> Result<EnvironmentHealthItem, String> {
    let db_path = platform::db_path(app)?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to run database integrity check: {e}"))?;

    if integrity != "ok" {
        return Ok(EnvironmentHealthItem {
            id: "database".into(),
            status: EnvironmentHealthStatus::Error,
            summary: "Database integrity check failed.".into(),
            detail: Some(format!("path={} integrity={integrity}", db_path.display())),
            suggested_actions: vec![
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::OpenPath,
                    browser: None,
                    path_kind: Some("data".into()),
                    section: None,
                },
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::ExportBackup,
                    browser: None,
                    path_kind: None,
                    section: Some("data-backup".into()),
                },
            ],
        });
    }

    let latest_bak = find_latest_db_backup(&db_path);
    match latest_bak {
        Some(path) => Ok(EnvironmentHealthItem {
            id: "database".into(),
            status: EnvironmentHealthStatus::Ok,
            summary: "Database integrity is ok and an automatic backup file was found.".into(),
            detail: Some(format!(
                "path={} latest_backup={}",
                db_path.display(),
                path.display()
            )),
            suggested_actions: vec![
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::OpenPath,
                    browser: None,
                    path_kind: Some("data".into()),
                    section: None,
                },
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::ExportBackup,
                    browser: None,
                    path_kind: None,
                    section: Some("data-backup".into()),
                },
            ],
        }),
        None => Ok(EnvironmentHealthItem {
            id: "database".into(),
            status: EnvironmentHealthStatus::Warn,
            summary: "Database integrity is ok, but no automatic .db.bak file was found yet."
                .into(),
            detail: Some(format!("path={}", db_path.display())),
            suggested_actions: vec![
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::ExportBackup,
                    browser: None,
                    path_kind: None,
                    section: Some("data-backup".into()),
                },
                EnvironmentFixAction {
                    kind: EnvironmentFixKind::OpenPath,
                    browser: None,
                    path_kind: Some("data".into()),
                    section: None,
                },
            ],
        }),
    }
}

async fn resolve_open_path(
    app: &AppHandle,
    state: &AppState,
    kind: &str,
) -> Result<PathBuf, String> {
    match kind {
        "save_dir" => {
            let settings = db::get_settings(&state.pool, default_download_dir(app)?).await?;
            Ok(PathBuf::from(settings.default_save_dir))
        }
        "log" => platform::app_log_dir(),
        _ => {
            let db_path = platform::db_path(app)?;
            Ok(db_path.parent().map(Path::to_path_buf).unwrap_or(db_path))
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn disk_thresholds_map_correctly() {
        assert_eq!(
            disk_status_for_available(DISK_ERROR_BYTES - 1),
            EnvironmentHealthStatus::Error
        );
        assert_eq!(
            disk_status_for_available(DISK_WARN_BYTES - 1),
            EnvironmentHealthStatus::Warn
        );
        assert_eq!(
            disk_status_for_available(DISK_WARN_BYTES),
            EnvironmentHealthStatus::Ok
        );
    }

    #[test]
    fn writable_probe_round_trips_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("vibe-env-writable-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        probe_directory_writable(&dir).expect("writable");
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_latest_db_backup_picks_newest() {
        let dir = std::env::temp_dir().join(format!("vibe-env-bak-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db = dir.join("vibe.db");
        std::fs::write(&db, b"db").expect("db");
        let older = dir.join("vibe.db.bak-100");
        let newer = dir.join("vibe.db.bak-200");
        std::fs::write(&older, b"old").expect("old");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&newer, b"new").expect("new");
        let found = find_latest_db_backup(&db).expect("bak");
        assert_eq!(found.file_name().unwrap(), newer.file_name().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn proxy_tcp_probe_succeeds_for_listening_http_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept");
        });
        let config = ResolvedProxyConfig {
            mode: AppProxyMode::Custom,
            url: Some(format!("http://{addr}")),
            no_proxy: None,
            username: None,
            password: None,
        };
        let detail = probe_custom_proxy(&config).await.expect("probe");
        assert!(detail.contains("tcp ok"));
        server.await.expect("server");
    }

    #[tokio::test]
    async fn socks5_greeting_probe_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut greeting = [0_u8; 3];
            stream.read_exact(&mut greeting).await.expect("greeting");
            assert_eq!(greeting, [0x05, 0x01, 0x00]);
            stream.write_all(&[0x05, 0x00]).await.expect("method");
        });
        let config = ResolvedProxyConfig {
            mode: AppProxyMode::Custom,
            url: Some(format!("socks5://{addr}")),
            no_proxy: None,
            username: None,
            password: None,
        };
        let detail = probe_custom_proxy(&config).await.expect("probe");
        assert!(detail.contains("greeting ok"));
        server.await.expect("server");
    }

    #[tokio::test]
    async fn proxy_probe_times_out_when_nothing_listens() {
        // High port unlikely to be open; connect should fail quickly.
        let config = ResolvedProxyConfig {
            mode: AppProxyMode::Custom,
            url: Some("http://127.0.0.1:1".into()),
            no_proxy: None,
            username: None,
            password: None,
        };
        let err = probe_custom_proxy(&config).await.expect_err("should fail");
        assert!(
            err.contains("failed") || err.contains("timed out") || err.contains("refused"),
            "unexpected error: {err}"
        );
    }
}
