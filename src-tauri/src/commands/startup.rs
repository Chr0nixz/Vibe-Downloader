use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{db, platform};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    pub mode: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub code: Option<String>,
    pub database_path: Option<String>,
    pub backup_path: Option<String>,
    pub backup_verified: bool,
    pub can_reset: bool,
    pub log_path: Option<String>,
    pub data_path: Option<String>,
}

#[derive(Debug, Clone)]
struct RecoveryContext {
    database_path: PathBuf,
    backup_path: Option<PathBuf>,
    backup_verified: bool,
}

#[derive(Debug, Default)]
struct StartupProgress {
    /// True while `run_startup_init` is executing (blocks overlapping retries).
    init_in_flight: bool,
    app_state_managed: bool,
    clipboard_started: bool,
    browser_bridge_started: bool,
    scheduler_started: bool,
    monitors_started: bool,
    tray_created: bool,
    floating_synced: bool,
}

#[derive(Debug)]
struct StartupSnapshot {
    status: StartupStatus,
    recovery: Option<RecoveryContext>,
}

#[derive(Debug)]
pub struct StartupState {
    snapshot: Mutex<StartupSnapshot>,
    progress: Mutex<StartupProgress>,
}

impl StartupState {
    pub fn initializing() -> Self {
        Self {
            snapshot: Mutex::new(StartupSnapshot {
                status: blank_status("initializing"),
                recovery: None,
            }),
            progress: Mutex::new(StartupProgress::default()),
        }
    }

    pub fn current_mode(&self) -> String {
        self.lock_snapshot().status.mode.clone()
    }

    pub fn set_ready(&self) {
        let mut snapshot = self.lock_snapshot();
        snapshot.status = blank_status("ready");
        snapshot.recovery = None;
    }

    pub fn set_recovery(&self, recovery: db::DatabaseRecovery) {
        let mut snapshot = self.lock_snapshot();
        snapshot.status = StartupStatus {
            mode: "database_recovery_required".to_string(),
            reason: Some(recovery.reason.clone()),
            message: Some(recovery.message.clone()),
            code: Some(recovery.reason.clone()),
            database_path: Some(recovery.database_path.to_string_lossy().to_string()),
            backup_path: recovery
                .backup_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            backup_verified: recovery.backup_verified,
            can_reset: recovery.backup_verified,
            log_path: diagnostic_log_path(),
            data_path: Some(
                recovery
                    .database_path
                    .parent()
                    .unwrap_or(recovery.database_path.as_path())
                    .to_string_lossy()
                    .to_string(),
            ),
        };
        snapshot.recovery = Some(RecoveryContext {
            database_path: recovery.database_path,
            backup_path: recovery.backup_path,
            backup_verified: recovery.backup_verified,
        });
    }

    /// UX-01: surface ordinary init failures so the UI can offer Retry.
    pub fn set_failed(&self, code: impl Into<String>, message: impl Into<String>) {
        let code = code.into();
        let message = message.into();
        let mut snapshot = self.lock_snapshot();
        // Never overwrite an active recovery gate or a ready app.
        if snapshot.status.mode == "database_recovery_required" || snapshot.status.mode == "ready" {
            return;
        }
        snapshot.status = StartupStatus {
            mode: "startup_failed".to_string(),
            reason: Some(code.clone()),
            message: Some(message),
            code: Some(code),
            database_path: None,
            backup_path: None,
            backup_verified: false,
            can_reset: false,
            log_path: diagnostic_log_path(),
            data_path: diagnostic_data_path_hint(),
        };
        snapshot.recovery = None;
    }

    /// Mark init as in-flight. Returns false if another init is already running.
    pub fn try_begin_init(&self) -> bool {
        let mut progress = self.lock_progress();
        if progress.init_in_flight {
            return false;
        }
        progress.init_in_flight = true;
        true
    }

    pub fn end_init(&self) {
        self.lock_progress().init_in_flight = false;
    }

    pub fn app_state_managed(&self) -> bool {
        self.lock_progress().app_state_managed
    }

    pub fn mark_app_state_managed(&self) {
        self.lock_progress().app_state_managed = true;
    }

    pub fn clipboard_started(&self) -> bool {
        self.lock_progress().clipboard_started
    }

    pub fn mark_clipboard_started(&self) {
        self.lock_progress().clipboard_started = true;
    }

    pub fn browser_bridge_started(&self) -> bool {
        self.lock_progress().browser_bridge_started
    }

    pub fn mark_browser_bridge_started(&self) {
        self.lock_progress().browser_bridge_started = true;
    }

    pub fn scheduler_started(&self) -> bool {
        self.lock_progress().scheduler_started
    }

    pub fn mark_scheduler_started(&self) {
        self.lock_progress().scheduler_started = true;
    }

    pub fn monitors_started(&self) -> bool {
        self.lock_progress().monitors_started
    }

    pub fn mark_monitors_started(&self) {
        self.lock_progress().monitors_started = true;
    }

    pub fn tray_created(&self) -> bool {
        self.lock_progress().tray_created
    }

    pub fn mark_tray_created(&self) {
        self.lock_progress().tray_created = true;
    }

    pub fn floating_synced(&self) -> bool {
        self.lock_progress().floating_synced
    }

    pub fn mark_floating_synced(&self) {
        self.lock_progress().floating_synced = true;
    }

    /// Transition `startup_failed` → `initializing` for a user Retry.
    pub fn begin_retry(&self) -> Result<(), String> {
        // Drop the progress guard before taking the snapshot lock to keep
        // lock ordering consistent with other StartupState methods.
        if self.lock_progress().init_in_flight {
            return Err("Startup initialization is already in progress.".to_string());
        }
        let mut snapshot = self.lock_snapshot();
        if snapshot.status.mode != "startup_failed" {
            return Err("Startup retry is only available after a failure.".to_string());
        }
        snapshot.status = blank_status("initializing");
        snapshot.recovery = None;
        Ok(())
    }

