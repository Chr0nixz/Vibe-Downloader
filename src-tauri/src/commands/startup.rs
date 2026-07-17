use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::{db, platform};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    pub mode: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub database_path: Option<String>,
    pub backup_path: Option<String>,
    pub backup_verified: bool,
    pub can_reset: bool,
}

#[derive(Debug, Clone)]
struct RecoveryContext {
    database_path: PathBuf,
    backup_path: Option<PathBuf>,
    backup_verified: bool,
}

#[derive(Debug)]
struct StartupSnapshot {
    status: StartupStatus,
    recovery: Option<RecoveryContext>,
}

#[derive(Debug)]
pub struct StartupState {
    snapshot: Mutex<StartupSnapshot>,
}

impl StartupState {
    pub fn initializing() -> Self {
        Self {
            snapshot: Mutex::new(StartupSnapshot {
                status: StartupStatus {
                    mode: "initializing".to_string(),
                    reason: None,
                    message: None,
                    database_path: None,
                    backup_path: None,
                    backup_verified: false,
                    can_reset: false,
                },
                recovery: None,
            }),
        }
    }

    pub fn set_ready(&self) {
        let mut snapshot = self.lock();
        snapshot.status = StartupStatus {
            mode: "ready".to_string(),
            reason: None,
            message: None,
            database_path: None,
            backup_path: None,
            backup_verified: false,
            can_reset: false,
        };
        snapshot.recovery = None;
    }

    pub fn set_recovery(&self, recovery: db::DatabaseRecovery) {
        let mut snapshot = self.lock();
        snapshot.status = StartupStatus {
            mode: "database_recovery_required".to_string(),
            reason: Some(recovery.reason.clone()),
            message: Some(recovery.message.clone()),
            database_path: Some(recovery.database_path.to_string_lossy().to_string()),
            backup_path: recovery
                .backup_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            backup_verified: recovery.backup_verified,
            can_reset: recovery.backup_verified,
        };
        snapshot.recovery = Some(RecoveryContext {
            database_path: recovery.database_path,
            backup_path: recovery.backup_path,
            backup_verified: recovery.backup_verified,
        });
    }

    fn lock(&self) -> MutexGuard<'_, StartupSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_startup_status(state: State<'_, StartupState>) -> Result<StartupStatus, String> {
    Ok(state.lock().status.clone())
}

#[tauri::command]
#[specta::specta]
pub async fn open_database_recovery_folder(state: State<'_, StartupState>) -> Result<(), String> {
    let recovery = state
        .lock()
        .recovery
        .clone()
        .ok_or_else(|| "Database recovery is not active.".to_string())?;
    let path = recovery
        .backup_path
        .as_deref()
        .unwrap_or(&recovery.database_path);
    let folder = path.parent().unwrap_or(path);
    platform::open_path(folder)
}

#[tauri::command]
#[specta::specta]
pub async fn reset_database_for_recovery(state: State<'_, StartupState>) -> Result<(), String> {
    let recovery = state
        .lock()
        .recovery
        .clone()
        .ok_or_else(|| "Database recovery is not active.".to_string())?;
    if !recovery.backup_verified
        || !recovery
            .backup_path
            .as_ref()
            .is_some_and(|path| path.is_file())
    {
        return Err("A verified database backup is required before reset.".to_string());
    }
    db::reset_database_files(&recovery.database_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_is_disabled_without_a_verified_backup() {
        let state = StartupState::initializing();
        state.set_recovery(db::DatabaseRecovery {
            reason: "migration_dirty".to_string(),
            message: "dirty".to_string(),
            database_path: PathBuf::from("database.sqlite"),
            backup_path: None,
            backup_verified: false,
        });
        let status = state.lock().status.clone();
        assert!(!status.can_reset);
        assert!(!status.backup_verified);
    }
}
