//! FUN-16 / D3: user-facing application backup and restore commands.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tokio::fs;

use crate::{
    db::{
        self, pack_backup_file, pending_restore_path, read_backup_file, snapshot_database_to_path,
        write_backup_file, BackupManifest, BACKUP_FORMAT_VERSION, CREDENTIALS_POLICY_MACHINE_BOUND,
    },
    models::AppErrorPayload,
    platform, AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupCreateResult {
    pub path: String,
    pub schema_version: String,
    pub credentials_policy: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreResult {
    pub requires_restart: bool,
    pub pre_restore_backup_path: String,
    pub pending_restore_path: String,
    pub credentials_policy: String,
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
    })
}

#[tauri::command]
#[specta::specta]
pub async fn validate_app_backup(
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
    let _ = fs::remove_file(&verified).await;
    Ok(BackupValidateResult {
        path: path.to_string_lossy().to_string(),
        schema_version: parsed.manifest.schema_version.to_string(),
        app_version: parsed.manifest.app_version,
        created_at: parsed.manifest.created_at,
        credentials_policy: parsed.manifest.credentials_policy,
        database_bytes: parsed.manifest.database_bytes.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn restore_app_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    backup_path: String,
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

    let db_path = platform::db_path(&app)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pre_restore = db_path.with_extension(format!("db.bak-{timestamp}"));
    snapshot_database_to_path(&state.pool, &db_path, &pre_restore).await?;

    let pending = pending_restore_path(&db_path);
    if pending.exists() {
        let _ = fs::remove_file(&pending).await;
    }
    fs::rename(&verified, &pending).await.map_err(|e| {
        backup_error(
            "backup_restore_failed",
            format!("Could not stage restored database: {e}"),
        )
    })?;

    // Global proxy password stays in the OS keyring and is not in the backup.
    // Clear the saved flag so the UI does not pretend the password is present.
    let _ = sqlx::query(
        "INSERT INTO settings(key, value) VALUES('proxy_password_saved', 'false')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(&state.pool)
    .await;

    Ok(BackupRestoreResult {
        requires_restart: true,
        pre_restore_backup_path: pre_restore.to_string_lossy().to_string(),
        pending_restore_path: pending.to_string_lossy().to_string(),
        credentials_policy: parsed.manifest.credentials_policy,
    })
}

fn backup_error(code: &str, message: impl Into<String>) -> String {
    AppErrorPayload::new(code, message, false, vec!["check_url"]).command_error()
}
