//! FUN-16 / D3: user-facing application backup and restore commands.
//! Extended for the Backup & Migration Center (feature proposal §3.7):
//! content previews, pre-restore checks, migration path remap, safe-subset
//! restore, and the post-restore report.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tokio::fs;

use crate::{
    db::{
        self, pack_backup_file, pending_restore_path, read_backup_file, snapshot_database_to_path,
        write_backup_file, BackupManifest, RestoreStagingMeta, BACKUP_FORMAT_VERSION,
        CREDENTIALS_POLICY_MACHINE_BOUND,
    },
    models::{
        backup::{
            BackupContents, BackupDiskCheck, BackupPathPolicySummary, BackupSettingsPreview,
            BackupSubsetRestoreResult, BackupSubsetSelection, RestoreReport,
        },
        AppErrorPayload,
    },
    platform, AppState,
};

/// Restore needs room for the staged pending file, the pre-restore snapshot,
/// and WAL headroom — conservatively three copies plus a fixed margin.
const RESTORE_DISK_COPIES: u64 = 3;
const RESTORE_DISK_HEADROOM_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupCreateResult {
    pub path: String,
    pub schema_version: String,
    pub credentials_policy: String,
    /// FUN-23: true when the snapshot had to be byte-copied because the
    /// destination sits on another volume and rename cannot cross devices.
    pub used_copy_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupValidateResult {
    pub path: String,
    pub schema_version: String,
    pub app_version: String,
    pub created_at: String,
    pub credentials_policy: String,
    pub database_bytes: String,
    pub contents: BackupContents,
    pub path_policy: BackupPathPolicySummary,
    pub disk: BackupDiskCheck,
    pub settings_preview: BackupSettingsPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreResult {
    pub requires_restart: bool,
    pub pre_restore_backup_path: String,
    pub pending_restore_path: String,
    pub credentials_policy: String,
    pub remapped_paths: u32,
}

#[tauri::command]
#[specta::specta]
pub async fn create_app_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    destination_path: String,
) -> Result<BackupCreateResult, String> {
    let dest = PathBuf::from(&destination_path);
    let db_path = platform::db_path(&app)?;
    let schema_version = db::current_schema_version(&state.pool).await?;
    let snapshot_path = dest.with_extension("sqlite.tmp");
    let used_copy_fallback =
        snapshot_database_to_path(&state.pool, &db_path, &snapshot_path).await?;
    let database = fs::read(&snapshot_path).await.map_err(|e| {
        backup_error(
            "backup_read_failed",
            format!("Could not read snapshot: {e}"),
        )
    })?;
    let _ = fs::remove_file(&snapshot_path).await;

    let manifest = BackupManifest {
        format: "vibe-backup".into(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        schema_version,
        created_at: chrono::Utc::now().to_rfc3339(),
        credentials_policy: CREDENTIALS_POLICY_MACHINE_BOUND.into(),
        includes_global_proxy_password: false,
        checksum_algorithm: "sha256".into(),
        checksum: String::new(),
        database_bytes: 0,
    };
    let packed = pack_backup_file(&manifest, &database)?;
    write_backup_file(&dest, &packed)?;
    Ok(BackupCreateResult {
        path: dest.to_string_lossy().to_string(),
        schema_version: schema_version.to_string(),
        credentials_policy: CREDENTIALS_POLICY_MACHINE_BOUND.to_string(),
        used_copy_fallback,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn validate_app_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<BackupValidateResult, String> {
    let path = PathBuf::from(&backup_path);
    let parsed = read_backup_file(&path)?;
    let current = db::current_schema_version(&state.pool).await?;
    let verified = db::materialize_and_verify_backup_db(
        &parsed.database,
        parsed.manifest.schema_version,
        current,
    )
    .await?;

    let result = describe_verified_backup(&app, &state, &verified, &parsed.manifest).await;
    let _ = fs::remove_file(&verified).await;
    let (contents, path_policy, disk, settings_preview) = result?;

    Ok(BackupValidateResult {
        path: path.to_string_lossy().to_string(),
        schema_version: parsed.manifest.schema_version.to_string(),
        app_version: parsed.manifest.app_version,
        created_at: parsed.manifest.created_at,
        credentials_policy: parsed.manifest.credentials_policy,
        database_bytes: parsed.manifest.database_bytes.to_string(),
        contents,
        path_policy,
        disk,
        settings_preview,
    })
}

/// Shared inspection of a materialized backup database: content counts, the
/// SEC-02 path-policy report, disk headroom, and the settings whitelist
/// preview. Runs on a read-only pool over the verified temp copy.
async fn describe_verified_backup(
    app: &AppHandle,
    state: &State<'_, AppState>,
    verified: &Path,
    parsed: &db::BackupManifest,
) -> Result<
    (
        BackupContents,
        BackupPathPolicySummary,
        BackupDiskCheck,
        BackupSettingsPreview,
    ),
    String,
> {
    let url = format!("sqlite://{}?mode=ro", verified.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|e| {
            backup_error(
                "backup_invalid_database",
                format!("Could not open the materialized backup: {e}"),
            )
        })?;
    let contents = db::count_contents(&pool).await;
    let settings_preview = db::backup_settings_preview(&pool).await;
    pool.close().await;
    let contents = contents?;
    let settings_preview = settings_preview?;

    // Report-only scan: the restore command still enforces the policy.
    let live_default_dir = crate::commands::settings::default_download_dir(app)?;
    let live_settings = db::get_settings(&state.pool, live_default_dir.clone()).await?;
    let mut allowed_roots = vec![PathBuf::from(&live_settings.default_save_dir)];
    let live_default_dir = PathBuf::from(live_default_dir);
    if !allowed_roots.contains(&live_default_dir) {
        allowed_roots.push(live_default_dir);
    }
    let scan_url = format!("sqlite://{}?mode=ro", verified.display());
    let scan_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&scan_url)
        .await
        .map_err(|e| {
            backup_error(
                "backup_invalid_database",
                format!("Could not reopen the materialized backup: {e}"),
            )
        })?;
    let scanned = db::scan_backup_path_policy(&scan_pool, &allowed_roots).await;
    scan_pool.close().await;
    let scan = scanned?;
    let violation_count = scan.offenders.len() as u32;
    let path_policy = BackupPathPolicySummary {
        violation_count,
        sample_violations: scan.offenders.iter().take(3).cloned().collect(),
        offending_save_dirs: scan.offending_save_dirs.iter().take(8).cloned().collect(),
    };

    let db_path = platform::db_path(app)?;
    let free_bytes = platform::free_disk_bytes(&db_path);
    Ok((
        contents,
        path_policy,
        BackupDiskCheck {
            free_bytes: free_bytes.map(|value| value.to_string()),
            required_bytes: restore_required_bytes(parsed.database_bytes).to_string(),
        },
        settings_preview,
    ))
}

pub(crate) fn restore_required_bytes(database_bytes: u64) -> u64 {
    database_bytes
        .saturating_mul(RESTORE_DISK_COPIES)
        .saturating_add(RESTORE_DISK_HEADROOM_BYTES)
}

#[tauri::command]
#[specta::specta]
pub async fn restore_app_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    backup_path: String,
    remap_root: Option<String>,
) -> Result<BackupRestoreResult, String> {
    // Refuse while downloads are active — restore requires a clean restart.
    {
        let downloads = state.downloads.lock().await;
        if !downloads.is_empty() {
            return Err(backup_error(
                "backup_restore_busy",
                "Pause or wait for active downloads before restoring a backup.",
            ));
        }
    }

    let path = PathBuf::from(&backup_path);
    let parsed = read_backup_file(&path)?;
    let current = db::current_schema_version(&state.pool).await?;
    let verified = db::materialize_and_verify_backup_db(
        &parsed.database,
        parsed.manifest.schema_version,
        current,
    )
    .await?;

    // Disk headroom is checked before anything is staged: restore writes the
    // pending file AND the pre-restore snapshot next to the live database.
    let db_path = platform::db_path(&app)?;
    if let Some(free) = platform::free_disk_bytes(&db_path) {
        if free < restore_required_bytes(parsed.manifest.database_bytes) {
            let _ = fs::remove_file(&verified).await;
            return Err(backup_error(
                "backup_insufficient_disk",
                format!(
                    "Not enough disk space to stage the restore: {} bytes free, {} required.",
                    free,
                    restore_required_bytes(parsed.manifest.database_bytes)
                ),
            ));
        }
    }

    // §3.7 migration remap: an explicit user choice to relocate stored paths
    // onto this machine's roots. Without it, SEC-02 stays fail-closed.
    let mut remapped_paths: u32 = 0;
    if let Some(root) = remap_root.as_deref() {
        let root_path = PathBuf::from(root);
        if !root_path.is_absolute() {
            let _ = fs::remove_file(&verified).await;
            return Err(backup_error(
                "backup_invalid_remap_root",
                "The migration target folder must be an absolute path.",
            ));
        }
        let snapshot_settings_url = format!("sqlite://{}?mode=ro", verified.display());
        let snapshot_pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&snapshot_settings_url)
            .await
            .map_err(|e| {
                backup_error(
                    "backup_invalid_database",
                    format!("Could not reopen the materialized backup: {e}"),
                )
            })?;
        let preview = db::backup_settings_preview(&snapshot_pool).await;
        snapshot_pool.close().await;
        let preview = preview?;
        let old_default = (!preview.default_save_dir.trim().is_empty())
            .then(|| PathBuf::from(&preview.default_save_dir));
        remapped_paths =
            match db::remap_backup_paths(&verified, &root_path, old_default.as_deref()).await {
                Ok(count) => count,
                Err(error) => {
                    let _ = fs::remove_file(&verified).await;
                    return Err(error);
                }
            };
    }

    // SEC-02: verify the *contents* before staging. Up to this point we have only
    // checked that the file is well-formed and internally consistent, all of
    // which an attacker controls. The allowed roots deliberately come from the
    // live configuration - reading them from the restored database would let a
    // crafted backup declare its own save dir and bootstrap past this check.
    let live_default_dir = crate::commands::settings::default_download_dir(&app)?;
    let live_settings = db::get_settings(&state.pool, live_default_dir.clone()).await?;
    let mut allowed_roots = vec![PathBuf::from(&live_settings.default_save_dir)];
    let live_default_dir = PathBuf::from(live_default_dir);
    if !allowed_roots.contains(&live_default_dir) {
        allowed_roots.push(live_default_dir);
    }
    if let Some(root_path) = remap_root.as_deref().map(PathBuf::from) {
        allowed_roots.push(root_path);
    }
    if let Err(error) = db::enforce_backup_path_policy(&verified, &allowed_roots).await {
        let _ = fs::remove_file(&verified).await;
        return Err(error);
    }
    if remapped_paths > 0 {
        // The remap rewrote every stored path; confirm the file is still a
        // coherent SQLite database before it becomes the pending restore.
        if let Err(error) = db::verify_backup_integrity(&verified).await {
            let _ = fs::remove_file(&verified).await;
            return Err(error);
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pre_restore = db_path.with_extension(format!("db.bak-{timestamp}"));
    // SEC-02: a failure here used to leak the materialized database - a full copy
    // of the backup, including encrypted credentials - in the shared temp dir.
    if let Err(error) = snapshot_database_to_path(&state.pool, &db_path, &pre_restore).await {
        let _ = fs::remove_file(&verified).await;
        return Err(error);
    }

    let pending = pending_restore_path(&db_path);
    if pending.exists() {
        let _ = fs::remove_file(&pending).await;
    }
    if let Err(error) = fs::rename(&verified, &pending).await {
        let _ = fs::remove_file(&verified).await;
        return Err(backup_error(
            "backup_restore_failed",
            format!("Could not stage restored database: {error}"),
        ));
    }
    // Consumed by the startup apply to build the post-restore report.
    if let Err(error) = db::write_staging_meta(
        &pending,
        &RestoreStagingMeta {
            pre_restore_backup_path: pre_restore.to_string_lossy().to_string(),
            backup_created_at: parsed.manifest.created_at.clone(),
            schema_version: parsed.manifest.schema_version,
        },
    ) {
        tracing::warn!(error = %error, "restore staging meta could not be written");
    }

    // FUN-26: the proxy-password flag is corrected against the LOCAL keyring
    // when the pending restore replaces the live database at next startup
    // (db::backup::post_restore_scrub). Writing the flag here was a no-op —
    // the swap overwrites this database entirely.

    Ok(BackupRestoreResult {
        requires_restart: true,
        pre_restore_backup_path: pre_restore.to_string_lossy().to_string(),
        pending_restore_path: pending.to_string_lossy().to_string(),
        credentials_policy: parsed.manifest.credentials_policy,
        remapped_paths,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn describe_backup_source(state: State<'_, AppState>) -> Result<BackupContents, String> {
    db::count_contents(&state.pool).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_backup_subset(
    app: AppHandle,
    state: State<'_, AppState>,
    backup_path: String,
    selection: BackupSubsetSelection,
) -> Result<BackupSubsetRestoreResult, String> {
    // The merge writes into the live database, so it needs the same quiet
    // period the whole-file restore requires.
    {
        let downloads = state.downloads.lock().await;
        if !downloads.is_empty() {
            return Err(backup_error(
                "backup_restore_busy",
                "Pause or wait for active downloads before restoring a backup.",
            ));
        }
    }

    let path = PathBuf::from(&backup_path);
    let parsed = read_backup_file(&path)?;
    let current = db::current_schema_version(&state.pool).await?;
    let verified = db::materialize_and_verify_backup_db(
        &parsed.database,
        parsed.manifest.schema_version,
        current,
    )
    .await?;

    // SEC-02: the tasks subset copies stored paths into the live database, so
    // the same live-roots policy as the whole-file restore gates it here.
    // Roots come from the live configuration, never from the backup itself.
    let mut allowed_roots = Vec::new();
    if selection.tasks {
        let live_default_dir = crate::commands::settings::default_download_dir(&app)?;
        let live_settings = db::get_settings(&state.pool, live_default_dir.clone()).await?;
        allowed_roots.push(PathBuf::from(&live_settings.default_save_dir));
        let live_default_dir = PathBuf::from(live_default_dir);
        if !allowed_roots.contains(&live_default_dir) {
            allowed_roots.push(live_default_dir);
        }
    }

    let db_path = platform::db_path(&app)?;
    let result = db::restore_subset(&db_path, &verified, selection, &allowed_roots).await;
    let _ = fs::remove_file(&verified).await;
    let restored = result?;
    if restored.tasks_inserted > 0 {
        // The merge bypasses the scheduler, so no per-task events fire; a
        // full queue-changed refresh makes the new tasks visible without a
        // manual reload (the frontend merges the fresh page into its store).
        crate::events::emit_queue_changed(&app);
    }
    Ok(restored)
}

#[tauri::command]
#[specta::specta]
pub async fn get_last_restore_report(app: AppHandle) -> Result<Option<RestoreReport>, String> {
    let db_path = platform::db_path(&app)?;
    Ok(db::read_restore_report(&db_path))
}

#[tauri::command]
#[specta::specta]
pub async fn dismiss_restore_report(app: AppHandle) -> Result<bool, String> {
    let db_path = platform::db_path(&app)?;
    Ok(db::dismiss_restore_report(&db_path))
}

fn backup_error(code: &str, message: impl Into<String>) -> String {
    AppErrorPayload::new(code, message, false, vec!["check_url"]).command_error()
}