    fn lock_snapshot(&self) -> MutexGuard<'_, StartupSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_progress(&self) -> MutexGuard<'_, StartupProgress> {
        self.progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn blank_status(mode: &str) -> StartupStatus {
    StartupStatus {
        mode: mode.to_string(),
        reason: None,
        message: None,
        code: None,
        database_path: None,
        backup_path: None,
        backup_verified: false,
        can_reset: false,
        log_path: None,
        data_path: None,
    }
}

fn diagnostic_log_path() -> Option<String> {
    platform::app_log_dir()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

fn diagnostic_data_path_hint() -> Option<String> {
    // Best-effort without AppHandle: parent of the usual DB file is not known
    // until path() is resolved; expose log dir parent on Windows/macOS/Linux
    // via LOCALAPPDATA / Library / XDG when possible by stripping `/logs`.
    platform::app_log_dir().ok().and_then(|log_dir| {
        log_dir
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
    })
}

pub fn classify_startup_error(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("database") || lower.contains("sqlite") || lower.contains("migration") {
        "database"
    } else if lower.contains("proxy")
        || lower.contains("engineregistry")
        || lower.contains("engine")
    {
        "engine"
    } else if lower.contains("settings") {
        "settings"
    } else if lower.contains("tray") {
        "tray"
    } else if lower.contains("browser") || lower.contains("bridge") {
        "browser_bridge"
    } else {
        "init"
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_startup_status(state: State<'_, StartupState>) -> Result<StartupStatus, String> {
    Ok(state.lock_snapshot().status.clone())
}

#[tauri::command]
#[specta::specta]
pub async fn open_database_recovery_folder(state: State<'_, StartupState>) -> Result<(), String> {
    let recovery = state
        .lock_snapshot()
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
pub async fn open_startup_log_folder() -> Result<(), String> {
    let folder = platform::app_log_dir()?;
    platform::open_path(&folder)
}

#[tauri::command]
#[specta::specta]
pub async fn open_startup_data_folder(app: AppHandle) -> Result<(), String> {
    let db_path = platform::db_path(&app)?;
    let folder = db_path.parent().unwrap_or(db_path.as_path());
    platform::open_path(folder)
}

#[tauri::command]
#[specta::specta]
pub async fn reset_database_for_recovery(state: State<'_, StartupState>) -> Result<(), String> {
    let recovery = state
        .lock_snapshot()
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

/// UX-01: idempotent retry after `startup_failed`.
#[tauri::command]
#[specta::specta]
pub async fn retry_startup_init(
    app: AppHandle,
    state: State<'_, StartupState>,
) -> Result<(), String> {
    state.begin_retry()?;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::run_startup_init(&handle).await {
            tracing::error!(error = %error, "startup initialization retry failed");
        }
    });
    Ok(())
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
        let status = state.lock_snapshot().status.clone();
        assert!(!status.can_reset);
        assert!(!status.backup_verified);
    }

    #[test]
    fn set_failed_transitions_from_initializing() {
        let state = StartupState::initializing();
        state.set_failed("database", "could not open db");
        let status = state.lock_snapshot().status.clone();
        assert_eq!(status.mode, "startup_failed");
        assert_eq!(status.code.as_deref(), Some("database"));
        assert_eq!(status.message.as_deref(), Some("could not open db"));
        assert_eq!(status.reason.as_deref(), Some("database"));
    }

    #[test]
    fn set_failed_does_not_overwrite_ready_or_recovery() {
        let state = StartupState::initializing();
        state.set_ready();
        state.set_failed("init", "late failure");
        assert_eq!(state.current_mode(), "ready");

        let state = StartupState::initializing();
        state.set_recovery(db::DatabaseRecovery {
            reason: "migration_dirty".to_string(),
            message: "dirty".to_string(),
            database_path: PathBuf::from("database.sqlite"),
            backup_path: None,
            backup_verified: false,
        });
        state.set_failed("init", "should not replace recovery");
        assert_eq!(state.current_mode(), "database_recovery_required");
    }

    #[test]
    fn begin_retry_only_from_failed_and_blocks_while_in_flight() {
        let state = StartupState::initializing();
        assert!(state.begin_retry().is_err());

        state.set_failed("init", "boom");
        assert!(state.begin_retry().is_ok());
        assert_eq!(state.current_mode(), "initializing");

        state.set_failed("init", "boom again");
        assert!(state.try_begin_init());
        assert!(state.begin_retry().is_err());
        state.end_init();
        assert!(state.begin_retry().is_ok());
    }

    #[test]
    fn service_flags_are_sticky_for_idempotent_retry() {
        let state = StartupState::initializing();
        assert!(!state.app_state_managed());
        state.mark_app_state_managed();
        state.mark_clipboard_started();
        state.mark_scheduler_started();
        assert!(state.app_state_managed());
        assert!(state.clipboard_started());
        assert!(state.scheduler_started());
        assert!(!state.tray_created());
    }

    #[test]
    fn classify_startup_error_maps_common_phrases() {
        assert_eq!(
            classify_startup_error("Failed to open SQLite database"),
            "database"
        );
        assert_eq!(
            classify_startup_error("EngineRegistry build failed"),
            "engine"
        );
        assert_eq!(classify_startup_error("settings load error"), "settings");
        assert_eq!(classify_startup_error("tray icon missing"), "tray");
        assert_eq!(
            classify_startup_error("Could not start browser realtime bridge"),
            "browser_bridge"
        );
        assert_eq!(classify_startup_error("something else"), "init");
    }
}
