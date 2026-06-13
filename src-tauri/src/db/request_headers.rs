use sqlx::{Row, SqlitePool};

use crate::models::{AppErrorPayload, BrowserKind};

pub const TASK_REQUEST_HEADERS_TTL_HOURS: i64 = 24;

pub async fn upsert_task_request_headers(
    pool: &SqlitePool,
    task_id: &str,
    headers: &[(String, String)],
    source_browser: Option<BrowserKind>,
) -> Result<(), String> {
    if headers.is_empty() {
        return Ok(());
    }
    let now = crate::models::task::now_iso();
    let expires_at =
        (chrono::Utc::now() + chrono::Duration::hours(TASK_REQUEST_HEADERS_TTL_HOURS)).to_rfc3339();
    let headers_json = serde_json::to_string(headers).map_err(|e| e.to_string())?;
    let (headers_ciphertext, nonce) = crate::secure_headers::encrypt_headers(&headers_json)?;
    sqlx::query(
        r#"
        INSERT INTO task_request_headers (
            task_id, headers_json, headers_ciphertext, nonce, expires_at, created_at, last_used_at, source_browser
        ) VALUES (?, '', ?, ?, ?, ?, NULL, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            headers_json = '',
            headers_ciphertext = excluded.headers_ciphertext,
            nonce = excluded.nonce,
            expires_at = excluded.expires_at,
            source_browser = excluded.source_browser
        "#,
    )
    .bind(task_id)
    .bind(headers_ciphertext)
    .bind(nonce)
    .bind(expires_at)
    .bind(&now)
    .bind(source_browser.map(BrowserKind::as_str))
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn resolve_task_request_headers(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let now = crate::models::task::now_iso();
    let row = sqlx::query(
        r#"
        SELECT headers_json, headers_ciphertext, nonce, expires_at FROM task_request_headers
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some(row) = row else {
        return Ok(Vec::new());
    };
    let expires_at: String = row.get("expires_at");
    if expires_at <= now {
        delete_task_request_headers(pool, task_id).await?;
        return Err(AppErrorPayload::auth_headers_expired().command_error());
    }

    sqlx::query("UPDATE task_request_headers SET last_used_at = ? WHERE task_id = ?")
        .bind(&now)
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let ciphertext: Option<String> = row.get("headers_ciphertext");
    let nonce: Option<String> = row.get("nonce");
    let headers_json = match (ciphertext, nonce) {
        (Some(ciphertext), Some(nonce)) if !ciphertext.is_empty() && !nonce.is_empty() => {
            crate::secure_headers::decrypt_headers(&ciphertext, &nonce).map_err(|error| {
                AppErrorPayload::auth_headers_unavailable(format!(
                    "Browser authentication headers are unavailable: {error}"
                ))
                .command_error()
            })?
        }
        _ => {
            let raw: String = row.get("headers_json");
            if raw.is_empty() {
                return Ok(Vec::new());
            }
            if let Ok(headers) = serde_json::from_str::<Vec<(String, String)>>(&raw) {
                let _ = upsert_task_request_headers(pool, task_id, &headers, None).await;
            }
            raw
        }
    };

    serde_json::from_str(&headers_json).map_err(|e| e.to_string())
}

pub async fn delete_task_request_headers(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM task_request_headers WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn clear_all_task_request_headers(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("DELETE FROM task_request_headers")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn clear_expired_task_request_headers(pool: &SqlitePool) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query("DELETE FROM task_request_headers WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
