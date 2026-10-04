use std::collections::HashMap;

use chrono::Timelike;
use sqlx::{Row, SqlitePool};

use crate::models::{AppAccentColor, AppSettings, CompletionAction};
use crate::proxy::{self, AppProxyMode};

use super::{
    DEFAULT_MAX_ACTIVE_TASKS, DEFAULT_MAX_CONNECTIONS_PER_HOST,
    DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES, DEFAULT_SEGMENT_COUNT, MAX_MAX_ACTIVE_TASKS,
    MAX_MAX_CONNECTIONS_PER_HOST, MAX_MULTI_CONNECTION_THRESHOLD_BYTES, MAX_SEGMENT_COUNT,
    MIN_MAX_ACTIVE_TASKS, MIN_MAX_CONNECTIONS_PER_HOST, MIN_MULTI_CONNECTION_THRESHOLD_BYTES,
    MIN_SEGMENT_COUNT,
};

/// UX-41: declares every settings key this module owns together with
/// `APP_SETTING_KEYS`, the exact list `reset_settings` deletes. One source for
/// both means a new key cannot be added without "restore defaults" covering
/// it — the previous hand-kept array could silently drift from the constants.
macro_rules! app_setting_keys {
    ($($(#[$meta:meta])* $name:ident = $value:literal;)*) => {
        $($(#[$meta])* const $name: &str = $value;)*
        const APP_SETTING_KEYS: &[&str] = &[$($name),*];
    };
}

app_setting_keys! {
    SETTING_MAX_ACTIVE_TASKS = "max_active_tasks";
    SETTING_DEFAULT_SAVE_DIR = "default_save_dir";
    SETTING_GLOBAL_SPEED_LIMIT_BPS = "global_speed_limit_bps";
    SETTING_MULTI_CONNECTION_THRESHOLD_BYTES = "multi_connection_threshold_bytes";
    SETTING_SEGMENT_COUNT = "segment_count";
    SETTING_MAX_CONNECTIONS_PER_HOST = "max_connections_per_host";
    SETTING_SYSTEM_NOTIFICATIONS = "system_notifications";
    SETTING_CLOSE_TO_TRAY = "close_to_tray";
    SETTING_START_ON_BOOT = "start_on_boot";
    SETTING_AUTO_RESUME_ON_STARTUP = "auto_resume_on_startup";
    SETTING_FLOATING_WINDOW_ENABLED = "floating_window_enabled";
    SETTING_CLIPBOARD_MONITOR_ENABLED = "clipboard_monitor_enabled";
    SETTING_ACCENT_COLOR = "accent_color";
    SETTING_PROXY_MODE = "proxy_mode";
    SETTING_PROXY_URL = "proxy_url";
    SETTING_PROXY_NO_PROXY = "proxy_no_proxy";
    SETTING_PROXY_USERNAME = "proxy_username";
    SETTING_PROXY_PASSWORD_SAVED = "proxy_password_saved";
    SETTING_SCHEDULE_DOWNLOAD_WINDOW_ENABLED = "schedule_download_window_enabled";
    SETTING_SCHEDULE_DOWNLOAD_WINDOW_START = "schedule_download_window_start";
    SETTING_SCHEDULE_DOWNLOAD_WINDOW_END = "schedule_download_window_end";
    SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_ENABLED = "schedule_speed_limit_window_enabled";
    SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_START = "schedule_speed_limit_window_start";
    SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_END = "schedule_speed_limit_window_end";
    SETTING_SCHEDULE_SPEED_LIMIT_BPS = "schedule_speed_limit_bps";
    SETTING_TITLEBAR_GRADIENT_ENABLED = "titlebar_gradient_enabled";
    SETTING_COMPLETION_ACTION = "completion_action";
    SETTING_COMPLETION_COUNTDOWN_SECONDS = "completion_countdown_seconds";
    SETTING_COMPLETION_RUN_COMMAND = "completion_run_command";
    /// ARC-58: fire the completion action when the queue drains with failures too.
    SETTING_COMPLETION_INCLUDE_FAILURES = "completion_include_failures";
    SETTING_DELETE_TO_TRASH = "delete_to_trash";
    SETTING_AUTO_UPDATE_CHECK_ENABLED = "auto_update_check_enabled";
    SETTING_FFMPEG_PATH = "ffmpeg_path";
    /// F-7: Global BitTorrent upload speed limit (bytes/sec). Empty/0 = unlimited.
    SETTING_BT_UPLOAD_LIMIT_BPS = "bt_upload_limit_bps";
}

pub async fn get_settings(
    pool: &SqlitePool,
    default_save_dir: String,
) -> Result<AppSettings, String> {
    let kv = load_all_settings(pool).await?;
    settings_from_kv(kv, default_save_dir)
}

/// UX-41: derive `AppSettings` from a raw key/value snapshot. An empty `kv`
/// yields exactly the defaults a fresh install reads, so `reset_settings`
/// shares one source of truth with `get_settings` instead of the frontend
/// keeping a second, drift-prone copy of the default values.
fn settings_from_kv(
    kv: HashMap<String, String>,
    default_save_dir: String,
) -> Result<AppSettings, String> {
    let max_active_tasks = parse_i32_or_default(
        kv.get(SETTING_MAX_ACTIVE_TASKS).map(String::as_str),
        DEFAULT_MAX_ACTIVE_TASKS,
        MIN_MAX_ACTIVE_TASKS,
        MAX_MAX_ACTIVE_TASKS,
    )?;
    let default_save_dir = kv
        .get(SETTING_DEFAULT_SAVE_DIR)
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .unwrap_or(default_save_dir);
    let global_speed_limit_bps = normalize_speed_limit_bps(
        kv.get(SETTING_GLOBAL_SPEED_LIMIT_BPS)
            .map(String::as_str)
            .unwrap_or(""),
    );
    let multi_connection_threshold_bytes = kv
        .get(SETTING_MULTI_CONNECTION_THRESHOLD_BYTES)
        .and_then(|v| normalize_multi_connection_threshold_bytes(v))
        .unwrap_or_else(|| DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES.to_string());
    let segment_count = parse_i32_or_default(
        kv.get(SETTING_SEGMENT_COUNT).map(String::as_str),
        DEFAULT_SEGMENT_COUNT,
        MIN_SEGMENT_COUNT,
        MAX_SEGMENT_COUNT,
    )?;
    let max_connections_per_host = parse_i32_or_default(
        kv.get(SETTING_MAX_CONNECTIONS_PER_HOST).map(String::as_str),
        DEFAULT_MAX_CONNECTIONS_PER_HOST,
        MIN_MAX_CONNECTIONS_PER_HOST,
        MAX_MAX_CONNECTIONS_PER_HOST,
    )?;
    let system_notifications = parse_bool_setting(&kv, SETTING_SYSTEM_NOTIFICATIONS, true)?;
    let close_to_tray = parse_bool_setting(&kv, SETTING_CLOSE_TO_TRAY, false)?;
    let start_on_boot = parse_bool_setting(&kv, SETTING_START_ON_BOOT, false)?;
    // Resume only rows interrupted while actively downloading. Explicitly
    // paused rows keep their state, so the safe default can recover after a
    // crash or power loss without overriding a user's manual pause.
    let auto_resume_on_startup = parse_bool_setting(&kv, SETTING_AUTO_RESUME_ON_STARTUP, true)?;
    let floating_window_enabled = parse_bool_setting(&kv, SETTING_FLOATING_WINDOW_ENABLED, false)?;
    let clipboard_monitor_enabled =
        parse_bool_setting(&kv, SETTING_CLIPBOARD_MONITOR_ENABLED, true)?;
    let accent_color = kv
        .get(SETTING_ACCENT_COLOR)
        .map(|v| normalize_accent_color(v))
        .unwrap_or(AppAccentColor::Blue);
    let proxy_mode = kv
        .get(SETTING_PROXY_MODE)
        .map(|v| normalize_proxy_mode(v))
        .unwrap_or(AppProxyMode::Off);
    let proxy_url = kv
        .get(SETTING_PROXY_URL)
        .and_then(|v| normalize_proxy_url(v))
        .unwrap_or_default();
    let proxy_no_proxy = kv
        .get(SETTING_PROXY_NO_PROXY)
        .and_then(|v| normalize_proxy_no_proxy(v))
        .unwrap_or_default();
    let proxy_username = kv
        .get(SETTING_PROXY_USERNAME)
        .and_then(|v| normalize_proxy_optional(v))
        .unwrap_or_default();
    let proxy_password_saved = parse_bool_setting(&kv, SETTING_PROXY_PASSWORD_SAVED, false)?;
    let schedule_download_window_enabled =
        parse_bool_setting(&kv, SETTING_SCHEDULE_DOWNLOAD_WINDOW_ENABLED, false)?;
    let schedule_download_window_start = normalize_local_time(
        kv.get(SETTING_SCHEDULE_DOWNLOAD_WINDOW_START)
            .map(String::as_str)
            .unwrap_or(""),
    )
    .unwrap_or_else(|| "00:00".to_string());
    let schedule_download_window_end = normalize_local_time(
        kv.get(SETTING_SCHEDULE_DOWNLOAD_WINDOW_END)
            .map(String::as_str)
            .unwrap_or(""),
    )
    .unwrap_or_else(|| "06:00".to_string());
    let schedule_speed_limit_window_enabled =
        parse_bool_setting(&kv, SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_ENABLED, false)?;
    let schedule_speed_limit_window_start = normalize_local_time(
        kv.get(SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_START)
            .map(String::as_str)
            .unwrap_or(""),
    )
    .unwrap_or_else(|| "18:00".to_string());
    let schedule_speed_limit_window_end = normalize_local_time(
        kv.get(SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_END)
            .map(String::as_str)
            .unwrap_or(""),
    )
    .unwrap_or_else(|| "23:00".to_string());
    let schedule_speed_limit_bps = normalize_speed_limit_bps(
        kv.get(SETTING_SCHEDULE_SPEED_LIMIT_BPS)
            .map(String::as_str)
            .unwrap_or(""),
    );
    let titlebar_gradient_enabled =
        parse_bool_setting(&kv, SETTING_TITLEBAR_GRADIENT_ENABLED, false)?;
    let completion_action = kv
        .get(SETTING_COMPLETION_ACTION)
        .map(|v| CompletionAction::from_db_str(v))
        .unwrap_or(CompletionAction::None);
    let completion_countdown_seconds = parse_i32_or_default(
        kv.get(SETTING_COMPLETION_COUNTDOWN_SECONDS)
            .map(String::as_str),
        30,
        5,
        300,
    )?;
    let completion_run_command = kv
        .get(SETTING_COMPLETION_RUN_COMMAND)
        .cloned()
        .unwrap_or_default();
    // ARC-58: default false preserves the historical "all succeeded" behavior.
    let completion_include_failures =
        parse_bool_setting(&kv, SETTING_COMPLETION_INCLUDE_FAILURES, false)?;
    let delete_to_trash = parse_bool_setting(&kv, SETTING_DELETE_TO_TRASH, true)?;
    let auto_update_check_enabled =
        parse_bool_setting(&kv, SETTING_AUTO_UPDATE_CHECK_ENABLED, true)?;
    let ffmpeg_path = kv
        .get(SETTING_FFMPEG_PATH)
        .and_then(|v| normalize_ffmpeg_path(v));
    // F-7: BT upload limit, persisted as a speed-limit string (same shape as
    // global_speed_limit_bps). Empty/invalid/0 means unlimited.
    let bt_upload_limit_bps = normalize_speed_limit_bps(
        kv.get(SETTING_BT_UPLOAD_LIMIT_BPS)
            .map(String::as_str)
            .unwrap_or(""),
    );

    Ok(AppSettings {
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
        completion_include_failures,
        delete_to_trash,
        auto_update_check_enabled,
        ffmpeg_path,
        bt_upload_limit_bps,
    })
}

pub async fn upsert_settings(pool: &SqlitePool, settings: &AppSettings) -> Result<(), String> {
    let settings = AppSettings {
        max_active_tasks: settings
            .max_active_tasks
            .clamp(MIN_MAX_ACTIVE_TASKS, MAX_MAX_ACTIVE_TASKS),
        segment_count: settings
            .segment_count
            .clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT),
        max_connections_per_host: settings
            .max_connections_per_host
            .clamp(MIN_MAX_CONNECTIONS_PER_HOST, MAX_MAX_CONNECTIONS_PER_HOST),
        ..settings.clone()
    };
    upsert_setting_value(
        pool,
        SETTING_MAX_ACTIVE_TASKS,
        &settings.max_active_tasks.to_string(),
    )
    .await?;
    upsert_setting_value(pool, SETTING_DEFAULT_SAVE_DIR, &settings.default_save_dir).await?;
    upsert_setting_value(
        pool,
        SETTING_GLOBAL_SPEED_LIMIT_BPS,
        settings.global_speed_limit_bps.as_deref().unwrap_or(""),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_MULTI_CONNECTION_THRESHOLD_BYTES,
        &settings.multi_connection_threshold_bytes,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SEGMENT_COUNT,
        &settings.segment_count.to_string(),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_MAX_CONNECTIONS_PER_HOST,
        &settings.max_connections_per_host.to_string(),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SYSTEM_NOTIFICATIONS,
        bool_setting_value(settings.system_notifications),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_CLOSE_TO_TRAY,
        bool_setting_value(settings.close_to_tray),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_START_ON_BOOT,
        bool_setting_value(settings.start_on_boot),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_AUTO_RESUME_ON_STARTUP,
        bool_setting_value(settings.auto_resume_on_startup),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_FLOATING_WINDOW_ENABLED,
        bool_setting_value(settings.floating_window_enabled),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_CLIPBOARD_MONITOR_ENABLED,
        bool_setting_value(settings.clipboard_monitor_enabled),
    )
    .await?;
    upsert_setting_value(pool, SETTING_ACCENT_COLOR, settings.accent_color.as_str()).await?;
    upsert_setting_value(pool, SETTING_PROXY_MODE, settings.proxy_mode.as_str()).await?;
    upsert_setting_value(pool, SETTING_PROXY_URL, &settings.proxy_url).await?;
    upsert_setting_value(pool, SETTING_PROXY_NO_PROXY, &settings.proxy_no_proxy).await?;
    upsert_setting_value(pool, SETTING_PROXY_USERNAME, &settings.proxy_username).await?;
    upsert_setting_value(
        pool,
        SETTING_PROXY_PASSWORD_SAVED,
        bool_setting_value(settings.proxy_password_saved),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_DOWNLOAD_WINDOW_ENABLED,
        bool_setting_value(settings.schedule_download_window_enabled),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_DOWNLOAD_WINDOW_START,
        &settings.schedule_download_window_start,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_DOWNLOAD_WINDOW_END,
        &settings.schedule_download_window_end,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_ENABLED,
        bool_setting_value(settings.schedule_speed_limit_window_enabled),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_START,
        &settings.schedule_speed_limit_window_start,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_END,
        &settings.schedule_speed_limit_window_end,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_SCHEDULE_SPEED_LIMIT_BPS,
        settings.schedule_speed_limit_bps.as_deref().unwrap_or(""),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_TITLEBAR_GRADIENT_ENABLED,
        bool_setting_value(settings.titlebar_gradient_enabled),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_COMPLETION_ACTION,
        settings.completion_action.as_str(),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_COMPLETION_COUNTDOWN_SECONDS,
        &settings.completion_countdown_seconds.to_string(),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_COMPLETION_RUN_COMMAND,
        &settings.completion_run_command,
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_COMPLETION_INCLUDE_FAILURES,
        bool_setting_value(settings.completion_include_failures),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_DELETE_TO_TRASH,
        bool_setting_value(settings.delete_to_trash),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_AUTO_UPDATE_CHECK_ENABLED,
        bool_setting_value(settings.auto_update_check_enabled),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_FFMPEG_PATH,
        settings.ffmpeg_path.as_deref().unwrap_or(""),
    )
    .await?;
    upsert_setting_value(
        pool,
        SETTING_BT_UPLOAD_LIMIT_BPS,
        settings.bt_upload_limit_bps.as_deref().unwrap_or(""),
    )
    .await
}

/// Read the persisted `ffmpeg_path` setting without loading the full AppSettings.
/// Used by the download layer to resolve the ffmpeg binary location.
pub async fn get_ffmpeg_path_setting(pool: &SqlitePool) -> Option<String> {
    let kv = load_all_settings(pool).await.ok()?;
    kv.get(SETTING_FFMPEG_PATH)
        .and_then(|v| normalize_ffmpeg_path(v))
}

/// F-7: Read the persisted BT upload limit (bytes/sec) without loading the
/// full AppSettings. Used by the BT engine to apply the limit at session
/// creation and on reuse. Returns `None` when unset/invalid/0 (unlimited).
pub async fn get_bt_upload_limit_bps_setting(pool: &SqlitePool) -> Option<i64> {
    let kv = load_all_settings(pool).await.ok()?;
    parse_speed_limit_bps(kv.get(SETTING_BT_UPLOAD_LIMIT_BPS).map(String::as_str))
}

/// Normalize a user-provided ffmpeg path: trim whitespace and reject empty
/// strings. Does not check filesystem existence to avoid cross-platform
/// path-canonicalization issues during settings updates.
pub fn normalize_ffmpeg_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn clipboard_monitor_enabled(pool: &SqlitePool) -> Result<bool, String> {
    let kv = load_all_settings(pool).await?;
    Ok(kv_bool(&kv, SETTING_CLIPBOARD_MONITOR_ENABLED, true))
}

pub async fn delete_to_trash_enabled(pool: &SqlitePool) -> Result<bool, String> {
    let kv = load_all_settings(pool).await?;
    Ok(kv_bool(&kv, SETTING_DELETE_TO_TRASH, true))
}

pub fn normalize_accent_color(value: &str) -> AppAccentColor {
    match value.trim() {
        "blue" => AppAccentColor::Blue,
        "purple" => AppAccentColor::Purple,
        "teal" => AppAccentColor::Teal,
        "green" => AppAccentColor::Green,
        "orange" => AppAccentColor::Orange,
        "rose" => AppAccentColor::Rose,
        "indigo" => AppAccentColor::Indigo,
        "amber" => AppAccentColor::Amber,
        _ => AppAccentColor::Blue,
    }
}

pub fn normalize_proxy_mode(value: &str) -> AppProxyMode {
    proxy::normalize_proxy_mode(value)
}

pub fn normalize_proxy_url(value: &str) -> Option<String> {
    proxy::normalize_proxy_url(value)
}

pub fn normalize_proxy_no_proxy(value: &str) -> Option<String> {
    proxy::normalize_proxy_no_proxy(value)
}

pub fn normalize_proxy_optional(value: &str) -> Option<String> {
    proxy::normalize_proxy_optional(value)
}

pub fn normalize_speed_limit_bps(value: &str) -> Option<String> {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|limit| *limit > 0)
        .map(|limit| limit.to_string())
}

pub fn parse_speed_limit_bps(value: Option<&str>) -> Option<i64> {
    value
        .and_then(normalize_speed_limit_bps)
        .and_then(|value| value.parse::<i64>().ok())
}

pub fn normalize_local_time(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let (hour, minute) = trimmed.split_once(':')?;
    let hour = hour.parse::<u32>().ok()?;
    let minute = minute.parse::<u32>().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(format!("{hour:02}:{minute:02}"))
}

pub fn local_time_window_active(start: &str, end: &str) -> bool {
    let Some(start_minutes) = local_minutes(start) else {
        return false;
    };
    let Some(end_minutes) = local_minutes(end) else {
        return false;
    };
    if start_minutes == end_minutes {
        return true;
    }
    let now = chrono::Local::now();
    let current = now.hour() * 60 + now.minute();
    if start_minutes < end_minutes {
        current >= start_minutes && current < end_minutes
    } else {
        current >= start_minutes || current < end_minutes
    }
}

/// E-5: Returns the duration until the next schedule window boundary
/// (either start or end, whichever comes first). Used by the schedule
/// monitor to sleep until the exact boundary instead of polling every 60s,
/// eliminating up to 60s latency at boundary crossings.
///
/// Returns a 60s fallback if the window strings are unparseable, and a 1h
/// cap for safety (settings changes, clock adjustments, DST transitions).
pub fn duration_until_next_window_boundary(start: &str, end: &str) -> std::time::Duration {
    let Some(start_minutes) = local_minutes(start) else {
        return std::time::Duration::from_secs(60);
    };
    let Some(end_minutes) = local_minutes(end) else {
        return std::time::Duration::from_secs(60);
    };
    if start_minutes == end_minutes {
        // Window is always active — no boundary to wait for.
        return std::time::Duration::from_secs(3600);
    }
    let now = chrono::Local::now();
    let current_secs = (now.hour() * 60 + now.minute()) * 60 + now.second();
    let start_secs = start_minutes * 60;
    let end_secs = end_minutes * 60;

    // Seconds until next start (wraps around midnight).
    let until_start = if current_secs < start_secs {
        start_secs - current_secs
    } else {
        86400 - current_secs + start_secs
    };
    // Seconds until next end (wraps around midnight).
    let until_end = if current_secs < end_secs {
        end_secs - current_secs
    } else {
        86400 - current_secs + end_secs
    };

    let next = until_start.min(until_end).min(3600); // cap at 1h
    std::time::Duration::from_secs(next as u64)
}

fn local_minutes(value: &str) -> Option<u32> {
    let normalized = normalize_local_time(value)?;
    let (hour, minute) = normalized.split_once(':')?;
    Some(hour.parse::<u32>().ok()? * 60 + minute.parse::<u32>().ok()?)
}

pub fn normalize_multi_connection_threshold_bytes(value: &str) -> Option<String> {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .map(|limit| {
            limit.clamp(
                MIN_MULTI_CONNECTION_THRESHOLD_BYTES,
                MAX_MULTI_CONNECTION_THRESHOLD_BYTES,
            )
        })
        .map(|limit| limit.to_string())
}

pub fn parse_multi_connection_threshold_bytes(value: &str) -> i64 {
    normalize_multi_connection_threshold_bytes(value)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES)
}

async fn load_all_settings(pool: &SqlitePool) -> Result<HashMap<String, String>, String> {
    let rows = sqlx::query("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut map = HashMap::with_capacity(rows.len());
    for row in rows {
        let key: String = row.get("key");
        let value: String = row.get("value");
        map.insert(key, value);
    }
    Ok(map)
}

/// UX-41: restore app settings to the fresh-install defaults by deleting the
/// owned keys; `settings_from_kv` then re-derives the same values a brand-new
/// database would produce.
pub async fn reset_settings(
    pool: &SqlitePool,
    default_save_dir: String,
) -> Result<AppSettings, String> {
    let placeholders = vec!["?"; APP_SETTING_KEYS.len()].join(", ");
    // Injection-safe: the string is built from a fixed count of `?`
    // placeholders derived from the const key array; no user input is
    // interpolated. All keys are bound as parameters below.
    let mut query = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM settings WHERE key IN ({placeholders})"
    )));
    for key in APP_SETTING_KEYS {
        query = query.bind(*key);
    }
    query.execute(pool).await.map_err(|e| e.to_string())?;
    get_settings(pool, default_save_dir).await
}

