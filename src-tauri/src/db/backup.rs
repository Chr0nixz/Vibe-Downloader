//! FUN-16 / D3: versioned application database backup format (`.vibe-backup`).

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

use super::connection::{create_verified_backup, wal_checkpoint};

pub const BACKUP_MAGIC: &[u8; 4] = b"VIBE";
pub const BACKUP_FORMAT_VERSION: u8 = 1;
pub const PENDING_RESTORE_SUFFIX: &str = ".vibe-restore-pending";
pub const CREDENTIALS_POLICY_MACHINE_BOUND: &str = "machine_bound_ciphertext";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format: String,
    pub format_version: u8,
    pub app_version: String,
    pub schema_version: i64,
    pub created_at: String,
    pub credentials_policy: String,
    pub includes_global_proxy_password: bool,
    pub checksum_algorithm: String,
    pub checksum: String,
    pub database_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedBackup {
    pub manifest: BackupManifest,
    pub database: Vec<u8>,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    to_hex(&hasher.finalize())
}

pub async fn current_schema_version(pool: &SqlitePool) -> Result<i64, String> {
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Could not read schema version: {e}"))?;
    Ok(version.unwrap_or(0))
}

/// Snapshot the live database into a verified SQLite file via VACUUM INTO.
pub async fn snapshot_database_to_path(
    pool: &SqlitePool,
    db_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    wal_checkpoint(pool).await?;
    if destination.exists() {
        std::fs::remove_file(destination)
            .map_err(|e| format!("Could not replace existing snapshot: {e}"))?;
    }
    // Reuse the verified VACUUM path by writing beside the live DB, then rename.
    let verified = create_verified_backup(pool, db_path).await?;
    std::fs::rename(&verified, destination).map_err(|e| {
        let _ = std::fs::remove_file(&verified);
        format!("Could not move verified snapshot into place: {e}")
    })?;
    Ok(())
}

