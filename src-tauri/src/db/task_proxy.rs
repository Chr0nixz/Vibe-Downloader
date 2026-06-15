use reqwest::Url;
use sqlx::{Row, SqlitePool};

use crate::{
    models::{task::now_iso, TaskProxyMode, TaskProxySettings, TaskProxySettingsInput},
    proxy::{self, AppProxyMode, ResolvedProxyConfig},
};

pub async fn get_task_proxy_settings(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<TaskProxySettings, String> {
    let row = sqlx::query(
        r#"
        SELECT task_id, mode, proxy_url, proxy_username, proxy_password_ciphertext, no_proxy
        FROM task_proxy_settings
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row
        .map(row_to_task_proxy_settings)
        .unwrap_or_else(|| TaskProxySettings {
            task_id: task_id.to_string(),
            mode: TaskProxyMode::Inherit,
            proxy_url: String::new(),
            proxy_username: String::new(),
            proxy_password_saved: false,
            no_proxy: String::new(),
        }))
}

pub async fn upsert_task_proxy_settings(
    pool: &SqlitePool,
    input: TaskProxySettingsInput,
) -> Result<TaskProxySettings, String> {
    let now = now_iso();
    let current = load_task_proxy_secret_record(pool, &input.task_id).await?;
    let mode = input.mode;
    let proxy_url = input
        .proxy_url
        .as_deref()
        .and_then(proxy::normalize_proxy_url)
        .unwrap_or_default();
    let proxy_username = input
        .proxy_username
        .as_deref()
        .and_then(proxy::normalize_proxy_optional)
        .unwrap_or_default();
    proxy::validate_proxy_settings(proxy_mode_for_task(mode), &proxy_url, &proxy_username)?;
    let no_proxy = input
        .no_proxy
        .as_deref()
        .and_then(proxy::normalize_proxy_no_proxy)
        .unwrap_or_default();

    let mut password_ciphertext = current
        .as_ref()
        .and_then(|record| record.proxy_password_ciphertext.clone());
    let mut nonce = current.as_ref().and_then(|record| record.nonce.clone());
    if input.clear_proxy_password.unwrap_or(false) || mode != TaskProxyMode::Custom {
        password_ciphertext = None;
        nonce = None;
    }
    if let Some(password) = input.proxy_password.as_deref().map(str::trim) {
        if !password.is_empty() {
            let (ciphertext, secret_nonce) =
                crate::secure_headers::encrypt_secret(password, "task proxy password")?;
            password_ciphertext = Some(ciphertext);
            nonce = Some(secret_nonce);
        }
    }

    sqlx::query(
        r#"
        INSERT INTO task_proxy_settings (
            task_id, mode, proxy_url, proxy_username, proxy_password_ciphertext, nonce,
            no_proxy, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            mode = excluded.mode,
            proxy_url = excluded.proxy_url,
            proxy_username = excluded.proxy_username,
            proxy_password_ciphertext = excluded.proxy_password_ciphertext,
            nonce = excluded.nonce,
            no_proxy = excluded.no_proxy,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&input.task_id)
    .bind(mode.as_str())
    .bind(proxy_url)
    .bind(proxy_username)
    .bind(password_ciphertext)
    .bind(nonce)
    .bind(no_proxy)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    get_task_proxy_settings(pool, &input.task_id).await
}

pub async fn resolve_task_proxy_config(
    pool: &SqlitePool,
    task_id: &str,
    protocol: &str,
    global: &ResolvedProxyConfig,
) -> Result<ResolvedProxyConfig, String> {
    let Some(record) = load_task_proxy_secret_record(pool, task_id).await? else {
        return Ok(global.clone());
    };
    match record.mode {
        TaskProxyMode::Inherit => Ok(global.clone()),
        TaskProxyMode::Off => Ok(ResolvedProxyConfig::default()),
        TaskProxyMode::Custom => {
            let url = record
                .proxy_url
                .clone()
                .and_then(|value| proxy::normalize_proxy_url(&value))
                .ok_or_else(|| proxy_protocol_error(protocol, "Custom proxy URL is invalid."))?;
            validate_task_proxy_protocol(protocol, &url)?;
            let password = match (&record.proxy_password_ciphertext, &record.nonce) {
                (Some(ciphertext), Some(nonce)) => Some(crate::secure_headers::decrypt_secret(
                    ciphertext,
                    nonce,
                    "task proxy password",
                )?),
                _ => None,
            };
            Ok(ResolvedProxyConfig {
                mode: AppProxyMode::Custom,
                url: Some(url),
                no_proxy: record
                    .no_proxy
                    .as_deref()
                    .and_then(proxy::normalize_proxy_no_proxy),
                username: record
                    .proxy_username
                    .as_deref()
                    .and_then(proxy::normalize_proxy_optional),
                password,
            })
        }
    }
}

pub fn validate_task_proxy_protocol(protocol: &str, proxy_url: &str) -> Result<(), String> {
    let scheme = Url::parse(proxy_url)
        .map_err(|_| proxy_protocol_error(protocol, "Custom proxy URL is invalid."))?
        .scheme()
        .to_string();
    match protocol {
        "http" | "https" | "hls" => Ok(()),
        "bt" | "magnet" | "ftp" | "ftps" if scheme == "socks5" => Ok(()),
        "bt" | "magnet" => Err(proxy_protocol_error(
            protocol,
            "BitTorrent tasks only support SOCKS5 task proxy overrides.",
        )),
        "ftp" | "ftps" => Err(proxy_protocol_error(
            protocol,
            "FTP and FTPS tasks only support SOCKS5 task proxy overrides.",
        )),
        _ => Ok(()),
    }
}

fn proxy_protocol_error(protocol: &str, message: &str) -> String {
    crate::models::AppErrorPayload {
        code: "proxy_scheme_unsupported_for_task".to_string(),
        message: format!("{message} Protocol: {protocol}."),
        recoverable: true,
        actions: vec!["check_url".to_string()],
    }
    .command_error()
}

#[derive(Debug, Clone)]
struct TaskProxySecretRecord {
    mode: TaskProxyMode,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
    proxy_password_ciphertext: Option<String>,
    nonce: Option<String>,
    no_proxy: Option<String>,
}

async fn load_task_proxy_secret_record(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<TaskProxySecretRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT mode, proxy_url, proxy_username, proxy_password_ciphertext, nonce, no_proxy
        FROM task_proxy_settings
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row.map(|row| TaskProxySecretRecord {
        mode: TaskProxyMode::from_db_str(row.get::<String, _>("mode").as_str()),
        proxy_url: row.get("proxy_url"),
        proxy_username: row.get("proxy_username"),
        proxy_password_ciphertext: row.get("proxy_password_ciphertext"),
        nonce: row.get("nonce"),
        no_proxy: row.get("no_proxy"),
    }))
}

fn row_to_task_proxy_settings(row: sqlx::sqlite::SqliteRow) -> TaskProxySettings {
    TaskProxySettings {
        task_id: row.get("task_id"),
        mode: TaskProxyMode::from_db_str(row.get::<String, _>("mode").as_str()),
        proxy_url: row
            .get::<Option<String>, _>("proxy_url")
            .unwrap_or_default(),
        proxy_username: row
            .get::<Option<String>, _>("proxy_username")
            .unwrap_or_default(),
        proxy_password_saved: row
            .get::<Option<String>, _>("proxy_password_ciphertext")
            .is_some(),
        no_proxy: row.get::<Option<String>, _>("no_proxy").unwrap_or_default(),
    }
}

fn proxy_mode_for_task(mode: TaskProxyMode) -> AppProxyMode {
    match mode {
        TaskProxyMode::Custom => AppProxyMode::Custom,
        _ => AppProxyMode::Off,
    }
}
