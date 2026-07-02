use std::path::PathBuf;

use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::{
    db,
    events::{emit_queue_changed, emit_settings_changed},
    models::{AppAccentColor, AppSettings, CompletionAction},
    proxy::{self, AppProxyMode},
    AppState,
};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsInput {
    pub max_active_tasks: Option<i32>,
    pub default_save_dir: Option<String>,
    pub global_speed_limit_bps: Option<String>,
    pub multi_connection_threshold_bytes: Option<String>,
    pub segment_count: Option<i32>,
    pub max_connections_per_host: Option<i32>,
    pub system_notifications: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub start_on_boot: Option<bool>,
    pub auto_resume_on_startup: Option<bool>,
    pub floating_window_enabled: Option<bool>,
    pub clipboard_monitor_enabled: Option<bool>,
    pub accent_color: Option<AppAccentColor>,
    pub proxy_mode: Option<AppProxyMode>,
    pub proxy_url: Option<String>,
    pub proxy_no_proxy: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub clear_proxy_password: Option<bool>,
    pub schedule_download_window_enabled: Option<bool>,
    pub schedule_download_window_start: Option<String>,
    pub schedule_download_window_end: Option<String>,
    pub schedule_speed_limit_window_enabled: Option<bool>,
    pub schedule_speed_limit_window_start: Option<String>,
    pub schedule_speed_limit_window_end: Option<String>,
    pub schedule_speed_limit_bps: Option<String>,
    pub titlebar_gradient_enabled: Option<bool>,
    pub completion_action: Option<CompletionAction>,
    pub completion_countdown_seconds: Option<i32>,
    pub completion_run_command: Option<String>,
    pub delete_to_trash: Option<bool>,
    pub auto_update_check_enabled: Option<bool>,
    /// Custom ffmpeg binary path. `None` leaves the value unchanged; `Some(None)`
    /// clears the setting (falls back to env/PATH lookup).
    pub ffmpeg_path: Option<Option<String>>,
    /// F-7: Global BitTorrent upload speed limit (bytes/sec). `None` leaves
    /// the value unchanged; `Some(None)` clears the limit (unlimited).
    pub bt_upload_limit_bps: Option<Option<String>>,
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    db::get_settings(&state.pool, default_download_dir(&app)?).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateSettingsInput,
) -> Result<AppSettings, String> {
    let current = db::get_settings(&state.pool, default_download_dir(&app)?).await?;
    let max_active_tasks = input
        .max_active_tasks
        .unwrap_or(current.max_active_tasks)
        .clamp(db::MIN_MAX_ACTIVE_TASKS, db::MAX_MAX_ACTIVE_TASKS);
    let default_save_dir =
        resolve_save_dir(&app, input.default_save_dir, current.default_save_dir)?;
    let global_speed_limit_bps = input
        .global_speed_limit_bps
        .and_then(|value| db::normalize_speed_limit_bps(&value));
    let multi_connection_threshold_bytes = input
        .multi_connection_threshold_bytes
        .and_then(|value| db::normalize_multi_connection_threshold_bytes(&value))
        .unwrap_or(current.multi_connection_threshold_bytes);
    let segment_count = input
        .segment_count
        .unwrap_or(current.segment_count)
        .clamp(db::MIN_SEGMENT_COUNT, db::MAX_SEGMENT_COUNT);
    let max_connections_per_host = input
        .max_connections_per_host
        .unwrap_or(current.max_connections_per_host)
        .clamp(
            db::MIN_MAX_CONNECTIONS_PER_HOST,
            db::MAX_MAX_CONNECTIONS_PER_HOST,
        );
    let system_notifications = input
        .system_notifications
        .unwrap_or(current.system_notifications);
    let close_to_tray = input.close_to_tray.unwrap_or(current.close_to_tray);
    let start_on_boot = input.start_on_boot.unwrap_or(current.start_on_boot);
    let auto_resume_on_startup = input
        .auto_resume_on_startup
        .unwrap_or(current.auto_resume_on_startup);
    let floating_window_enabled = input
        .floating_window_enabled
        .unwrap_or(current.floating_window_enabled);
    let clipboard_monitor_enabled = input
        .clipboard_monitor_enabled
        .unwrap_or(current.clipboard_monitor_enabled);
    let accent_color = input.accent_color.unwrap_or(current.accent_color);
    let proxy_mode = input.proxy_mode.unwrap_or(current.proxy_mode);
    let proxy_url = input
        .proxy_url
        .as_deref()
        .and_then(db::normalize_proxy_url)
        .unwrap_or_else(|| {
            if input.proxy_url.is_some() {
                String::new()
            } else {
                current.proxy_url.clone()
            }
        });
    let proxy_no_proxy = input
        .proxy_no_proxy
        .as_deref()
        .and_then(db::normalize_proxy_no_proxy)
        .unwrap_or_else(|| {
            if input.proxy_no_proxy.is_some() {
                String::new()
            } else {
                current.proxy_no_proxy.clone()
            }
        });
    let mut proxy_username = input
        .proxy_username
        .as_deref()
        .and_then(db::normalize_proxy_optional)
        .unwrap_or_else(|| {
            if input.proxy_username.is_some() {
                String::new()
            } else {
                current.proxy_username.clone()
            }
        });
    proxy::validate_proxy_settings(proxy_mode, &proxy_url, &proxy_username)?;
    let mut proxy_password_saved = current.proxy_password_saved;
    if input.clear_proxy_password.unwrap_or(false) {
        proxy::clear_proxy_password()?;
        proxy_password_saved = false;
    }
    if let Some(password) = input.proxy_password.as_deref().map(str::trim) {
        if !password.is_empty() {
            proxy::save_proxy_password(password)?;
            proxy_password_saved = true;
        }
    }
    if proxy_mode != AppProxyMode::Custom {
        proxy_username.clear();
    }
    let schedule_download_window_enabled = input
        .schedule_download_window_enabled
        .unwrap_or(current.schedule_download_window_enabled);
    let schedule_download_window_start = input
        .schedule_download_window_start
        .as_deref()
        .and_then(db::normalize_local_time)
        .unwrap_or_else(|| current.schedule_download_window_start.clone());
    let schedule_download_window_end = input
        .schedule_download_window_end
        .as_deref()
        .and_then(db::normalize_local_time)
        .unwrap_or_else(|| current.schedule_download_window_end.clone());
    let schedule_speed_limit_window_enabled = input
        .schedule_speed_limit_window_enabled
        .unwrap_or(current.schedule_speed_limit_window_enabled);
    let schedule_speed_limit_window_start = input
        .schedule_speed_limit_window_start
        .as_deref()
        .and_then(db::normalize_local_time)
        .unwrap_or_else(|| current.schedule_speed_limit_window_start.clone());
    let schedule_speed_limit_window_end = input
        .schedule_speed_limit_window_end
        .as_deref()
        .and_then(db::normalize_local_time)
        .unwrap_or_else(|| current.schedule_speed_limit_window_end.clone());
    let schedule_speed_limit_bps = input
        .schedule_speed_limit_bps
        .and_then(|value| db::normalize_speed_limit_bps(&value));
    let titlebar_gradient_enabled = input
        .titlebar_gradient_enabled
        .unwrap_or(current.titlebar_gradient_enabled);
    let completion_action = input.completion_action.unwrap_or(current.completion_action);
    let completion_countdown_seconds = input
        .completion_countdown_seconds
        .unwrap_or(current.completion_countdown_seconds)
        .clamp(5, 300);
    let completion_run_command = input
        .completion_run_command
        .unwrap_or_else(|| current.completion_run_command.clone());
    // S-4: Audit-log changes to the completion run command — this is the
    // field that drives local command execution on task completion.
    if completion_run_command != current.completion_run_command {
        tracing::info!(
            setting = "completion_run_command",
            old_value = %truncate_for_audit(&current.completion_run_command),
            new_value = %truncate_for_audit(&completion_run_command),
            "sensitive setting changed"
        );
    }
    let delete_to_trash = input.delete_to_trash.unwrap_or(current.delete_to_trash);
    let auto_update_check_enabled = input
        .auto_update_check_enabled
        .unwrap_or(current.auto_update_check_enabled);
    let ffmpeg_path = match input.ffmpeg_path {
        Some(value) => value.and_then(|v| db::normalize_ffmpeg_path(&v)),
        None => current.ffmpeg_path.clone(),
    };
    // F-7: BT upload limit. `None` = unchanged; `Some(None)` = clear (unlimited);
    // `Some(Some(v))` = set to normalized value (0/invalid → None).
    let bt_upload_limit_bps = match input.bt_upload_limit_bps {
        Some(value) => value.and_then(|v| db::normalize_speed_limit_bps(&v)),
        None => current.bt_upload_limit_bps.clone(),
    };
    let settings = AppSettings {
        max_active_tasks,
        default_save_dir,
        global_speed_limit_bps,
        multi_connection_threshold_bytes,
        segment_count,
        max_connections_per_host,
        system_notifications,
        close_to_tray,
        start_on_boot,
        auto_resume_on_startup,
        floating_window_enabled,
        clipboard_monitor_enabled,
        accent_color,
        proxy_mode,
        proxy_url,
        proxy_no_proxy,
        proxy_username,
        proxy_password_saved,
        schedule_download_window_enabled,
        schedule_download_window_start,
        schedule_download_window_end,
        schedule_speed_limit_window_enabled,
        schedule_speed_limit_window_start,
        schedule_speed_limit_window_end,
        schedule_speed_limit_bps,
        titlebar_gradient_enabled,
        completion_action,
        completion_countdown_seconds,
        completion_run_command,
        delete_to_trash,
        auto_update_check_enabled,
        ffmpeg_path,
        bt_upload_limit_bps,
    };

    db::upsert_settings(&state.pool, &settings).await?;
    state
        .engine_registry
        .set_proxy_config(proxy::ResolvedProxyConfig::from_settings(&settings))
        .await;
    state.speed_limiter.set_limit(db::parse_speed_limit_bps(
        settings.global_speed_limit_bps.as_deref(),
    )).await;
    super::floating::sync_floating_status_window(&app, settings.floating_window_enabled)?;
    emit_settings_changed(&app);
    emit_queue_changed(&app);
    state.scheduler.clone().dispatch(app.clone(), state.pool.clone()).await;
    if let Err(error) = super::tasks::check_schedule_preemption(app, state).await {
        tracing::warn!(error = %error, "schedule preemption check failed after settings update");
    }

    Ok(settings)
}

pub fn default_download_dir(app: &AppHandle) -> Result<String, String> {
    app.path()
        .download_dir()
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|e| format!("Failed to resolve the Downloads folder: {e}"))
}

fn resolve_save_dir(
    app: &AppHandle,
    value: Option<String>,
    current: String,
) -> Result<String, String> {
    let dir = match value.as_deref().map(str::trim) {
        Some("") => PathBuf::from(default_download_dir(app)?),
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(current),
    };

    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

/// Truncate a sensitive setting value for audit logging. Keeps the first
/// 100 characters so long commands are still identifiable without flooding
/// logs with potentially sensitive arguments.
fn truncate_for_audit(value: &str) -> &str {
    if value.len() <= 100 {
        value
    } else {
        &value[..100]
    }
}
