//! FUN-16 / D3: versioned application database backup format (`.vibe-backup`).

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
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
/// Returns whether the cross-volume copy fallback was used (FUN-23) so the
/// export result can surface it as a diagnostic.
pub async fn snapshot_database_to_path(
    pool: &SqlitePool,
    db_path: &Path,
    destination: &Path,
) -> Result<bool, String> {
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
        return Ok(true);
    }
    Ok(false)
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
    write_backup_file_impl(path, bytes, BackupWriteOptions::default())
}

#[derive(Default)]
struct BackupWriteOptions {
    #[cfg(debug_assertions)]
    fail_after: Option<(usize, std::io::ErrorKind)>,
    #[cfg(debug_assertions)]
    fail_sync: bool,
    #[cfg(debug_assertions)]
    before_publish: Option<Box<dyn FnOnce() + Send>>,
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn write_backup_file_for_test(
    path: &Path,
    bytes: &[u8],
    fail_after: Option<(usize, std::io::ErrorKind)>,
    fail_sync: bool,
    before_publish: Option<Box<dyn FnOnce() + Send>>,
) -> Result<(), String> {
    write_backup_file_impl(
        path,
        bytes,
        BackupWriteOptions {
            fail_after,
            fail_sync,
            before_publish,
        },
    )
}

/// Write a backup without exposing a partially written replacement to readers.
/// The temporary file lives beside the destination so the final rename stays
/// on one volume and is the only operation that changes the visible backup.
fn write_backup_file_impl(
    path: &Path,
    bytes: &[u8],
    options: BackupWriteOptions,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            engine_backup_error(
                "backup_write_failed",
                format!("Could not create backup folder: {e}"),
            )
        })?;
    }
    // Reject malformed payloads before touching an existing destination.
    parse_backup_bytes(bytes)?;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("backup.vibe-backup");
    let mut temp_path = None;
    let mut file = None;
    for _ in 0..16 {
        let token = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), token));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(handle) => {
                temp_path = Some(candidate);
                file = Some(handle);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(engine_backup_error(
                    "backup_write_failed",
                    format!("Could not create backup staging file: {error}"),
                ));
            }
        }
    }
    let temp_path = temp_path.ok_or_else(|| {
        engine_backup_error(
            "backup_write_failed",
            "Could not allocate a unique backup staging file.",
        )
    })?;
    let mut file = file.expect("staging handle is present when staging path is present");
    let write_result = (|| {
        #[cfg(debug_assertions)]
        let mut written = 0usize;
        for chunk in bytes.chunks(64 * 1024) {
            #[cfg(debug_assertions)]
            if let Some((limit, kind)) = options.fail_after {
                if written + chunk.len() > limit {
                    file.write_all(&chunk[..limit.saturating_sub(written)])?;
                    return Err(std::io::Error::new(
                        kind,
                        "injected backup write interruption",
                    ));
                }
            }
            file.write_all(chunk)?;
            #[cfg(debug_assertions)]
            {
                written += chunk.len();
            }
        }
        #[cfg(debug_assertions)]
        if options.fail_sync {
            return Err(std::io::Error::other("injected backup sync failure"));
        }
        file.sync_all()
    })();
    if let Err(error) = write_result {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(engine_backup_error(
            "backup_write_failed",
            format!("Could not write backup file: {error}"),
        ));
    }
    drop(file);

    if let Err(error) = read_backup_file(&temp_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    #[cfg(debug_assertions)]
    if let Some(hook) = options.before_publish {
        hook();
    }
    #[cfg(not(debug_assertions))]
    let _ = options;
    if let Err(error) = atomic_replace_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(engine_backup_error(
            "backup_write_failed",
            format!("Could not publish backup file: {error}"),
        ));
    }
    sync_parent_directory(parent);
    Ok(())
}

