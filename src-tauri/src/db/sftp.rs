use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, SqlitePool};

use crate::models::task::now_iso;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SftpKnownHost {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint_sha256: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

/// TOFU (Trust On First Use): the first fingerprint seen for a host:port is recorded as
/// trusted. Any subsequent mismatch is rejected as a potential MITM. The user must
/// explicitly clear the `sftp_known_hosts` row to accept a legitimately-rotated key.
pub async fn verify_or_record_sftp_host_key(
    pool: &SqlitePool,
    host: &str,
    port: u16,
    algorithm: &str,
    fingerprint_sha256: &str,
) -> Result<(), String> {
    let host = host.trim().to_ascii_lowercase();
    let port = i64::from(port);
    let row = sqlx::query(
        r#"
        SELECT algorithm, fingerprint_sha256
        FROM sftp_known_hosts
        WHERE host = ? AND port = ?
        "#,
    )
    .bind(&host)
    .bind(port)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let now = now_iso();
    match row {
        Some(row) => {
            let known_algorithm: String = row.get("algorithm");
            let known_fingerprint: String = row.get("fingerprint_sha256");
            if known_fingerprint != fingerprint_sha256 {
                // ARC-15: fail-closed mismatch with a recoverable action that opens
                // Settings → Network known-host management (forget is explicit).
                return Err(crate::models::AppErrorPayload::new(
                    "sftp_host_key_changed",
                    format!(
                        "SFTP host key changed for {host}:{port}. Expected {known_algorithm} {known_fingerprint}, got {algorithm} {fingerprint_sha256}."
                    ),
                    true,
                    vec!["manage_sftp_host_keys", "retry"],
                )
                .command_error());
            }
            sqlx::query(
                r#"
                UPDATE sftp_known_hosts
                SET algorithm = ?, last_seen_at = ?
                WHERE host = ? AND port = ?
                "#,
            )
            .bind(algorithm)
            .bind(&now)
            .bind(&host)
            .bind(port)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        None => {
            sqlx::query(
                r#"
                INSERT INTO sftp_known_hosts (
                    host, port, algorithm, fingerprint_sha256, first_seen_at, last_seen_at
                ) VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&host)
            .bind(port)
            .bind(algorithm)
            .bind(fingerprint_sha256)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

/// ARC-15: List trusted SFTP host keys for the Settings recovery UI.
pub async fn list_sftp_known_hosts(pool: &SqlitePool) -> Result<Vec<SftpKnownHost>, String> {
    let rows = sqlx::query(
        r#"
        SELECT host, port, algorithm, fingerprint_sha256, first_seen_at, last_seen_at
        FROM sftp_known_hosts
        ORDER BY host ASC, port ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.into_iter()
        .map(|row| {
            let port: i64 = row.get("port");
            let port =
                u16::try_from(port).map_err(|_| format!("invalid sftp known-host port: {port}"))?;
            Ok(SftpKnownHost {
                host: row.get("host"),
                port,
                algorithm: row.get("algorithm"),
                fingerprint_sha256: row.get("fingerprint_sha256"),
                first_seen_at: row.get("first_seen_at"),
                last_seen_at: row.get("last_seen_at"),
            })
        })
        .collect()
}

/// ARC-15: Explicitly forget a trusted host key. DELETE only — never overwrite
/// fingerprints in place; the next connection re-runs TOFU INSERT.
pub async fn forget_sftp_known_host(
    pool: &SqlitePool,
    host: &str,
    port: u16,
) -> Result<bool, String> {
    let host = host.trim().to_ascii_lowercase();
    let result = sqlx::query(
        r#"
        DELETE FROM sftp_known_hosts
        WHERE host = ? AND port = ?
        "#,
    )
    .bind(&host)
    .bind(i64::from(port))
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(result.rows_affected() > 0)
}
