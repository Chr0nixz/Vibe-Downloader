use sqlx::{Row, SqlitePool};

use crate::models::task::now_iso;

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
                return Err(crate::models::AppErrorPayload::new(
                    "sftp_host_key_changed",
                    format!(
                        "SFTP host key changed for {host}:{port}. Expected {known_algorithm} {known_fingerprint}, got {algorithm} {fingerprint_sha256}."
                    ),
                    true,
                    vec!["check_url", "restart"],
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