fn atomic_replace_file(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source_wide = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination_wide = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // MOVEFILE_REPLACE_EXISTING performs the replacement as one filesystem
        // operation, while WRITE_THROUGH asks Windows to flush the metadata.
        unsafe {
            MoveFileExW(
                PCWSTR(source_wide.as_ptr()),
                PCWSTR(destination_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(source, destination)
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) {
    #[cfg(unix)]
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) {}

/// Materialize backup database bytes to a temp path and verify integrity + migrations.
///
/// `staging_dir` picks where the verified file lands. Pass the live database's
/// parent directory on the restore path (ARC-53): the pending-restore rename
/// is a filesystem move, which fails across volumes — staging beside the DB
/// keeps it same-volume regardless of where the OS temp dir lives. `None`
/// falls back to the OS temp dir for read-only consumers (preview / subset
/// restore) that never rename the file.
pub async fn materialize_and_verify_backup_db(
    database: &[u8],
    schema_version: i64,
    current_schema: i64,
    staging_dir: Option<&Path>,
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
    let dir = staging_dir
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("vibe-backup-verify-{id}.sqlite"));
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
    let structure = validate_backup_secrets(&pool).await;
    pool.close().await;
    if let Err(error) = structure {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

/// Inspect only encoding, nonce size and tag size. Never decrypt backup
/// credentials or include stored values in diagnostics from an untrusted DB.
pub async fn validate_backup_secrets(pool: &SqlitePool) -> Result<(), String> {
    for (table, column, query) in [
        (
            "task_request_headers",
            "headers_ciphertext",
            "SELECT rowid, headers_ciphertext, nonce FROM task_request_headers",
        ),
        (
            "task_credentials",
            "credentials_ciphertext",
            "SELECT rowid, credentials_ciphertext, nonce FROM task_credentials",
        ),
        (
            "task_proxy_settings",
            "proxy_password_ciphertext",
            "SELECT rowid, proxy_password_ciphertext, nonce FROM task_proxy_settings",
        ),
    ] {
        let rows = sqlx::query(query).fetch_all(pool).await.map_err(|_| {
            engine_backup_error(
                "backup_invalid_database",
                format!("Could not read {table}."),
            )
        })?;
        for row in rows {
            let id: i64 = row.get("rowid");
            let ciphertext: Option<String> = row.try_get(column).map_err(|_| {
                engine_backup_error(
                    "backup_invalid_database",
                    format!("Invalid {table} record {id} field {column}."),
                )
            })?;
            let nonce: Option<String> = row.try_get("nonce").map_err(|_| {
                engine_backup_error(
                    "backup_invalid_database",
                    format!("Invalid {table} record {id} field nonce."),
                )
            })?;
            let invalid_field = match (ciphertext.as_deref(), nonce.as_deref()) {
                (None, None) => None,
                (Some(ct), Some(nonce)) => {
                    crate::secure_headers::validate_secret_structure(ct, nonce)
                        .err()
                        .map(|field| if field == "nonce" { "nonce" } else { column })
                }
                (None, Some(_)) => Some(column),
                (Some(_), None) => Some("nonce"),
            };
            if let Some(field) = invalid_field {
                return Err(engine_backup_error(
                    "backup_invalid_database",
                    format!("Invalid {table} record {id} field {field}."),
                ));
            }
        }
    }
    Ok(())
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
    rows: &[sqlx::sqlite::SqliteRow],
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

/// SEC-02 path-policy scan in reporting form. `validate_app_backup` feeds this
/// to the pre-restore check panel; the restore command itself still fails
/// closed through [`enforce_backup_path_policy`].
#[derive(Debug, Default)]
pub struct BackupPathScan {
    pub offenders: Vec<String>,
    /// Distinct task save dirs outside the allowed roots, in first-seen order.
    pub offending_save_dirs: Vec<String>,
}

/// Scan a materialized backup database for stored paths outside `roots`.
/// Read-only: never rewrites rows, so the result can be shown to the user
/// without consuming the backup.
pub async fn scan_backup_path_policy(
    pool: &SqlitePool,
    allowed_roots: &[PathBuf],
) -> Result<BackupPathScan, String> {
    // Static SQL only - sqlx's injection audit rejects `format!`-built queries,
    // and keeping the strings literal preserves that guarantee here too.
    let fetched = async {
        let tasks = sqlx::query("SELECT id AS row_id, save_dir, temp_path, final_path FROM tasks")
            .fetch_all(pool)
            .await?;
        let files =
            sqlx::query("SELECT id AS row_id, save_dir, temp_path, final_path FROM task_files")
                .fetch_all(pool)
                .await?;
        Ok::<_, sqlx::Error>((tasks, files))
    }
    .await;
    let (task_rows, file_rows) = fetched.map_err(|e| {
        engine_backup_error(
            "backup_invalid_database",
            format!("Could not read task paths from the backup: {e}"),
        )
    })?;

    let mut scan = BackupPathScan::default();
    collect_path_offenders("tasks", &task_rows, allowed_roots, &mut scan.offenders);
    collect_path_offenders("task_files", &file_rows, allowed_roots, &mut scan.offenders);

    // Distinct disallowed save dirs give the migration UI something concrete
    // to show ("these folders will move"). Save_dir is NOT NULL in the schema.
    for row in &task_rows {
        let save_dir: String = row.try_get("save_dir").unwrap_or_default();
        if !save_dir.is_empty()
            && !scan.offending_save_dirs.iter().any(|d| d == &save_dir)
            && !stored_path_is_allowed(Some(&save_dir), allowed_roots)
        {
            scan.offending_save_dirs.push(save_dir);
        }
    }
    Ok(scan)
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
    let scan = scan_backup_path_policy(&pool, allowed_roots).await;
    pool.close().await;
    let scan = scan?;

    if scan.offenders.is_empty() {
        return Ok(());
    }
    // Only the first few are reported: the message reaches the UI and a hostile
    // backup could otherwise pad it arbitrarily.
    let shown = scan
        .offenders
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    Err(engine_backup_error(
        "backup_unsafe_paths",
        format!(
            "This backup contains {} file path(s) outside your download folders and was rejected: {shown}",
            scan.offenders.len()
        ),
    ))
}

/// Subdirectory under a migration remap root where files that lived outside
/// the old default save dir are relocated.
const REMAP_MIGRATED_DIR: &str = "migrated";

/// Rewrite every stored task path in a materialized backup so restore can
/// proceed on a machine whose download roots differ (cross-machine migration).
///
/// Rule: paths under `old_default` keep their relative structure under
/// `new_root`; everything else is relocated to `new_root/migrated/<file_name>`
/// — collision-free for distinct file names, and disambiguated with a numeric
/// suffix when the unique `final_path` index would otherwise reject the row.
///
/// This is the only sanctioned escape hatch from the SEC-02 fail-closed
/// policy, and it must stay an explicit user choice: the caller passes
/// `remap_root` and the rewritten database is re-verified against the new
/// roots afterwards. Returns the number of rewritten path values.
pub async fn remap_backup_paths(
    verified_db: &Path,
    new_root: &Path,
    old_default: Option<&Path>,
) -> Result<u32, String> {
    if !new_root.is_absolute() {
        return Err(engine_backup_error(
            "backup_invalid_remap_root",
            "The migration target folder must be an absolute path.",
        ));
    }
    let url = format!("sqlite:{}?mode=rw", verified_db.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| {
            engine_backup_error(
                "backup_invalid_database",
                format!("Could not open the backup database for remapping: {e}"),
            )
        })?;
    let result = remap_in_pool(&pool, new_root, old_default).await;
    pool.close().await;
    result
}

async fn remap_in_pool(
    pool: &SqlitePool,
    new_root: &Path,
    old_default: Option<&Path>,
) -> Result<u32, String> {
    let fetched = async {
        let tasks = sqlx::query("SELECT id, save_dir, temp_path, final_path FROM tasks")
            .fetch_all(pool)
            .await?;
        let files = sqlx::query("SELECT id, save_dir, temp_path, final_path FROM task_files")
            .fetch_all(pool)
            .await?;
        Ok::<_, sqlx::Error>((tasks, files))
    }
    .await;
    let (task_rows, file_rows) = fetched.map_err(|e| {
        engine_backup_error(
            "backup_invalid_database",
            format!("Could not read task paths for remapping: {e}"),
        )
    })?;

    let mut remapped: u32 = 0;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Could not begin remap transaction: {e}"))?;

    // Static SQL per table: sqlx's injection audit requires literal query
    // strings, and both statements share the exact same column shape.
    remapped += remap_rows(
        &mut tx,
        &task_rows,
        "UPDATE tasks SET save_dir = ?, temp_path = ?, final_path = ? WHERE id = ?",
        new_root,
        old_default,
    )
    .await?;
    remapped += remap_rows(
        &mut tx,
        &file_rows,
        "UPDATE task_files SET save_dir = ?, temp_path = ?, final_path = ? WHERE id = ?",
        new_root,
        old_default,
    )
    .await?;

    sqlx::query("UPDATE settings SET value = ? WHERE key = 'default_save_dir'")
        .bind(new_root.to_string_lossy().to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Could not remap default_save_dir: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("Could not commit path remap: {e}"))?;
    Ok(remapped)
}

/// Rewrite one table's path columns inside the remap transaction. Rows whose
/// values are already acceptable (empty, relative, or unmapped) are skipped.
async fn remap_rows(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    rows: &[sqlx::sqlite::SqliteRow],
    update_sql: &'static str,
    new_root: &Path,
    old_default: Option<&Path>,
) -> Result<u32, String> {
    let mut remapped: u32 = 0;
    for row in rows {
        let row_id: String = row.try_get("id").unwrap_or_default();
        let save_dir: String = row.try_get("save_dir").unwrap_or_default();
        let temp_path: Option<String> = row.try_get("temp_path").unwrap_or(None);
        let final_path: Option<String> = row.try_get("final_path").unwrap_or(None);

        let new_save = remap_path_value(&save_dir, new_root, old_default);
        let new_temp = temp_path
            .as_deref()
            .and_then(|value| remap_path_value(value, new_root, old_default));
        let new_final = final_path
            .as_deref()
            .and_then(|value| remap_path_value(value, new_root, old_default));
        // A computed value equal to the stored one is NOT a remap: when the
        // chosen root equals the old default, the whole snapshot is a no-op
        // and must report zero changes instead of rewriting identical text.
        let save_changed = new_save.as_deref().is_some_and(|value| value != save_dir);
        let temp_changed = new_temp
            .as_deref()
            .is_some_and(|value| Some(value) != temp_path.as_deref());
        let final_changed = new_final
            .as_deref()
            .is_some_and(|value| Some(value) != final_path.as_deref());
        let changed = save_changed as u32 + temp_changed as u32 + final_changed as u32;
        if changed == 0 {
            continue;
        }
        let mut final_value = new_final.clone();
        // The unique index on tasks.final_path covers every rewritten row;
        // two migrated files with the same name would collide, so suffix the
        // file name until the update sticks (bounded attempts).
        let mut updated = false;
        for attempt in 0..100u32 {
            let result = sqlx::query(update_sql)
                .bind(new_save.clone().unwrap_or_else(|| save_dir.clone()))
                .bind(new_temp.clone().or_else(|| temp_path.clone()))
                .bind(final_value.clone().or_else(|| final_path.clone()))
                .bind(&row_id)
                .execute(&mut **tx)
                .await;
            match result {
                Ok(_) => {
                    updated = true;
                    break;
                }
                Err(error) if is_unique_violation(&error) && new_final.is_some() => {
                    let path = Path::new(new_final.as_deref().unwrap_or_default());
                    final_value = Some(suffixed_path(path, attempt + 2));
                }
                Err(error) => {
                    return Err(engine_backup_error(
                        "backup_remap_failed",
                        format!("Could not rewrite stored path for row {row_id}: {error}"),
                    ));
                }
            }
        }
        if !updated {
            return Err(engine_backup_error(
                "backup_remap_failed",
                format!("Could not find a collision-free path for row {row_id}."),
            ));
        }
        remapped += changed;
    }
    Ok(remapped)
}

/// True when the SQLite error is the unique-constraint failure our collision
/// fallback knows how to resolve.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// Append `-migrated-<n>` before the extension so `a.bin` becomes
/// `a-migrated-2.bin` on the second collision.
fn suffixed_path(path: &Path, n: u32) -> String {
    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let (stem, ext) = match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (file.clone(), String::new()),
    };
    path.with_file_name(format!("{stem}-migrated-{n}{ext}"))
        .to_string_lossy()
        .to_string()
}

/// Compute the replacement for one stored path, or `None` when the value must
/// stay untouched (empty, relative — SEC-02 rejects those later — or when the
/// backup predates any configured default dir and the file name would be
/// ambiguous). Membership under `old_default` is judged on the normalized
/// (separator/case-folded) text, while the rewritten value is built from the
/// original string so non-ASCII names survive byte-for-byte.
fn remap_path_value(value: &str, new_root: &Path, old_default: Option<&Path>) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return None;
    }
    if let Some(old) = old_default {
        if is_within_root(path, old) {
            let norm_old = normalize_for_compare(old);
            let norm_old = norm_old.trim_end_matches('/');
            let norm_value = normalize_for_compare(path);
            if norm_value.as_bytes().get(norm_old.len()) == Some(&b'/') {
                let relative = &trimmed[norm_old.len() + 1..];
                return Some(new_root.join(relative).to_string_lossy().to_string());
            }
            // The value IS the old default root itself.
            return Some(new_root.to_string_lossy().to_string());
        }
    }
    let file_name = path.file_name()?;
    Some(
        new_root
            .join(REMAP_MIGRATED_DIR)
            .join(file_name)
            .to_string_lossy()
            .to_string(),
    )
}

/// Read the machine-relevant settings rows from a backup snapshot so the
/// pre-restore panel can show what the SEC-09 scrub will reset.
pub async fn backup_settings_preview(
    pool: &SqlitePool,
) -> Result<crate::models::backup::BackupSettingsPreview, String> {
    use crate::models::backup::BackupSettingsPreview;

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("Could not read backup settings: {e}"))?;
    let ffmpeg = read_optional_setting(&mut conn, "ffmpeg_path").await?;
    let completion_action = read_optional_setting(&mut conn, "completion_action").await?;
    let proxy_saved = read_optional_setting(&mut conn, "proxy_password_saved").await?;
    let default_save_dir = read_optional_setting(&mut conn, "default_save_dir").await?;
    Ok(BackupSettingsPreview {
        ffmpeg_configured: ffmpeg.is_some_and(|value| !value.trim().is_empty()),
        completion_action: completion_action.unwrap_or_else(|| "notify".to_string()),
        proxy_password_saved: proxy_saved.is_some_and(|value| value == "true"),
        default_save_dir: default_save_dir.unwrap_or_default(),
    })
}

/// Re-run `PRAGMA integrity_check` on a materialized backup database, e.g.
/// after a path remap rewrote its rows.
pub async fn verify_backup_integrity(verified_db: &Path) -> Result<(), String> {
    let url = format!("sqlite:{}?mode=ro", verified_db.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| {
            engine_backup_error(
                "backup_invalid_database",
                format!("Could not reopen the backup database: {e}"),
            )
        })?;
    let integrity: Result<String, _> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await;
    pool.close().await;
    match integrity {
        Ok(value) if value == "ok" => Ok(()),
        Ok(value) => Err(engine_backup_error(
            "backup_invalid_database",
            format!("Backup integrity check failed: {value}"),
        )),
        Err(e) => Err(engine_backup_error(
            "backup_invalid_database",
            format!("Backup integrity check failed: {e}"),
        )),
    }
}

pub fn pending_restore_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(PENDING_RESTORE_SUFFIX);
    PathBuf::from(path)
}

