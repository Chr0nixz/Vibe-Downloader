//! FUN-16 / D3: versioned application database backup format (`.vibe-backup`).

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

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
    // Reuse the verified VACUUM path by writing beside the live DB, then move.
    // FUN-23: `rename` cannot cross volumes/filesystems (Windows
    // ERROR_NOT_SAME_DEVICE, POSIX EXDEV) — a user-selected destination on
    // another drive previously failed AND deleted the good snapshot. Fall back
    // to copy + byte-verify so any destination volume works.
    let verified = create_verified_backup(pool, db_path).await?;
    if let Err(rename_error) = std::fs::rename(&verified, destination) {
        let copied = copy_verified_snapshot(&verified, destination)
            .map_err(|copy_error| {
                let _ = std::fs::remove_file(&verified);
                format!(
                    "Could not move verified snapshot into place ({rename_error}) and the copy fallback failed: {copy_error}"
                )
            });
        let _ = std::fs::remove_file(&verified);
        copied?;
    }
    Ok(())
}

/// FUN-23: cross-volume move fallback — byte copy followed by a full-content
/// comparison against the source, so a partially written destination is never
/// accepted as a backup.
fn copy_verified_snapshot(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::copy(source, destination)
        .map_err(|e| format!("Could not copy the snapshot to the destination volume: {e}"))?;
    let source_bytes = std::fs::read(source)
        .map_err(|e| format!("Could not re-read the snapshot for verification: {e}"))?;
    let destination_bytes = std::fs::read(destination)
        .map_err(|e| format!("Could not verify the copied snapshot: {e}"))?;
    if source_bytes != destination_bytes {
        let _ = std::fs::remove_file(destination);
        return Err("The copied snapshot does not match the source; it was removed.".to_string());
    }
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
    // SEC-02: the policy was previously parsed and handed to the UI without ever
    // being checked, so a crafted manifest could declare anything (or nothing).
    // Validating here rather than in the restore command means `validate_app_backup`
    // rejects it too, before the user is offered a restore.
    if manifest.credentials_policy != CREDENTIALS_POLICY_MACHINE_BOUND {
        return Err(engine_backup_error(
            "backup_invalid_manifest",
            format!(
                "Unsupported credentials policy '{}'; expected '{CREDENTIALS_POLICY_MACHINE_BOUND}'.",
                manifest.credentials_policy
            ),
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

/// SEC-02: Normalize a path for prefix comparison.
///
/// Deliberately textual: the paths in a backup usually do not exist yet, so
/// `canonicalize` is not an option. Windows comparisons are case-insensitive.
fn normalize_for_compare(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

/// SEC-02: True when `candidate` is `root` itself or sits underneath it.
///
/// Compares on a trailing separator so `/data/dl-evil` is not accepted as being
/// inside `/data/dl`.
fn is_within_root(candidate: &Path, root: &Path) -> bool {
    let candidate = normalize_for_compare(candidate);
    let root = normalize_for_compare(root);
    let root_trimmed = root.trim_end_matches('/');
    if root_trimmed.is_empty() {
        return false;
    }
    candidate == root_trimmed || candidate.starts_with(&format!("{root_trimmed}/"))
}

/// SEC-02: Reject a stored path that restore would later write to or delete.
fn stored_path_is_allowed(value: Option<&str>, allowed_roots: &[PathBuf]) -> bool {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        // NULL / empty is not a write target.
        return true;
    };
    let path = Path::new(value);
    // A relative path would be resolved against the process working directory
    // at download time, so it is never acceptable from an untrusted backup.
    if !path.is_absolute() {
        return false;
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }
    allowed_roots.iter().any(|root| is_within_root(path, root))
}

/// SEC-02: Record every stored path in `rows` that restore must not be allowed
/// to write to or delete.
fn collect_path_offenders(
    table: &str,
    rows: Vec<sqlx::sqlite::SqliteRow>,
    allowed_roots: &[PathBuf],
    offenders: &mut Vec<String>,
) {
    for row in rows {
        let row_id: String = row.try_get("row_id").unwrap_or_default();
        for column in ["save_dir", "temp_path", "final_path"] {
            let value: Option<String> = row.try_get(column).unwrap_or(None);
            if !stored_path_is_allowed(value.as_deref(), allowed_roots) {
                offenders.push(format!(
                    "{table}.{column} of row {row_id} ({})",
                    value.unwrap_or_default()
                ));
            }
        }
    }
}

/// SEC-02: Reject a backup whose task rows point outside the allowed roots.
///
/// Restore consumes these strings verbatim. `prepare_task_for_download` does
/// `PathBuf::from(task.temp_path)` with no re-sanitization, the engine then runs
/// `create_dir_all(parent)` and writes the file, and deleting the task later
/// feeds `final_path` straight to the file remover. Nothing re-enters
/// `unique_final_path`, so without this check a crafted `.vibe-backup` is an
/// arbitrary-write *and* arbitrary-delete primitive - and because
/// `auto_resume_on_startup` also lives in the backup, it needs no user action
/// beyond accepting the restore.
///
/// Fails closed on the entire backup instead of rewriting offending rows:
/// rewriting can collide with `idx_tasks_final_path_active` and would hand the
/// user a silently altered restore.
///
/// `allowed_roots` must come from the *live* configuration. Sourcing it from the
/// backup's own `settings.default_save_dir` would let the attacker bootstrap
/// their way around the policy.
pub async fn enforce_backup_path_policy(
    verified_db: &Path,
    allowed_roots: &[PathBuf],
) -> Result<(), String> {
    if allowed_roots.is_empty() {
        return Err(engine_backup_error(
            "backup_unsafe_paths",
            "No allowed download root is configured, so the backup cannot be validated.",
        ));
    }
    let url = format!("sqlite:{}?mode=ro", verified_db.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| {
            engine_backup_error(
                "backup_invalid_database",
                format!("Could not open the backup database for validation: {e}"),
            )
        })?;

    // Static SQL only - sqlx's injection audit rejects `format!`-built queries,
    // and keeping the strings literal preserves that guarantee here too.
    let fetched = async {
        let tasks = sqlx::query("SELECT id AS row_id, save_dir, temp_path, final_path FROM tasks")
            .fetch_all(&pool)
            .await?;
        let files =
            sqlx::query("SELECT id AS row_id, save_dir, temp_path, final_path FROM task_files")
                .fetch_all(&pool)
                .await?;
        Ok::<_, sqlx::Error>((tasks, files))
    }
    .await;
    pool.close().await;
    let (task_rows, file_rows) = fetched.map_err(|e| {
        engine_backup_error(
            "backup_invalid_database",
            format!("Could not read task paths from the backup: {e}"),
        )
    })?;

    let mut offenders: Vec<String> = Vec::new();
    collect_path_offenders("tasks", task_rows, allowed_roots, &mut offenders);
    collect_path_offenders("task_files", file_rows, allowed_roots, &mut offenders);

    if offenders.is_empty() {
        return Ok(());
    }
    // Only the first few are reported: the message reaches the UI and a hostile
    // backup could otherwise pad it arbitrarily.
    let shown = offenders
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    Err(engine_backup_error(
        "backup_unsafe_paths",
        format!(
            "This backup contains {} file path(s) outside your download folders and was rejected: {shown}",
            offenders.len()
        ),
    ))
}

pub fn pending_restore_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(PENDING_RESTORE_SUFFIX);
    PathBuf::from(path)
}

/// Apply a staged restore before opening the live pool (startup path).
pub async fn apply_pending_restore_if_any(db_path: &Path) -> Result<bool, String> {
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
    if let Err(error) = post_restore_scrub(db_path).await {
        tracing::warn!(error = %error, "post-restore settings scrub failed");
    }
    tracing::info!(
        db_path = %db_path.display(),
        "applied pending vibe-backup restore"
    );
    Ok(true)
}

/// FUN-26 + SEC-09: run right after the restored database replaced the live
/// one. A previous fix wrote `proxy_password_saved=false` into the live DB —
/// which this swap then overwrote, so the restored row claimed a password the
/// local keyring never had. Fix the flag against keyring reality here, and
/// scrub settings that must never silently execute after a restore from
/// another machine.
async fn post_restore_scrub(db_path: &Path) -> Result<(), String> {
    let connection = sqlite_connect_single(db_path).await?;
    let proxy_password_present =
        crate::proxy::load_proxy_password().is_ok_and(|value| value.is_some());
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('proxy_password_saved', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(if proxy_password_present {
        "true"
    } else {
        "false"
    })
    .execute(&connection)
    .await
    .map_err(|e| e.to_string())?;
    // SEC-09: completion commands and the ffmpeg path are machine-specific and
    // a crafted backup could smuggle an executable path in here.
    for (key, value) in [
        ("completion_action", "notify"),
        ("completion_run_command", ""),
        ("ffmpeg_path", ""),
    ] {
        sqlx::query(
            "INSERT INTO settings(key, value) VALUES(?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&connection)
        .await
        .map_err(|e| e.to_string())?;
    }
    connection.close().await;
    Ok(())
}

/// Opens a single short-lived connection for the post-restore scrub (the pool
/// for the new DB is not up yet at this point in startup).
async fn sqlite_connect_single(db_path: &Path) -> Result<sqlx::SqlitePool, String> {
    let url = format!("sqlite://{}?mode=rw", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| format!("Could not open the restored database for scrubbing: {e}"))?;
    Ok(pool)
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
