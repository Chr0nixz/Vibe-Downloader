//! Encrypted task profiles expire secrets without discarding public overrides.

use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::models::{
    is_sensitive_request_header, normalize_task_request_profile, AppErrorPayload,
    TaskRequestHeaderInput, TaskRequestProfileInput, TaskRequestProfileView,
};

pub struct PreparedTaskRequestProfile {
    origin: String,
    public_ciphertext: String,
    public_nonce: String,
    sensitive_ciphertext: Option<String>,
    sensitive_nonce: Option<String>,
    sensitive_names_json: String,
    sensitive_expires_at: Option<String>,
}

pub fn prepare_task_request_profile(
    url: &str,
    input: &TaskRequestProfileInput,
) -> Result<PreparedTaskRequestProfile, String> {
    prepare_task_request_profile_with_expiry(url, input, None)
}

pub fn prepare_task_request_profile_with_expiry(
    url: &str,
    input: &TaskRequestProfileInput,
    sensitive_expires_at: Option<&str>,
) -> Result<PreparedTaskRequestProfile, String> {
    let headers = normalize_task_request_profile(url, input)?;
    let (sensitive, public): (Vec<_>, Vec<_>) = headers
        .into_iter()
        .partition(|(name, _)| is_sensitive_request_header(name));
    let raw = zeroize::Zeroizing::new(serde_json::to_string(&public).map_err(|e| e.to_string())?);
    let (public_ciphertext, public_nonce) = crate::secure_headers::encrypt_headers(&raw)?;
    let sensitive_names_json =
        serde_json::to_string(&sensitive.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .map_err(|e| e.to_string())?;
    let (sensitive_ciphertext, sensitive_nonce, sensitive_expires_at) = if sensitive.is_empty() {
        (None, None, None)
    } else {
        let raw =
            zeroize::Zeroizing::new(serde_json::to_string(&sensitive).map_err(|e| e.to_string())?);
        let (ciphertext, nonce) = crate::secure_headers::encrypt_headers(&raw)?;
        let expires_at = sensitive_expires_at.map(str::to_string).unwrap_or_else(|| {
            (chrono::Utc::now() + chrono::Duration::hours(super::TASK_REQUEST_HEADERS_TTL_HOURS))
                .to_rfc3339()
        });
        if expires_at <= crate::models::task::now_iso() {
            return Err(AppErrorPayload::auth_headers_expired().command_error());
        }
        (Some(ciphertext), Some(nonce), Some(expires_at))
    };
    Ok(PreparedTaskRequestProfile {
        origin: crate::download::url_origin(url).unwrap_or_default(),
        public_ciphertext,
        public_nonce,
        sensitive_ciphertext,
        sensitive_nonce,
        sensitive_names_json,
        sensitive_expires_at,
    })
}

pub async fn save_task_request_profile(
    conn: &mut SqliteConnection,
    task_id: &str,
    profile: &PreparedTaskRequestProfile,
) -> Result<(), String> {
    sqlx::query("INSERT INTO task_request_profiles (task_id, origin, public_ciphertext, public_nonce,
        sensitive_ciphertext, sensitive_nonce, sensitive_names_json, sensitive_expires_at,
        sensitive_expired, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?)
        ON CONFLICT(task_id) DO UPDATE SET origin=excluded.origin,
        public_ciphertext=excluded.public_ciphertext, public_nonce=excluded.public_nonce,
        sensitive_ciphertext=excluded.sensitive_ciphertext, sensitive_nonce=excluded.sensitive_nonce,
        sensitive_names_json=excluded.sensitive_names_json, sensitive_expires_at=excluded.sensitive_expires_at,
        sensitive_expired=0, updated_at=excluded.updated_at")
        .bind(task_id).bind(&profile.origin).bind(&profile.public_ciphertext).bind(&profile.public_nonce)
        .bind(&profile.sensitive_ciphertext).bind(&profile.sensitive_nonce)
        .bind(&profile.sensitive_names_json).bind(&profile.sensitive_expires_at)
        .bind(crate::models::task::now_iso()).execute(conn).await.map_err(|e| e.to_string())?;
    Ok(())
}

fn decrypt(ciphertext: &str, nonce: &str) -> Result<Vec<(String, String)>, String> {
    let raw = crate::secure_headers::decrypt_headers(ciphertext, nonce).map_err(|_| {
        AppErrorPayload::auth_headers_unavailable("Task request profile is unavailable.")
            .command_error()
    })?;
    serde_json::from_str(&raw).map_err(|_| {
        AppErrorPayload::auth_headers_unavailable("Stored task request profile is invalid.")
            .command_error()
    })
}

pub async fn expire_task_request_profile_secrets(pool: &SqlitePool) -> Result<(), String> {
    // Keep the expiry marker: another retry must require refresh, not silently
    // send an unauthenticated request after the secret bytes have been wiped.
    sqlx::query(
        "UPDATE task_request_profiles SET sensitive_ciphertext=NULL, sensitive_nonce=NULL,
        sensitive_expired=1 WHERE sensitive_expires_at <= ? AND sensitive_expired=0",
    )
    .bind(crate::models::task::now_iso())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn resolve_task_request_profile_headers(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<(String, String)>, String> {
    expire_task_request_profile_secrets(pool).await?;
    let row = sqlx::query("SELECT * FROM task_request_profiles WHERE task_id=?")
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(row) = row else {
        return Ok(Vec::new());
    };
    if row.get::<bool, _>("sensitive_expired") {
        return Err(AppErrorPayload::auth_headers_expired().command_error());
    }
    let mut headers = decrypt(row.get("public_ciphertext"), row.get("public_nonce"))?;
    match (
        row.get::<Option<String>, _>("sensitive_ciphertext"),
        row.get::<Option<String>, _>("sensitive_nonce"),
    ) {
        (Some(ciphertext), Some(nonce)) => headers.extend(decrypt(&ciphertext, &nonce)?),
        (None, None) => {}
        _ => {
            return Err(AppErrorPayload::auth_headers_unavailable(
                "Task request metadata is incomplete.",
            )
            .command_error())
        }
    }
    Ok(headers)
}

pub async fn get_task_request_profile(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<TaskRequestProfileView, String> {
    expire_task_request_profile_secrets(pool).await?;
    let mut view = TaskRequestProfileView {
        task_id: task_id.to_string(),
        user_agent: None,
        referer: None,
        custom_headers: Vec::new(),
        sensitive_header_names: Vec::new(),
        sensitive_expires_at: None,
        sensitive_expired: false,
    };
    let row = sqlx::query("SELECT * FROM task_request_profiles WHERE task_id=?")
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(row) = row {
        for (name, value) in decrypt(row.get("public_ciphertext"), row.get("public_nonce"))? {
            match name.as_str() {
                "user-agent" => view.user_agent = Some(value),
                "referer" => view.referer = Some(value),
                _ => view
                    .custom_headers
                    .push(TaskRequestHeaderInput { name, value }),
            }
        }
        view.sensitive_header_names =
            serde_json::from_str(row.get("sensitive_names_json")).map_err(|e| e.to_string())?;
        view.sensitive_expires_at = row.get("sensitive_expires_at");
        view.sensitive_expired = row.get("sensitive_expired");
    }
    Ok(view)
}

/// Ordinary edits never refresh a stored token's expiry or expose its value.
pub async fn update_task_request_profile(
    pool: &SqlitePool,
    task_id: &str,
    url: &str,
    input: &TaskRequestProfileInput,
    replace_sensitive: bool,
) -> Result<(), String> {
    let headers = normalize_task_request_profile(url, input)?;
    if !replace_sensitive
        && headers
            .iter()
            .any(|(name, _)| is_sensitive_request_header(name))
    {
        return Err(AppErrorPayload::new(
            "request_profile_invalid",
            "Secret replacement must be explicit.",
            false,
            vec![],
        )
        .command_error());
    }
    let profile = prepare_task_request_profile(url, input)?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    if replace_sensitive {
        save_task_request_profile(&mut conn, task_id, &profile).await
    } else {
        sqlx::query("INSERT INTO task_request_profiles (task_id, origin, public_ciphertext, public_nonce, updated_at)
            VALUES (?, ?, ?, ?, ?) ON CONFLICT(task_id) DO UPDATE SET
            public_ciphertext=excluded.public_ciphertext, public_nonce=excluded.public_nonce, updated_at=excluded.updated_at")
            .bind(task_id).bind(&profile.origin).bind(&profile.public_ciphertext).bind(&profile.public_nonce)
            .bind(crate::models::task::now_iso()).execute(&mut *conn).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