/// Replace live DB + sidecars with the pending restored file.
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
    let scrub = match post_restore_scrub(db_path).await {
        Ok(observation) => Some(observation),
        Err(error) => {
            tracing::warn!(error = %error, "post-restore settings scrub failed");
            None
        }
    };
    // §3.7: the Backup Center surfaces "what to reconfigure" after the swap.
    // Best-effort — a report failure must never block startup.
    if let Err(error) =
        super::restore_report::write_report_after_swap(db_path, scrub.as_ref()).await
    {
        tracing::warn!(error = %error, "post-restore report could not be written");
    }
    tracing::info!(
        db_path = %db_path.display(),
        "applied pending vibe-backup restore"
    );
    Ok(true)
}

/// What the pre-restore settings looked like, captured before the
/// machine-bound scrub rewrites them; feeds the post-restore report.
#[derive(Debug, Default, Clone)]
pub struct RestoreScrubObservation {
    pub proxy_password_saved_in_backup: Option<bool>,
    pub ffmpeg_was_configured: bool,
    pub completion_action_was: Option<String>,
}

/// FUN-26 + SEC-09: run right after the restored database replaced the live
/// one. A previous fix wrote `proxy_password_saved=false` into the live DB —
/// which this swap then overwrote, so the restored row claimed a password the
/// local keyring never had. Fix the flag against keyring reality here, and
/// scrub settings that must never silently execute after a restore from
/// another machine.
async fn post_restore_scrub(db_path: &Path) -> Result<RestoreScrubObservation, String> {
    let connection = sqlite_connect_single(db_path).await?;
    let proxy_password_present =
        crate::proxy::load_proxy_password().is_ok_and(|value| value.is_some());
    let mut conn = connection
        .acquire()
        .await
        .map_err(|e| format!("Could not acquire the restored database connection: {e}"))?;
    let result = run_restore_scrub_core(&mut conn, proxy_password_present).await;
    drop(conn);
    connection.close().await;
    result
}