pub fn pack_backup_file(
    manifest_without_checksum: &BackupManifest,
    database: &[u8],
) -> Result<Vec<u8>, String> {
    let mut manifest = manifest_without_checksum.clone();
    manifest.database_bytes = database.len() as u64;
    manifest.checksum = sha256_hex(&[database]);
    let manifest_json = serde_json::to_vec(&manifest)
        .map_err(|e| format!("Could not serialize backup manifest: {e}"))?;

    let mut out = Vec::with_capacity(4 + 1 + 4 + manifest_json.len() + 8 + database.len() + 32);
    out.extend_from_slice(BACKUP_MAGIC);
    out.push(BACKUP_FORMAT_VERSION);
    out.extend_from_slice(&(manifest_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&manifest_json);
    out.extend_from_slice(&(database.len() as u64).to_le_bytes());
    out.extend_from_slice(database);
    let mut hasher = Sha256::new();
    hasher.update(&manifest_json);
    hasher.update(database);
    out.extend_from_slice(&hasher.finalize());
    Ok(out)
}

pub fn parse_backup_bytes(bytes: &[u8]) -> Result<ParsedBackup, String> {
    if bytes.len() < 4 + 1 + 4 + 8 + 32 {
        return Err(engine_backup_error(
            "backup_corrupt",
            "Backup file is truncated.",
        ));
    }
    if &bytes[0..4] != BACKUP_MAGIC {
        return Err(engine_backup_error(
            "backup_invalid_magic",
            "Backup file magic does not match a Vibe backup.",
        ));
    }
    let format_version = bytes[4];
    if format_version != BACKUP_FORMAT_VERSION {
        return Err(engine_backup_error(
            "backup_unsupported_version",
            format!("Unsupported backup format version: {format_version}"),
        ));
    }
    let manifest_len = u32::from_le_bytes(bytes[5..9].try_into().unwrap()) as usize;
    let manifest_start: usize = 9;
    let manifest_end = manifest_start
        .checked_add(manifest_len)
        .ok_or_else(|| engine_backup_error("backup_corrupt", "Invalid manifest length."))?;
    if manifest_end + 8 + 32 > bytes.len() {
        return Err(engine_backup_error(
            "backup_corrupt",
            "Backup file is truncated while reading the manifest.",
        ));
    }
    let manifest_json = &bytes[manifest_start..manifest_end];
    let db_len =
        u64::from_le_bytes(bytes[manifest_end..manifest_end + 8].try_into().unwrap()) as usize;
    let db_start = manifest_end + 8;
    let db_end = db_start
        .checked_add(db_len)
        .ok_or_else(|| engine_backup_error("backup_corrupt", "Invalid database length."))?;
    if db_end + 32 != bytes.len() {
        return Err(engine_backup_error(
            "backup_corrupt",
            "Backup file length does not match the declared payload.",
        ));
    }
    let database = &bytes[db_start..db_end];
    let trailer = &bytes[db_end..];
    let mut hasher = Sha256::new();
    hasher.update(manifest_json);
    hasher.update(database);
    let digest = hasher.finalize();
    if digest.as_slice() != trailer {
        return Err(engine_backup_error(
            "backup_checksum_mismatch",
            "Backup checksum verification failed.",
        ));
    }
    let manifest: BackupManifest = serde_json::from_slice(manifest_json).map_err(|e| {
        engine_backup_error(
            "backup_invalid_manifest",
            format!("Backup manifest is invalid: {e}"),
        )
    })?;
    if manifest.format != "vibe-backup" {
        return Err(engine_backup_error(
            "backup_invalid_manifest",
            "Backup manifest format field is not vibe-backup.",
        ));
    }
    let expected_db_checksum = sha256_hex(&[database]);
    if !manifest
        .checksum
        .eq_ignore_ascii_case(&expected_db_checksum)
    {
        return Err(engine_backup_error(
            "backup_checksum_mismatch",
            "Backup manifest database checksum mismatch.",
        ));
    }
    if manifest.database_bytes != database.len() as u64 {
        return Err(engine_backup_error(
            "backup_corrupt",
            "Backup manifest database size does not match payload.",
        ));
    }
    Ok(ParsedBackup {
        manifest,
        database: database.to_vec(),
    })
}

pub fn read_backup_file(path: &Path) -> Result<ParsedBackup, String> {
    let mut file = File::open(path).map_err(|e| {
        engine_backup_error(
            "backup_read_failed",
            format!("Could not read backup file: {e}"),
        )
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| {
        engine_backup_error(
            "backup_read_failed",
            format!("Could not read backup file: {e}"),
        )
    })?;
    parse_backup_bytes(&bytes)
}

pub fn write_backup_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            engine_backup_error(
                "backup_write_failed",
                format!("Could not create backup folder: {e}"),
            )
        })?;
    }
    let mut file = File::create(path).map_err(|e| {
        engine_backup_error(
            "backup_write_failed",
            format!("Could not create backup file: {e}"),
        )
    })?;
    file.write_all(bytes).map_err(|e| {
        engine_backup_error(
            "backup_write_failed",
            format!("Could not write backup file: {e}"),
        )
    })?;
    Ok(())
}

