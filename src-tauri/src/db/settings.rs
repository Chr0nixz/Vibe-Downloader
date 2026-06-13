use sqlx::{Row, SqlitePool};

use crate::models::{AppAccentColor, AppFontFamily, AppSettings};
use crate::proxy::{self, AppProxyMode};

use super::{
    DEFAULT_MAX_ACTIVE_TASKS, DEFAULT_MAX_CONNECTIONS_PER_HOST,
    DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES, DEFAULT_SEGMENT_COUNT, MAX_MAX_ACTIVE_TASKS,
    MAX_MAX_CONNECTIONS_PER_HOST, MAX_MULTI_CONNECTION_THRESHOLD_BYTES, MAX_SEGMENT_COUNT,
    MIN_MAX_ACTIVE_TASKS, MIN_MAX_CONNECTIONS_PER_HOST, MIN_MULTI_CONNECTION_THRESHOLD_BYTES,
    MIN_SEGMENT_COUNT,
};

const SETTING_MAX_ACTIVE_TASKS: &str = "max_active_tasks";
const SETTING_DEFAULT_SAVE_DIR: &str = "default_save_dir";
const SETTING_GLOBAL_SPEED_LIMIT_BPS: &str = "global_speed_limit_bps";
const SETTING_MULTI_CONNECTION_THRESHOLD_BYTES: &str = "multi_connection_threshold_bytes";
const SETTING_SEGMENT_COUNT: &str = "segment_count";
const SETTING_MAX_CONNECTIONS_PER_HOST: &str = "max_connections_per_host";
const SETTING_SYSTEM_NOTIFICATIONS: &str = "system_notifications";
const SETTING_CLOSE_TO_TRAY: &str = "close_to_tray";
const SETTING_START_ON_BOOT: &str = "start_on_boot";
const SETTING_FLOATING_WINDOW_ENABLED: &str = "floating_window_enabled";
const SETTING_CLIPBOARD_MONITOR_ENABLED: &str = "clipboard_monitor_enabled";
const SETTING_FONT_FAMILY: &str = "font_family";
const SETTING_ACCENT_COLOR: &str = "accent_color";
const SETTING_PROXY_MODE: &str = "proxy_mode";
const SETTING_PROXY_URL: &str = "proxy_url";
const SETTING_PROXY_NO_PROXY: &str = "proxy_no_proxy";
const SETTING_PROXY_USERNAME: &str = "proxy_username";
const SETTING_PROXY_PASSWORD_SAVED: &str = "proxy_password_saved";

pub async fn get_settings(
    pool: &SqlitePool,
    default_save_dir: String,
) -> Result<AppSettings, String> {
    let max_active_tasks = get_setting_value(pool, SETTING_MAX_ACTIVE_TASKS)
        .await?
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_MAX_ACTIVE_TASKS)
        .clamp(MIN_MAX_ACTIVE_TASKS, MAX_MAX_ACTIVE_TASKS);
    let default_save_dir = get_setting_value(pool, SETTING_DEFAULT_SAVE_DIR)
        .await?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_save_dir);
    let global_speed_limit_bps = get_setting_value(pool, SETTING_GLOBAL_SPEED_LIMIT_BPS)
        .await?
        .and_then(|value| normalize_speed_limit_bps(&value));
    let multi_connection_threshold_bytes =
        get_setting_value(pool, SETTING_MULTI_CONNECTION_THRESHOLD_BYTES)
            .await?
            .and_then(|value| normalize_multi_connection_threshold_bytes(&value))
            .unwrap_or_else(|| DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES.to_string());
    let segment_count = get_setting_value(pool, SETTING_SEGMENT_COUNT)
        .await?
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_SEGMENT_COUNT)
        .clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT);
    let max_connections_per_host = get_setting_value(pool, SETTING_MAX_CONNECTIONS_PER_HOST)
        .await?
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_MAX_CONNECTIONS_PER_HOST)
        .clamp(MIN_MAX_CONNECTIONS_PER_HOST, MAX_MAX_CONNECTIONS_PER_HOST);
    let system_notifications = get_bool_setting(pool, SETTING_SYSTEM_NOTIFICATIONS, true).await?;
    let close_to_tray = get_bool_setting(pool, SETTING_CLOSE_TO_TRAY, false).await?;
    let start_on_boot = get_bool_setting(pool, SETTING_START_ON_BOOT, false).await?;
    let floating_window_enabled =
        get_bool_setting(pool, SETTING_FLOATING_WINDOW_ENABLED, false).await?;
    let clipboard_monitor_enabled =
        get_bool_setting(pool, SETTING_CLIPBOARD_MONITOR_ENABLED, true).await?;
    let font_family = get_setting_value(pool, SETTING_FONT_FAMILY)
        .await?
        .map(|value| normalize_font_family(&value))
        .unwrap_or(AppFontFamily::SourceHanSansSc);
    let accent_color = get_setting_value(pool, SETTING_ACCENT_COLOR)
        .await?
        .map(|value| normalize_accent_color(&value))
        .unwrap_or(AppAccentColor::Blue);
    let proxy_mode = get_setting_value(pool, SETTING_PROXY_MODE)
        .await?
        .map(|value| normalize_proxy_mode(&value))
        .unwrap_or(AppProxyMode::Off);
    let proxy_url = get_setting_value(pool, SETTING_PROXY_URL)
        .await?
        .and_then(|value| normalize_proxy_url(&value))
        .unwrap_or_default();
    let proxy_no_proxy = get_setting_value(pool, SETTING_PROXY_NO_PROXY)
        .await?
        .and_then(|value| normalize_proxy_no_proxy(&value))
        .unwrap_or_default();
    let proxy_username = get_setting_value(pool, SETTING_PROXY_USERNAME)
        .await?
        .and_then(|value| normalize_proxy_optional(&value))
        .unwrap_or_default();
    let proxy_password_saved = get_bool_setting(pool, SETTING_PROXY_PASSWORD_SAVED, false).await?;

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
        floating_window_enabled,
        clipboard_monitor_enabled,
        font_family,
        accent_color,
        proxy_mode,
        proxy_url,
        proxy_no_proxy,
        proxy_username,
        proxy_password_saved,
    })
}

pub async fn upsert_settings(pool: &SqlitePool, settings: &AppSettings) -> Result<(), String> {
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
    upsert_setting_value(pool, SETTING_FONT_FAMILY, settings.font_family.as_str()).await?;
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
    .await
}

pub async fn clipboard_monitor_enabled(pool: &SqlitePool) -> Result<bool, String> {
    get_bool_setting(pool, SETTING_CLIPBOARD_MONITOR_ENABLED, true).await
}

pub fn normalize_font_family(value: &str) -> AppFontFamily {
    match value.trim() {
        "system" => AppFontFamily::System,
        "source_han_sans_sc" => AppFontFamily::SourceHanSansSc,
        _ => AppFontFamily::SourceHanSansSc,
    }
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

async fn get_setting_value(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(row.map(|row| row.get("value")))
}

async fn get_bool_setting(pool: &SqlitePool, key: &str, default: bool) -> Result<bool, String> {
    Ok(get_setting_value(pool, key)
        .await?
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default))
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
