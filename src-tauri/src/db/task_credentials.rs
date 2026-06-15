use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::models::{task::now_iso, AppErrorPayload};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskCredentialsSecret {
    username: String,
    password: String,
}

pub async fn upsert_task_credentials(
    pool: &SqlitePool,
    task_id: &str,
    protocol: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let now = now_iso();
    let secret = serde_json::to_string(&TaskCredentialsSecret {
        username: username.to_string(),
        password: password.to_string(),
    })
    .map_err(|e| format!("Could not serialize task credentials: {e}"))?;
    let (credentials_ciphertext, nonce) =
        crate::secure_headers::encrypt_secret(&secret, "task credentials")?;
    sqlx::query(
        r#"
        INSERT INTO task_credentials (
            task_id, protocol, credentials_ciphertext, nonce, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            protocol = excluded.protocol,
            credentials_ciphertext = excluded.credentials_ciphertext,
            nonce = excluded.nonce,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task_id)
    .bind(protocol)
    .bind(credentials_ciphertext)
    .bind(nonce)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn resolve_task_credentials(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<TaskCredentials>, String> {
    let row = sqlx::query(
        r#"
        SELECT credentials_ciphertext, nonce
        FROM task_credentials
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some(row) = row else {
        return Ok(None);
    };
    let ciphertext: String = row.get("credentials_ciphertext");
    let nonce: String = row.get("nonce");
    let secret = crate::secure_headers::decrypt_secret(&ciphertext, &nonce, "task credentials")
        .map_err(|error| {
            AppErrorPayload::auth_headers_unavailable(format!(
                "Task credentials are unavailable: {error}"
            ))
            .command_error()
        })?;
    let credentials: TaskCredentialsSecret = serde_json::from_str(&secret)
        .map_err(|_| "Stored task credentials are invalid.".to_string())?;
    Ok(Some(TaskCredentials {
        username: credentials.username,
        password: credentials.password,
    }))
}

pub async fn migrate_legacy_ftp_credentials(pool: &SqlitePool) -> Result<(), String> {
    let rows = sqlx::query(
        r#"
        SELECT id, url, final_url, protocol
        FROM tasks
        WHERE protocol IN ('ftp', 'ftps', 'webdav', 'webdavs')
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    for row in rows {
        let task_id: String = row.get("id");
        let url: String = row.get("url");
        let final_url: Option<String> = row.get("final_url");
        let protocol: String = row.get("protocol");

        let legacy_url = legacy_credentials_from_url(&url);
        let legacy_final_url = final_url.as_deref().and_then(legacy_credentials_from_url);
        let Some(legacy) = legacy_url.as_ref().or(legacy_final_url.as_ref()) else {
            continue;
        };
        if let Err(error) = upsert_task_credentials(
            pool,
            &task_id,
            &protocol,
            &legacy.username,
            &legacy.password,
        )
        .await
        {
            let sanitized_url = legacy_url
                .as_ref()
                .map(|value| value.sanitized_url.as_str())
                .unwrap_or(&url);
            let sanitized_final_url = legacy_final_url
                .as_ref()
                .map(|value| value.sanitized_url.as_str())
                .or(final_url.as_deref());
            mark_credentials_required(pool, &task_id, sanitized_url, sanitized_final_url).await?;
            crate::db::insert_task_event(
                pool,
                &task_id,
                "ftp_credentials_unavailable",
                Some(&error),
            )
            .await?;
            tracing::warn!(
                task_id = %task_id,
                error = %error,
                "legacy FTP credentials could not be encrypted; task marked for user attention"
            );
            continue;
        }

        let sanitized_url = legacy_url
            .as_ref()
            .map(|value| value.sanitized_url.as_str())
            .unwrap_or(&url);
        let sanitized_final_url = legacy_final_url
            .as_ref()
            .map(|value| value.sanitized_url.as_str())
            .or(final_url.as_deref());
        update_task_urls(pool, &task_id, sanitized_url, sanitized_final_url).await?;
    }
    Ok(())
}

pub struct LegacyCredentials {
    pub username: String,
    pub password: String,
    pub sanitized_url: String,
}

pub fn legacy_credentials_from_url(input: &str) -> Option<LegacyCredentials> {
    let mut url = reqwest::Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "ftp" | "ftps" | "webdav" | "webdavs") {
        return None;
    }
    if url.username().is_empty() && url.password().is_none() {
        return None;
    }
    let username = if url.username().is_empty() {
        "anonymous".to_string()
    } else {
        percent_decode_lossy(url.username())
    };
    let password = url.password().map(percent_decode_lossy).unwrap_or_default();
    if url.set_username("").is_err() {
        return None;
    }
    let _ = url.set_password(None);
    Some(LegacyCredentials {
        username,
        password,
        sanitized_url: url.to_string(),
    })
}

async fn update_task_urls(
    pool: &SqlitePool,
    task_id: &str,
    sanitized_url: &str,
    sanitized_final_url: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE tasks
        SET url = ?, final_url = COALESCE(?, final_url), updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(sanitized_url)
    .bind(sanitized_final_url)
    .bind(now_iso())
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn mark_credentials_required(
    pool: &SqlitePool,
    task_id: &str,
    sanitized_url: &str,
    final_url: Option<&str>,
) -> Result<(), String> {
    let payload = AppErrorPayload::new(
        "ftp_credentials_unavailable",
        "FTP credentials could not be moved into encrypted storage. Recreate this task or send the URL again.",
        true,
        vec!["check_url", "restart"],
    );
    let actions =
        serde_json::to_string(&vec!["check_url", "restart"]).map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        UPDATE tasks
        SET url = ?, final_url = COALESCE(?, final_url), status = 'needs_attention',
            error_message = ?, error_code = ?, recovery_actions = ?, speed_bps = 0,
            connection_count = 0, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(sanitized_url)
    .bind(final_url)
    .bind(payload.command_error())
    .bind(payload.code)
    .bind(actions)
    .bind(now_iso())
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(hex);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use sqlx::{sqlite::SqlitePoolOptions, Row};

    #[test]
    fn extracts_and_strips_ftp_credentials() {
        let legacy =
            legacy_credentials_from_url("ftp://alice:s3cret@example.com:2121/private/file.bin")
                .expect("credentials");

        assert_eq!(legacy.username, "alice");
        assert_eq!(legacy.password, "s3cret");
        assert_eq!(
            legacy.sanitized_url,
            "ftp://example.com:2121/private/file.bin"
        );
    }

    #[test]
    fn extracts_and_strips_webdav_credentials() {
        let legacy =
            legacy_credentials_from_url("webdavs://alice:s3cret@example.com/private/file.bin")
                .expect("credentials");

        assert_eq!(legacy.username, "alice");
        assert_eq!(legacy.password, "s3cret");
        assert_eq!(
            legacy.sanitized_url,
            "webdavs://example.com/private/file.bin"
        );
    }

    #[test]
    fn ignores_anonymous_ftp_urls() {
        assert!(legacy_credentials_from_url("ftp://example.com/file.bin").is_none());
    }

    #[tokio::test]
    async fn stores_and_resolves_encrypted_credentials_without_plaintext_username() {
        install_test_secret_key();
        let pool = credential_pool().await;

        upsert_task_credentials(&pool, "task-1", "ftp", "alice", "s3cret")
            .await
            .expect("store credentials");

        let row = sqlx::query(
            "SELECT credentials_ciphertext, nonce FROM task_credentials WHERE task_id = ?",
        )
        .bind("task-1")
        .fetch_one(&pool)
        .await
        .expect("credential row");
        let ciphertext: String = row.get("credentials_ciphertext");
        let nonce: String = row.get("nonce");
        assert!(!ciphertext.contains("alice"));
        assert!(!ciphertext.contains("s3cret"));
        assert!(!nonce.is_empty());

        let credentials = resolve_task_credentials(&pool, "task-1")
            .await
            .expect("resolve credentials")
            .expect("credentials");
        assert_eq!(credentials.username, "alice");
        assert_eq!(credentials.password, "s3cret");
    }

    #[tokio::test]
    async fn migrates_legacy_ftp_credentials_and_sanitizes_task_urls() {
        install_test_secret_key();
        let pool = credential_pool().await;
        sqlx::query(
            r#"
            INSERT INTO tasks (id, url, final_url, protocol, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind("legacy-ftp")
        .bind("ftp://alice:s3cret@example.com/private/file.bin")
        .bind("ftp://alice:s3cret@example.com/private/file.bin")
        .bind("ftp")
        .bind(now_iso())
        .execute(&pool)
        .await
        .expect("insert task");

        migrate_legacy_ftp_credentials(&pool)
            .await
            .expect("migrate credentials");

        let row = sqlx::query("SELECT url, final_url FROM tasks WHERE id = ?")
            .bind("legacy-ftp")
            .fetch_one(&pool)
            .await
            .expect("task row");
        let url: String = row.get("url");
        let final_url: Option<String> = row.get("final_url");
        assert_eq!(url, "ftp://example.com/private/file.bin");
        assert_eq!(
            final_url.as_deref(),
            Some("ftp://example.com/private/file.bin")
        );

        let credentials = resolve_task_credentials(&pool, "legacy-ftp")
            .await
            .expect("resolve migrated credentials")
            .expect("credentials");
        assert_eq!(credentials.username, "alice");
        assert_eq!(credentials.password, "s3cret");
    }

    #[tokio::test]
    async fn migrates_credentials_from_legacy_final_url_when_primary_url_is_sanitized() {
        install_test_secret_key();
        let pool = credential_pool().await;
        sqlx::query(
            r#"
            INSERT INTO tasks (id, url, final_url, protocol, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind("legacy-final-url")
        .bind("ftps://example.com/private/file.bin")
        .bind("ftps://alice:s3cret@example.com/private/file.bin")
        .bind("ftps")
        .bind(now_iso())
        .execute(&pool)
        .await
        .expect("insert task");

        migrate_legacy_ftp_credentials(&pool)
            .await
            .expect("migrate credentials");

        let row = sqlx::query("SELECT url, final_url FROM tasks WHERE id = ?")
            .bind("legacy-final-url")
            .fetch_one(&pool)
            .await
            .expect("task row");
        assert_eq!(
            row.get::<String, _>("url"),
            "ftps://example.com/private/file.bin"
        );
        assert_eq!(
            row.get::<Option<String>, _>("final_url").as_deref(),
            Some("ftps://example.com/private/file.bin")
        );

        let credentials = resolve_task_credentials(&pool, "legacy-final-url")
            .await
            .expect("resolve migrated credentials")
            .expect("credentials");
        assert_eq!(credentials.username, "alice");
        assert_eq!(credentials.password, "s3cret");
    }

    async fn credential_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        sqlx::query(
            r#"
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                final_url TEXT,
                protocol TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'queued',
                error_message TEXT,
                error_code TEXT,
                recovery_actions TEXT NOT NULL DEFAULT '[]',
                speed_bps INTEGER NOT NULL DEFAULT 0,
                connection_count INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("tasks table");
        sqlx::query(
            r#"
            CREATE TABLE task_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("task_events table");
        sqlx::query(
            r#"
            CREATE TABLE task_credentials (
                task_id TEXT PRIMARY KEY,
                protocol TEXT NOT NULL,
                credentials_ciphertext TEXT NOT NULL,
                nonce TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("task_credentials table");
        pool
    }

    fn install_test_secret_key() {
        std::env::set_var(
            "VIBE_DOWNLOADER_TEST_SECRET_KEY",
            STANDARD.encode([7_u8; 32]),
        );
    }
}
