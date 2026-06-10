use std::path::PathBuf;

use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::{
    db,
    events::{emit_queue_changed, emit_settings_changed},
    models::{AppFontFamily, AppSettings},
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
    pub floating_window_enabled: Option<bool>,
    pub font_family: Option<AppFontFamily>,
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
    let floating_window_enabled = input
        .floating_window_enabled
        .unwrap_or(current.floating_window_enabled);
    let font_family = input.font_family.unwrap_or(current.font_family);
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
        floating_window_enabled,
        font_family,
    };

    db::upsert_settings(&state.pool, &settings).await?;
    state.speed_limiter.set_limit(db::parse_speed_limit_bps(
        settings.global_speed_limit_bps.as_deref(),
    ));
    super::floating::sync_floating_status_window(&app, settings.floating_window_enabled)?;
    emit_settings_changed(&app);
    emit_queue_changed(&app);
    super::tasks::schedule_queued_tasks(app, state.inner()).await;

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
