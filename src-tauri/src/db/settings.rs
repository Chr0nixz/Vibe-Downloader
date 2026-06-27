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

const SETTING_MAX_ACTIVE_TASKS: &str = "max_active_tasks";
const SETTING_DEFAULT_SAVE_DIR: &str = "default_save_dir";
const SETTING_GLOBAL_SPEED_LIMIT_BPS: &str = "global_speed_limit_bps";
const SETTING_MULTI_CONNECTION_THRESHOLD_BYTES: &str = "multi_connection_threshold_bytes";
const SETTING_SEGMENT_COUNT: &str = "segment_count";
const SETTING_MAX_CONNECTIONS_PER_HOST: &str = "max_connections_per_host";
const SETTING_SYSTEM_NOTIFICATIONS: &str = "system_notifications";
const SETTING_CLOSE_TO_TRAY: &str = "close_to_tray";
const SETTING_START_ON_BOOT: &str = "start_on_boot";
const SETTING_AUTO_RESUME_ON_STARTUP: &str = "auto_resume_on_startup";
const SETTING_FLOATING_WINDOW_ENABLED: &str = "floating_window_enabled";
const SETTING_CLIPBOARD_MONITOR_ENABLED: &str = "clipboard_monitor_enabled";
const SETTING_ACCENT_COLOR: &str = "accent_color";
const SETTING_PROXY_MODE: &str = "proxy_mode";
const SETTING_PROXY_URL: &str = "proxy_url";
const SETTING_PROXY_NO_PROXY: &str = "proxy_no_proxy";
const SETTING_PROXY_USERNAME: &str = "proxy_username";
const SETTING_PROXY_PASSWORD_SAVED: &str = "proxy_password_saved";
const SETTING_SCHEDULE_DOWNLOAD_WINDOW_ENABLED: &str = "schedule_download_window_enabled";
const SETTING_SCHEDULE_DOWNLOAD_WINDOW_START: &str = "schedule_download_window_start";
const SETTING_SCHEDULE_DOWNLOAD_WINDOW_END: &str = "schedule_download_window_end";
const SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_ENABLED: &str = "schedule_speed_limit_window_enabled";
const SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_START: &str = "schedule_speed_limit_window_start";
const SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_END: &str = "schedule_speed_limit_window_end";
const SETTING_SCHEDULE_SPEED_LIMIT_BPS: &str = "schedule_speed_limit_bps";
const SETTING_TITLEBAR_GRADIENT_ENABLED: &str = "titlebar_gradient_enabled";
const SETTING_COMPLETION_ACTION: &str = "completion_action";
const SETTING_COMPLETION_COUNTDOWN_SECONDS: &str = "completion_countdown_seconds";
const SETTING_COMPLETION_RUN_COMMAND: &str = "completion_run_command";
const SETTING_DELETE_TO_TRASH: &str = "delete_to_trash";

pub async fn get_settings(
    pool: &SqlitePool,
    default_save_dir: String,
) -> Result<AppSettings, String> {
    let kv = load_all_settings(pool).await?;

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
    let global_speed_limit_bps =
        normalize_speed_limit_bps(kv.get(SETTING_GLOBAL_SPEED_LIMIT_BPS).map(String::as_str).unwrap_or(""));
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
    let auto_resume_on_startup = parse_bool_setting(&kv, SETTING_AUTO_RESUME_ON_STARTUP, false)?;
    let floating_window_enabled = parse_bool_setting(&kv, SETTING_FLOATING_WINDOW_ENABLED, false)?;
    let clipboard_monitor_enabled = parse_bool_setting(&kv, SETTING_CLIPBOARD_MONITOR_ENABLED, true)?;
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
        kv.get(SETTING_SCHEDULE_DOWNLOAD_WINDOW_START).map(String::as_str).unwrap_or(""),
    )
    .unwrap_or_else(|| "00:00".to_string());
    let schedule_download_window_end = normalize_local_time(
        kv.get(SETTING_SCHEDULE_DOWNLOAD_WINDOW_END).map(String::as_str).unwrap_or(""),
    )
    .unwrap_or_else(|| "06:00".to_string());
    let schedule_speed_limit_window_enabled =
        parse_bool_setting(&kv, SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_ENABLED, false)?;
    let schedule_speed_limit_window_start = normalize_local_time(
        kv.get(SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_START).map(String::as_str).unwrap_or(""),
    )
    .unwrap_or_else(|| "18:00".to_string());
    let schedule_speed_limit_window_end = normalize_local_time(
        kv.get(SETTING_SCHEDULE_SPEED_LIMIT_WINDOW_END).map(String::as_str).unwrap_or(""),
    )
    .unwrap_or_else(|| "23:00".to_string());
    let schedule_speed_limit_bps =
        normalize_speed_limit_bps(kv.get(SETTING_SCHEDULE_SPEED_LIMIT_BPS).map(String::as_str).unwrap_or(""));
    let titlebar_gradient_enabled = parse_bool_setting(&kv, SETTING_TITLEBAR_GRADIENT_ENABLED, true)?;
    let completion_action = kv
        .get(SETTING_COMPLETION_ACTION)
        .map(|v| CompletionAction::from_db_str(v))
        .unwrap_or(CompletionAction::None);
    let completion_countdown_seconds = parse_i32_or_default(
        kv.get(SETTING_COMPLETION_COUNTDOWN_SECONDS).map(String::as_str),
        30,
        5,
        300,
    )?;
    let completion_run_command = kv
        .get(SETTING_COMPLETION_RUN_COMMAND)
        .cloned()
        .unwrap_or_default();
    let delete_to_trash = parse_bool_setting(&kv, SETTING_DELETE_TO_TRASH, true)?;

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
        delete_to_trash,
    })
}