fn kv_bool(kv: &HashMap<String, String>, key: &str, default: bool) -> bool {
    kv.get(key)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn bool_setting_value(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

async fn upsert_setting_value(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO settings (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn parse_bool_setting(
    kv: &HashMap<String, String>,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    match kv.get(key).map(|value| value.trim().to_ascii_lowercase()) {
        None => Ok(default),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => Ok(true),
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => Ok(false),
        Some(value) => Err(format!(
            "Invalid boolean value for setting '{key}': {value}"
        )),
    }
}

fn parse_i32_or_default(
    value: Option<&str>,
    default: i32,
    min: i32,
    max: i32,
) -> Result<i32, String> {
    let parsed = match value {
        Some(raw) if !raw.trim().is_empty() => raw
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("Invalid integer value: {raw}"))?,
        _ => default,
    };
    Ok(parsed.clamp(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool_setting_accepts_known_values() {
        let mut kv = HashMap::new();
        kv.insert("flag".to_string(), " yes ".to_string());
        assert!(parse_bool_setting(&kv, "flag", false).expect("parse bool"));

        kv.insert("flag".to_string(), " off ".to_string());
        assert!(!parse_bool_setting(&kv, "flag", true).expect("parse bool"));
    }

    #[test]
    fn parse_bool_setting_rejects_unknown_values() {
        let mut kv = HashMap::new();
        kv.insert("flag".to_string(), "maybe".to_string());
        let err = parse_bool_setting(&kv, "flag", false).expect_err("invalid bool");
        assert!(err.contains("Invalid boolean value for setting 'flag': maybe"));
    }

    #[test]
    fn parse_i32_or_default_clamps_and_rejects_invalid_values() {
        assert_eq!(
            parse_i32_or_default(Some(" 9 "), 4, 1, 8).expect("parse int"),
            8
        );
        assert_eq!(parse_i32_or_default(None, 4, 1, 8).expect("default"), 4);
        let err = parse_i32_or_default(Some("oops"), 4, 1, 8).expect_err("invalid int");
        assert!(err.contains("Invalid integer value: oops"));
    }

    #[tokio::test]
    async fn get_settings_rejects_invalid_persisted_values() {
        let pool = temp_pool().await;
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
            .bind(SETTING_CLIPBOARD_MONITOR_ENABLED)
            .bind("maybe")
            .execute(&pool)
            .await
            .expect("insert bool");
        let err = get_settings(&pool, "C:\\Downloads".to_string())
            .await
            .expect_err("invalid bool");
        assert!(err.contains("Invalid boolean value for setting 'clipboard_monitor_enabled'"));
    }

    /// UX-41 acceptance: after reset, the settings equal what a brand-new
    /// database produces — every field, not just the ones the frontend used to
    /// hardcode. Also asserts unrelated keys sharing the table survive.
    #[tokio::test]
    async fn reset_settings_matches_fresh_database() {
        let pool = temp_pool().await;
        // Pollute a representative set of owned keys (valid, non-default
        // values); key-list completeness is structural via app_setting_keys!.
        let polluted = [
            (SETTING_MAX_ACTIVE_TASKS, "7"),
            (SETTING_DEFAULT_SAVE_DIR, "D:\\Somewhere"),
            (SETTING_GLOBAL_SPEED_LIMIT_BPS, "1024"),
            (SETTING_MULTI_CONNECTION_THRESHOLD_BYTES, "1048576"),
            (SETTING_SEGMENT_COUNT, "8"),
            (SETTING_MAX_CONNECTIONS_PER_HOST, "16"),
            (SETTING_CLIPBOARD_MONITOR_ENABLED, "false"),
            (SETTING_ACCENT_COLOR, "amber"),
            (SETTING_PROXY_MODE, "custom"),
            (SETTING_PROXY_URL, "http://127.0.0.1:8080"),
            (SETTING_COMPLETION_ACTION, "exit"),
            (SETTING_COMPLETION_COUNTDOWN_SECONDS, "120"),
            (SETTING_FFMPEG_PATH, "D:\\ffmpeg\\bin\\ffmpeg.exe"),
            (SETTING_BT_UPLOAD_LIMIT_BPS, "2048"),
        ];
        for (key, value) in polluted {
            upsert_setting_value(&pool, key, value)
                .await
                .expect("pollute setting");
        }
        // An unrelated key sharing the table must survive the reset.
        upsert_setting_value(&pool, "browser_capture_settings", "{\"keep\":true}")
            .await
            .expect("insert unrelated key");

        let reset = reset_settings(&pool, "C:\\Downloads".to_string())
            .await
            .expect("reset settings");
        let fresh_pool = temp_pool().await;
        let fresh = get_settings(&fresh_pool, "C:\\Downloads".to_string())
            .await
            .expect("fresh settings");

        let reset_json = serde_json::to_value(&reset).expect("serialize reset");
        let fresh_json = serde_json::to_value(&fresh).expect("serialize fresh");
        assert_eq!(
            reset_json, fresh_json,
            "reset settings must equal a fresh database's settings"
        );
        // The specific field the frontend copy had drifted on.
        assert_eq!(
            reset.multi_connection_threshold_bytes,
            DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES.to_string()
        );
        let unrelated: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'browser_capture_settings'")
                .fetch_one(&pool)
                .await
                .expect("unrelated key must survive reset");
        assert_eq!(unrelated, "{\"keep\":true}");
    }

    /// UX-41: the reset list is generated together with the constants, so
    /// the remaining failure mode is two constants aliasing one row (reset
    /// would then leave the other field's intended key behind).
    #[test]
    fn app_setting_keys_are_unique() {
        let unique: std::collections::HashSet<&str> = APP_SETTING_KEYS.iter().copied().collect();
        assert_eq!(unique.len(), APP_SETTING_KEYS.len());
        assert!(APP_SETTING_KEYS.contains(&SETTING_COMPLETION_INCLUDE_FAILURES));
    }

    // ENG-04: these two decide what a user-typed speed limit / threshold
    // becomes. The clamp bounds are the product contract, and neither function
    // had direct coverage.
    #[test]
    fn normalize_speed_limit_bps_rejects_non_positive_and_junk() {
        assert_eq!(normalize_speed_limit_bps("1024"), Some("1024".to_string()));
        assert_eq!(
            normalize_speed_limit_bps(" 2048 "),
            Some("2048".to_string())
        );
        // Zero and negatives are how the UI expresses "no limit"; they must not
        // silently become a hard "0 bytes per second" limit.
        assert_eq!(normalize_speed_limit_bps("0"), None);
        assert_eq!(normalize_speed_limit_bps("-1"), None);
        assert_eq!(normalize_speed_limit_bps("1.5"), None);
        assert_eq!(normalize_speed_limit_bps("abc"), None);
        assert_eq!(normalize_speed_limit_bps(""), None);
    }

    #[test]
    fn normalize_multi_connection_threshold_clamps_to_declared_bounds() {
        assert_eq!(
            normalize_multi_connection_threshold_bytes("-5"),
            Some(MIN_MULTI_CONNECTION_THRESHOLD_BYTES.to_string())
        );
        assert_eq!(
            normalize_multi_connection_threshold_bytes("999999999999999999"),
            Some(MAX_MULTI_CONNECTION_THRESHOLD_BYTES.to_string())
        );
        assert_eq!(
            normalize_multi_connection_threshold_bytes("1048576"),
            Some("1048576".to_string())
        );
        assert_eq!(normalize_multi_connection_threshold_bytes("junk"), None);
    }

    async fn temp_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        sqlx::query(
            r#"
            CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("settings table");
        pool
    }

    // E-5: duration_until_next_window_boundary tests

    #[test]
    fn duration_until_next_window_boundary_is_bounded() {
        // Any valid window should return a duration between 1s and 3600s.
        let dur = duration_until_next_window_boundary("09:00", "17:00");
        assert!(dur.as_secs() > 0, "duration must be positive");
        assert!(
            dur.as_secs() <= 3600,
            "duration must be capped at 1h, got {}s",
            dur.as_secs()
        );
    }

    #[test]
    fn duration_until_next_window_boundary_always_active() {
        // start == end means the window is always active — no boundary.
        let dur = duration_until_next_window_boundary("00:00", "00:00");
        assert_eq!(dur.as_secs(), 3600);
    }

    #[test]
    fn duration_until_next_window_boundary_wrap_around() {
        // Window crossing midnight (22:00-06:00) — should still return
        // a bounded duration.
        let dur = duration_until_next_window_boundary("22:00", "06:00");
        assert!(dur.as_secs() > 0);
        assert!(dur.as_secs() <= 3600);
    }

    #[test]
    fn duration_until_next_window_boundary_invalid_input() {
        // Unparseable window strings — should return 60s fallback.
        assert_eq!(
            duration_until_next_window_boundary("invalid", "17:00").as_secs(),
            60
        );
        assert_eq!(
            duration_until_next_window_boundary("09:00", "broken").as_secs(),
            60
        );
    }
}
