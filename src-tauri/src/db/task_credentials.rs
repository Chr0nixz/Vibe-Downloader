use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::models::{task::now_iso, AppErrorPayload};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCredentials {
    pub username: String,
    pub password: String,
    pub private_key_data: Option<String>,
    pub private_key_passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskCredentialsSecret {
    username: String,
    password: String,
    #[serde(default)]
    private_key_data: Option<String>,
    #[serde(default)]
    private_key_passphrase: Option<String>,
}

pub async fn upsert_task_credentials(
    pool: &SqlitePool,
    task_id: &str,
    protocol: &str,
    username: &str,
    password: &str,
    private_key_data: Option<&str>,
    private_key_passphrase: Option<&str>,
) -> Result<(), String> {
    let now = now_iso();
    let secret = serde_json::to_string(&TaskCredentialsSecret {
        username: username.to_string(),
        password: password.to_string(),
        private_key_data: private_key_data.map(|s| s.to_string()),
        private_key_passphrase: private_key_passphrase.map(|s| s.to_string()),
    })
    .map_err(|e| format!("Could not serialize task credentials: {e}"))?;
    let (credentials_ciphertext, nonce) =
        crate::secure_headers::encrypt_secret(&secret, "task credentials", task_id.as_bytes())
            .map_err(|error| task_credentials_encrypt_error(&error))?;
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
    let secret = crate::secure_headers::decrypt_secret(
        &ciphertext,
        &nonce,
        "task credentials",
        task_id.as_bytes(),
    )
    .map_err(|error| task_credentials_decrypt_error(&error))?;
    let credentials: TaskCredentialsSecret = serde_json::from_str(&secret)
        .map_err(|_| task_credentials_invalid_error("Stored task credentials are invalid."))?;
    Ok(Some(TaskCredentials {
        username: credentials.username,
        password: credentials.password,
        private_key_data: credentials.private_key_data,
        private_key_passphrase: credentials.private_key_passphrase,
    }))
}

pub async fn migrate_legacy_ftp_credentials(pool: &SqlitePool) -> Result<(), String> {
    let rows = sqlx::query(
        r#"
        SELECT id, url, final_url, protocol
        FROM tasks
        WHERE protocol IN ('ftp', 'ftps', 'sftp', 'webdav', 'webdavs')
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok(());
    }

    // Wrap all per-row writes in one transaction. Without this, a crash
    // mid-loop could leave credentials encrypted but URLs still containing
    // plaintext passwords. The encryption itself (keyring) is a non-SQL
    // side effect, but all DB writes are atomic.
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

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

        let sanitized_url = legacy_url
            .as_ref()
            .map(|v| v.sanitized_url.as_str())
            .unwrap_or(&url);
        let sanitized_final_url = legacy_final_url
            .as_ref()
            .map(|v| v.sanitized_url.as_str())
            .or(final_url.as_deref());

        // Encrypt credentials (non-SQL side effect; keyring write).
        let secret = serde_json::to_string(&TaskCredentialsSecret {
            username: legacy.username.clone(),
            password: legacy.password.clone(),
            private_key_data: None,
            private_key_passphrase: None,
        })
        .map_err(|e| format!("Could not serialize task credentials: {e}"))?;

        let (credentials_ciphertext, nonce) =
            match crate::secure_headers::encrypt_secret(&secret, "task credentials", task_id.as_bytes())
            {
                Ok(result) => result,
                Err(error) => {
                    // Encryption failed — mark task, sanitize URL, all within the tx.
                    let now = crate::models::task::now_iso();
                    sqlx::query(
                        r#"
                        UPDATE tasks
                        SET url = ?, final_url = COALESCE(?, final_url), status = 'needs_attention',
                            error_message = ?, error_code = 'task_credentials_unavailable',
                            speed_bps = 0, connection_count = 0, updated_at = ?
                        WHERE id = ?
                        "#,
                    )
                    .bind(sanitized_url)
                    .bind(sanitized_final_url)
                    .bind(&task_credentials_encrypt_error(&error))
                    .bind(&now)
                    .bind(&task_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;

                    let event_now = crate::models::task::now_iso();
                    sqlx::query(
                        r#"
                        INSERT INTO task_events (task_id, event_type, payload, created_at)
                        VALUES (?, 'task_credentials_unavailable', ?, ?)
                        "#,
                    )
                    .bind(&task_id)
                    .bind(&task_credentials_encrypt_error(&error))
                    .bind(&event_now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;

                    tracing::warn!(
                        task_id = %task_id,
                        error = %error,
                        "legacy FTP credentials could not be encrypted; task marked for user attention"
                    );
                    continue;
                }
            };

        // Inline upsert_task_credentials within the transaction.
        let now = crate::models::task::now_iso();
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
        .bind(&task_id)
        .bind(&protocol)
        .bind(&credentials_ciphertext)
        .bind(&nonce)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        // Inline update_task_urls within the transaction.
        let url_now = crate::models::task::now_iso();
        sqlx::query(
            r#"
            UPDATE tasks
            SET url = ?, final_url = COALESCE(?, final_url), updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(sanitized_url)
        .bind(sanitized_final_url)
        .bind(&url_now)
        .bind(&task_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub struct LegacyCredentials {
    pub username: String,
    pub password: String,
    pub sanitized_url: String,
}

fn task_credentials_encrypt_error(error: &str) -> String {
    AppErrorPayload::new(
        "task_credentials_encrypt_failed",
        format!("Could not encrypt task credentials: {error}"),
        true,
        vec!["restart", "check_url"],
    )
    .command_error()
}

fn task_credentials_decrypt_error(error: &str) -> String {
    AppErrorPayload::new(
        "task_credentials_decrypt_failed",
        format!("Could not decrypt stored task credentials: {error}"),
        true,
        vec!["restart", "check_url"],
    )
    .command_error()
}

fn task_credentials_invalid_error(message: &str) -> String {
    AppErrorPayload::new(
        "task_credentials_invalid",
        message.to_string(),
        true,
        vec!["restart", "check_url"],
    )
    .command_error()
}

pub fn legacy_credentials_from_url(input: &str) -> Option<LegacyCredentials> {
    let mut url = reqwest::Url::parse(input.trim()).ok()?;
    let scheme = url.scheme().to_string();
    if !matches!(
        scheme.as_str(),
        "ftp" | "ftps" | "sftp" | "webdav" | "webdavs" | "http" | "https"
    ) {
        return None;
    }
    if url.username().is_empty() && url.password().is_none() {
        return None;
    }
    if scheme == "sftp" && url.username().is_empty() {
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
    fn ignores_anonymous_ftp_urls() {
        assert!(legacy_credentials_from_url("ftp://example.com/file.bin").is_none());
    }

    #[test]
    fn extracts_and_strips_sftp_credentials_without_anonymous_default() {
        let legacy =
            legacy_credentials_from_url("sftp://alice:s3cret@example.com:2222/private/file.bin")
                .expect("credentials");

        assert_eq!(legacy.username, "alice");
        assert_eq!(legacy.password, "s3cret");
        assert_eq!(
            legacy.sanitized_url,
            "sftp://example.com:2222/private/file.bin"
        );
        assert!(legacy_credentials_from_url("sftp://:pass@example.com/file.bin").is_none());
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

    #[tokio::test]
    async fn stores_and_resolves_encrypted_credentials_without_plaintext_username() {
        install_test_secret_key();
        let pool = credential_pool().await;

        upsert_task_credentials(&pool, "task-1", "ftp", "alice", "s3cret", None, None)
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

    #[test]
    fn credential_error_payloads_are_distinct() {
        let encrypt = task_credentials_encrypt_error("boom");
        let decrypt = task_credentials_decrypt_error("boom");
        let invalid = task_credentials_invalid_error("bad data");

        assert!(encrypt.contains("task_credentials_encrypt_failed"));
        assert!(decrypt.contains("task_credentials_decrypt_failed"));
        assert!(invalid.contains("task_credentials_invalid"));
        assert!(!encrypt.contains("task_credentials_decrypt_failed"));
        assert!(!decrypt.contains("task_credentials_encrypt_failed"));
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