pub async fn upsert_settings(pool: &SqlitePool, settings: &AppSettings) -> Result<(), String> {
    validate_settings(settings)?;
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
        SETTING_DELETE_TO_TRASH,
        bool_setting_value(settings.delete_to_trash),
    )
    .await
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

fn kv_bool(kv: &HashMap<String, String>, key: &str, default: bool) -> bool {
    kv.get(key)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
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
        Some(value) => Err(format!("Invalid boolean value for setting '{key}': {value}")),
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

fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    if settings.max_active_tasks < MIN_MAX_ACTIVE_TASKS || settings.max_active_tasks > MAX_MAX_ACTIVE_TASKS {
        return Err("max_active_tasks is out of range.".to_string());
    }
    if settings.segment_count < MIN_SEGMENT_COUNT || settings.segment_count > MAX_SEGMENT_COUNT {
        return Err("segment_count is out of range.".to_string());
    }
    if settings.max_connections_per_host < MIN_MAX_CONNECTIONS_PER_HOST
        || settings.max_connections_per_host > MAX_MAX_CONNECTIONS_PER_HOST
    {
        return Err("max_connections_per_host is out of range.".to_string());
    }
    if settings.completion_countdown_seconds < 5 || settings.completion_countdown_seconds > 300 {
        return Err("completion_countdown_seconds must be between 5 and 300.".to_string());
    }
    Ok(())
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

    #[test]
    fn validate_settings_rejects_out_of_range_fields() {
        let settings = AppSettings {
            max_active_tasks: 0,
            default_save_dir: String::new(),
            global_speed_limit_bps: None,
            multi_connection_threshold_bytes: "0".to_string(),
            segment_count: 1,
            max_connections_per_host: 1,
            system_notifications: true,
            close_to_tray: false,
            start_on_boot: false,
            auto_resume_on_startup: false,
            floating_window_enabled: false,
            clipboard_monitor_enabled: true,
            accent_color: AppAccentColor::Blue,
            proxy_mode: AppProxyMode::Off,
            proxy_url: String::new(),
            proxy_no_proxy: String::new(),
            proxy_username: String::new(),
            proxy_password_saved: false,
            schedule_download_window_enabled: false,
            schedule_download_window_start: "00:00".to_string(),
            schedule_download_window_end: "06:00".to_string(),
            schedule_speed_limit_window_enabled: false,
            schedule_speed_limit_window_start: "18:00".to_string(),
            schedule_speed_limit_window_end: "23:00".to_string(),
            schedule_speed_limit_bps: None,
            titlebar_gradient_enabled: true,
            completion_action: CompletionAction::None,
            completion_countdown_seconds: 30,
            completion_run_command: String::new(),
            delete_to_trash: true,
        };

        let err = validate_settings(&settings).expect_err("invalid settings");
        assert!(err.contains("max_active_tasks is out of range"));
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
}