/// Scrub core operating on an open connection, shared by the startup swap
/// path and the settings-subset restore. Returns the pre-scrub observation.
pub(crate) async fn run_restore_scrub_core(
    connection: &mut sqlx::SqliteConnection,
    proxy_password_present: bool,
) -> Result<RestoreScrubObservation, String> {
    let observation = RestoreScrubObservation {
        proxy_password_saved_in_backup: read_optional_setting(connection, "proxy_password_saved")
            .await?
            .map(|value| value == "true"),
        ffmpeg_was_configured: read_optional_setting(connection, "ffmpeg_path")
            .await?
            .is_some_and(|value| !value.trim().is_empty()),
        completion_action_was: read_optional_setting(connection, "completion_action").await?,
    };
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('proxy_password_saved', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(if proxy_password_present {
        "true"
    } else {
        "false"
    })
    .execute(&mut *connection)
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
        .execute(&mut *connection)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(observation)
}

async fn read_optional_setting(
    connection: &mut sqlx::SqliteConnection,
    key: &str,
) -> Result<Option<String>, String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(connection)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|(value,)| value))
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

    #[test]
    fn interrupted_backup_write_preserves_previous_file() {
        let directory = std::env::temp_dir().join(format!(
            "vibe-backup-write-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let destination = directory.join("state.vibe-backup");
        let original = b"previous-good-backup";
        std::fs::write(&destination, original).expect("write original");

        let database = b"database";
        let manifest = BackupManifest {
            format: "vibe-backup".into(),
            format_version: BACKUP_FORMAT_VERSION,
            app_version: "0.5.0".into(),
            schema_version: 1,
            created_at: "2026-09-21T00:00:00Z".into(),
            credentials_policy: CREDENTIALS_POLICY_MACHINE_BOUND.into(),
            includes_global_proxy_password: false,
            checksum_algorithm: "sha256".into(),
            checksum: String::new(),
            database_bytes: 0,
        };
        let packed = pack_backup_file(&manifest, database).expect("pack");
        let error = write_backup_file_impl(
            &destination,
            &packed,
            BackupWriteOptions {
                fail_after: Some((0, std::io::ErrorKind::WriteZero)),
                ..Default::default()
            },
        )
        .expect_err("injected write must fail");
        assert!(error.contains("backup_write_failed"));
        assert_eq!(
            std::fs::read(&destination).expect("read original"),
            original
        );
        assert_eq!(
            std::fs::read_dir(&directory)
                .expect("list directory")
                .count(),
            1,
            "failed staging file must be removed"
        );

        write_backup_file(&destination, &packed).expect("publish replacement");
        assert_eq!(
            read_backup_file(&destination)
                .expect("read replacement")
                .database,
            database
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