/// Materialize backup database bytes to a temp path and verify integrity + migrations.
pub async fn materialize_and_verify_backup_db(
    database: &[u8],
    schema_version: i64,
    current_schema: i64,
) -> Result<PathBuf, String> {
    if schema_version > current_schema {
        return Err(engine_backup_error(
            "backup_schema_too_new",
            format!(
                "Backup schema version {schema_version} is newer than this app ({current_schema})."
            ),
        ));
    }
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("vibe-backup-verify-{id}.sqlite"));
    std::fs::write(&path, database).map_err(|e| {
        engine_backup_error(
            "backup_write_failed",
            format!("Could not materialize backup database: {e}"),
        )
    })?;
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&path);
            engine_backup_error(
                "backup_invalid_database",
                format!("Could not open backup database: {e}"),
            )
        })?;
    let integrity: String = match sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
    {
        Ok(value) => value,
        Err(e) => {
            // Cannot use map_err here: SqlitePool::close must be awaited.
            pool.close().await;
            let _ = std::fs::remove_file(&path);
            return Err(engine_backup_error(
                "backup_invalid_database",
                format!("Backup integrity check failed: {e}"),
            ));
        }
    };
    if integrity != "ok" {
        pool.close().await;
        let _ = std::fs::remove_file(&path);
        return Err(engine_backup_error(
            "backup_invalid_database",
            format!("Backup integrity check failed: {integrity}"),
        ));
    }
    if let Err(error) = super::connection::run_migrations_for_backup(&pool).await {
        pool.close().await;
        let _ = std::fs::remove_file(&path);
        return Err(engine_backup_error(
            "backup_migrate_failed",
            format!("Backup database could not be migrated: {error}"),
        ));
    }
    pool.close().await;
    Ok(path)
}

pub fn pending_restore_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(PENDING_RESTORE_SUFFIX);
    PathBuf::from(path)
}

/// Apply a staged restore before opening the live pool (startup path).
pub fn apply_pending_restore_if_any(db_path: &Path) -> Result<bool, String> {
    let pending = pending_restore_path(db_path);
    if !pending.exists() {
        return Ok(false);
    }
    // Replace live DB + sidecars with the pending restored file.
    for sidecar in ["-wal", "-shm", "-journal"] {
        let mut side = db_path.as_os_str().to_owned();
        side.push(sidecar);
        let side_path = PathBuf::from(side);
        if side_path.exists() {
            let _ = std::fs::remove_file(&side_path);
        }
    }
    if db_path.exists() {
        std::fs::remove_file(db_path)
            .map_err(|e| format!("Could not replace live database during pending restore: {e}"))?;
    }
    std::fs::rename(&pending, db_path)
        .map_err(|e| format!("Could not apply pending restore database: {e}"))?;
    tracing::info!(
        db_path = %db_path.display(),
        "applied pending vibe-backup restore"
    );
    Ok(true)
}

fn engine_backup_error(code: &str, message: impl Into<String>) -> String {
    crate::models::AppErrorPayload::new(code, message, false, vec!["check_url"]).command_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_and_parse_round_trip() {
        let db = b"sqlite-bytes-not-real".to_vec();
        let manifest = BackupManifest {
            format: "vibe-backup".into(),
            format_version: BACKUP_FORMAT_VERSION,
            app_version: "0.3.0".into(),
            schema_version: 6,
            created_at: "2026-07-20T00:00:00Z".into(),
            credentials_policy: CREDENTIALS_POLICY_MACHINE_BOUND.into(),
            includes_global_proxy_password: false,
            checksum_algorithm: "sha256".into(),
            checksum: String::new(),
            database_bytes: 0,
        };
        let packed = pack_backup_file(&manifest, &db).expect("pack");
        let parsed = parse_backup_bytes(&packed).expect("parse");
        assert_eq!(parsed.database, db);
        assert_eq!(parsed.manifest.schema_version, 6);
    }

    #[test]
    fn corrupt_trailer_is_rejected() {
        let db = b"payload".to_vec();
        let manifest = BackupManifest {
            format: "vibe-backup".into(),
            format_version: BACKUP_FORMAT_VERSION,
            app_version: "0.3.0".into(),
            schema_version: 6,
            created_at: "2026-07-20T00:00:00Z".into(),
            credentials_policy: CREDENTIALS_POLICY_MACHINE_BOUND.into(),
            includes_global_proxy_password: false,
            checksum_algorithm: "sha256".into(),
            checksum: String::new(),
            database_bytes: 0,
        };
        let mut packed = pack_backup_file(&manifest, &db).expect("pack");
        let last = packed.len() - 1;
        packed[last] ^= 0xff;
        let err = parse_backup_bytes(&packed).expect_err("must fail");
        assert!(err.contains("backup_checksum_mismatch") || err.contains("checksum"));
    }
}
