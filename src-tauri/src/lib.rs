pub mod browser_realtime;
pub mod clipboard;
pub mod commands;
pub mod db;
pub mod download;
pub mod events;
pub mod logging;
pub mod models;
pub mod platform;
pub mod proxy;
pub mod scheduler;
pub mod secure_headers;
pub mod state_machine;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
};

use sqlx::SqlitePool;
use tauri::{
    generate_handler,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tokio::{
    sync::{Mutex, OwnedMutexGuard},
    task::JoinHandle,
};

pub(crate) const TRAY_MENU_WINDOW_LABEL: &str = "tray-menu";
const TRAY_MENU_WIDTH: f64 = 232.0;
const TRAY_MENU_HEIGHT: f64 = 260.0;
const TRAY_MENU_SCREEN_MARGIN: f64 = 10.0;

pub struct DownloadControl {
    pub cancel_token: tokio_util::sync::CancellationToken,
    pub finish: Arc<AtomicBool>,
    /// PERF-15: the finish command notifies this after `finish.store(true)` so
    /// the waiting HLS loop wakes immediately instead of polling the DB flag.
    pub finish_notify: Arc<tokio::sync::Notify>,
    pub speed_limiter: Arc<download::GlobalSpeedLimiter>,
    pub handle: Option<JoinHandle<()>>,
    pub source_key: String,
    pub connection_slots: usize,
}

/// R-2: Per-task runtime lock registry that serializes user-initiated control
/// operations (pause/cancel/delete/retry) against task startup and against each
/// other for the same task id.
///
/// Workers (download engines) intentionally do **not** take this lock — they
/// rely on R-1's conditional DB update to avoid overwriting a status change
/// made by a lock-holding user action. This avoids deadlock between a worker
/// holding the lock and a user action waiting for it.
///
/// `evict` should be called after task deletion (once the guard is dropped) to
/// prevent the HashMap from growing unbounded over long-running sessions.
#[derive(Default)]
pub struct TaskRuntimeLocks(Mutex<HashMap<String, Arc<Mutex<()>>>>);

impl TaskRuntimeLocks {
    /// Acquires an owned guard for `task_id`. The guard can be held across
    /// `.await` points and is released on drop. Inserts a new `Arc<Mutex<()>>`
    /// on first use of a given task id.
    pub async fn lock(&self, task_id: &str) -> OwnedMutexGuard<()> {
        let arc = {
            let mut map = self.0.lock().await;
            map.entry(task_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        arc.lock_owned().await
    }

    /// Removes the entry for `task_id` when no other path holds a reference to
    /// its `Arc<Mutex<()>>` (i.e. `strong_count == 1`, meaning only the
    /// registry itself holds it). Call this **after** dropping the guard.
    pub async fn evict(&self, task_id: &str) {
        let mut map = self.0.lock().await;
        if let Some(arc) = map.get(task_id) {
            if Arc::strong_count(arc) == 1 {
                map.remove(task_id);
            }
        }
    }
}

pub type RequestHeaders = Vec<(String, String)>;
pub type TaskRequestHeaders = Arc<Mutex<HashMap<String, RequestHeaders>>>;

pub struct AppState {
    pub pool: SqlitePool,
    pub downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
    pub request_headers: TaskRequestHeaders,
    pub browser_realtime: Arc<browser_realtime::BrowserRealtimeState>,
    pub scheduler: Arc<scheduler::Scheduler>,
    pub speed_limiter: Arc<download::GlobalSpeedLimiter>,
    pub engine_registry: Arc<download::EngineRegistry>,
    pub quit_requested: Arc<AtomicBool>,
    pub task_runtime_locks: Arc<TaskRuntimeLocks>,
    pub lifecycle_gate: Arc<Mutex<()>>,
    pub lifecycle: Arc<AppLifecycle>,
    pub active_supervisors: Arc<AtomicUsize>,
    /// Prevents repeated native close events from opening competing decisions
    /// while the close dialog or pause-and-exit action is in flight.
    pub close_request_pending: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AppLifecyclePhase {
    Running = 0,
    RestartDraining = 1,
    ExitDraining = 2,
    RestartReady = 3,
    ExitReady = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClaim {
    StartDrain,
    ExitNow,
    AlreadyDraining,
}

#[derive(Default)]
pub struct AppLifecycle(AtomicU8);

impl AppLifecycle {
    pub fn phase(&self) -> AppLifecyclePhase {
        match self.0.load(Ordering::SeqCst) {
            1 => AppLifecyclePhase::RestartDraining,
            2 => AppLifecyclePhase::ExitDraining,
            3 => AppLifecyclePhase::RestartReady,
            4 => AppLifecyclePhase::ExitReady,
            _ => AppLifecyclePhase::Running,
        }
    }

    pub fn transition(&self, from: AppLifecyclePhase, to: AppLifecyclePhase) -> bool {
        if from == to {
            return false;
        }
        self.0
            .compare_exchange(from as u8, to as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn claim_exit(&self) -> ExitClaim {
        loop {
            let phase = self.phase();
            match phase {
                AppLifecyclePhase::Running | AppLifecyclePhase::RestartDraining => {
                    if self.transition(phase, AppLifecyclePhase::ExitDraining) {
                        return ExitClaim::StartDrain;
                    }
                }
                AppLifecyclePhase::RestartReady => {
                    if self.transition(phase, AppLifecyclePhase::ExitReady) {
                        return ExitClaim::ExitNow;
                    }
                }
                AppLifecyclePhase::ExitDraining => return ExitClaim::AlreadyDraining,
                AppLifecyclePhase::ExitReady => return ExitClaim::ExitNow,
            }
        }
    }

    pub fn set_phase(&self, phase: AppLifecyclePhase) {
        self.0.store(phase as u8, Ordering::SeqCst);
    }
}

pub struct ActiveSupervisorGuard(Arc<AtomicUsize>);

impl ActiveSupervisorGuard {
    pub fn new(active: Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::SeqCst);
        Self(active)
    }
}

impl Drop for ActiveSupervisorGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub const APP_RESTART_DRAIN_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// Cancel active work and wait until engines, owned resources, and supervisor
/// post-processing have all left their active lifecycle.
pub async fn drain_download_owners(
    downloads: &Mutex<HashMap<String, DownloadControl>>,
    active_supervisors: &AtomicUsize,
    timeout: Option<std::time::Duration>,
) -> Result<(), String> {
    let count = {
        let downloads = downloads.lock().await;
        for (task_id, control) in downloads.iter() {
            tracing::debug!(task_id, "cancelling download for app lifecycle transition");
            control.cancel_token.cancel();
        }
        downloads.len()
    };
    if count > 0 {
        tracing::info!(active_count = count, "draining active downloads");
    }

    let wait_for_drain = async {
        loop {
            let slots_empty = downloads.lock().await.is_empty();
            if slots_empty && active_supervisors.load(Ordering::SeqCst) == 0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    };

    match timeout {
        Some(timeout) => tokio::time::timeout(timeout, wait_for_drain)
            .await
            .map_err(|_| task_stop_pending_error())?,
        None => wait_for_drain.await,
    }
    tracing::info!("active download owners drained");
    Ok(())
}

/// Stops new task reservations before cancelling and waiting for all active
/// download owners. A bounded timeout leaves the app running and fail-closed.
pub async fn shutdown_active_downloads(
    state: &AppState,
    timeout: Option<std::time::Duration>,
) -> Result<(), String> {
    let _lifecycle_guard = state.lifecycle_gate.clone().lock_owned().await;
    state.quit_requested.store(true, Ordering::SeqCst);
    drain_download_owners(&state.downloads, &state.active_supervisors, timeout).await
}

pub async fn prepare_app_relaunch(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let lifecycle = state.lifecycle.clone();
    loop {
        match lifecycle.phase() {
            AppLifecyclePhase::Running => {
                if lifecycle.transition(
                    AppLifecyclePhase::Running,
                    AppLifecyclePhase::RestartDraining,
                ) {
                    break;
                }
            }
            AppLifecyclePhase::RestartReady => return Ok(()),
            _ => return Err(task_stop_pending_error()),
        }
    }

    state.quit_requested.store(true, Ordering::SeqCst);
    match shutdown_active_downloads(state.inner(), Some(APP_RESTART_DRAIN_BUDGET)).await {
        Ok(()) => {
            if lifecycle.transition(
                AppLifecyclePhase::RestartDraining,
                AppLifecyclePhase::RestartReady,
            ) {
                Ok(())
            } else {
                Err(task_stop_pending_error())
            }
        }
        Err(error) => {
            let _lifecycle_guard = state.lifecycle_gate.clone().lock_owned().await;
            if lifecycle.transition(
                AppLifecyclePhase::RestartDraining,
                AppLifecyclePhase::Running,
            ) {
                state.quit_requested.store(false, Ordering::SeqCst);
            }
            Err(error)
        }
    }
}

pub async fn cancel_prepared_app_relaunch(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    {
        let _lifecycle_guard = state.lifecycle_gate.clone().lock_owned().await;
        match state.lifecycle.phase() {
            AppLifecyclePhase::RestartReady
                if state
                    .lifecycle
                    .transition(AppLifecyclePhase::RestartReady, AppLifecyclePhase::Running) =>
            {
                state.quit_requested.store(false, Ordering::SeqCst);
            }
            AppLifecyclePhase::RestartReady => return Err(task_stop_pending_error()),
            AppLifecyclePhase::Running => return Ok(()),
            _ => return Err(task_stop_pending_error()),
        }
    }
    state
        .scheduler
        .dispatch_detached(app.clone(), state.pool.clone());
    Ok(())
}

pub fn request_graceful_exit(app: tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        app.exit(0);
        return;
    };
    let lifecycle = state.lifecycle.clone();
    match lifecycle.claim_exit() {
        ExitClaim::AlreadyDraining => return,
        ExitClaim::ExitNow => {
            app.exit(0);
            return;
        }
        ExitClaim::StartDrain => {}
    }

    state.quit_requested.store(true, Ordering::SeqCst);
    let _ = app.emit("app://shutting-down", ());
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(error) = shutdown_active_downloads(state.inner(), None).await {
            tracing::error!(error = %error, "unbounded app shutdown drain failed");
            return;
        }
        state.lifecycle.set_phase(AppLifecyclePhase::ExitReady);
        app.exit(0);
    });
}

/// ARC-62: a deadline bounds the user action, never the lifetime of a file writer.
/// The caller retains control on timeout and must not mutate files or restart.
pub async fn cancel_and_drain_control(
    control: &mut DownloadControl,
    grace: std::time::Duration,
) -> Result<(), String> {
    control.cancel_token.cancel();
    let Some(handle) = control.handle.as_mut() else {
        return Err(task_stop_pending_error());
    };
    match tokio::time::timeout(grace, handle).await {
        Ok(result) => {
            control.handle.take();
            if let Err(error) = result {
                tracing::warn!(%error, "download supervisor join failed");
            }
            Ok(())
        }
        Err(_) => Err(task_stop_pending_error()),
    }
}

pub const USER_ACTION_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

pub fn task_stop_pending_error() -> String {
    models::AppErrorPayload::new(
        "task_stop_pending",
        "The previous download is still stopping. Retry this action after its file operations finish.",
        true,
        Vec::new(),
    ).command_error()
}

/// ARC-57 / ARC-62: preserve the active slot while awaiting the supervisor.
/// The per-task lock serializes callers; the map lock must be released because
/// the supervisor removes its own entry only after all owned resources drain.
pub async fn remove_and_drain_control(
    downloads: &Mutex<HashMap<String, DownloadControl>>,
    request_headers: &Mutex<HashMap<String, RequestHeaders>>,
    task_id: &str,
    grace: std::time::Duration,
) -> Result<(), String> {
    let mut handle = {
        let mut active = downloads.lock().await;
        let Some(control) = active.get_mut(task_id) else {
            return Ok(());
        };
        control.cancel_token.cancel();
        control.handle.take().ok_or_else(task_stop_pending_error)?
    };
    if tokio::time::timeout(grace, &mut handle).await.is_err() {
        let mut active = downloads.lock().await;
        if let Some(control) = active.get_mut(task_id) {
            control.handle = Some(handle);
            return Err(task_stop_pending_error());
        }
        // The supervisor finished resource convergence at the deadline boundary.
        // Its remaining tail owns no download writer.
        return Ok(());
    }
    downloads.lock().await.remove(task_id);
    request_headers.lock().await.remove(task_id);
    Ok(())
}

/// R-4: Single source of truth for the command list shared between
/// `tauri_specta::collect_commands!` (Specta bindings) and
/// `tauri::generate_handler!` (runtime invoke handler). Both macros have
/// overwrite-not-append semantics, so we cannot call them twice to add
/// debug-only commands; instead the optional `$extra` arm appends the
/// debug-only `seed_*` commands to the same list.
///
/// `$apply` is a single identifier resolved at the expansion site —
/// `collect_commands` is imported via `use tauri_specta::{collect_commands, Builder};`
/// inside `specta_builder()`, and `generate_handler` is imported at module
/// scope via `use tauri::{generate_handler, ...}`. Adding a new public
/// command only requires editing the base list in the second arm below.
macro_rules! vibe_commands_base {
    // No extra commands: delegate to the second arm with an empty extra.
    ($apply:ident) => {
        vibe_commands_base!($apply,)
    };
    // Base command list (defined once) + optional extra commands appended
    // by debug-only builds (e.g. `seed_mock_tasks`).
    ($apply:ident, $($extra:tt)*) => {
        $apply![
            commands::tasks::list_tasks,
            commands::tasks::list_tasks_page,
            commands::tasks::list_tasks_cursor,
            commands::tasks::list_tasks_by_ids,
            commands::tasks::get_task,
            commands::tasks::get_task_stats,
            commands::tasks::get_scheduler_snapshot,
            commands::tasks::list_segments,
            commands::tasks::list_segments_page,
            commands::tasks::list_hls_segments_page,
            commands::tasks::list_dash_segments_page,
            commands::tasks::get_segment_summary,
            commands::tasks::get_torrent_runtime_snapshot,
            commands::tasks::get_task_proxy_settings,
            commands::tasks::get_task_request_profile,
            commands::tasks::update_task_request_profile,
            commands::tasks::create_network_authorization,
            commands::tasks::get_task_network_policy,
            commands::tasks::revoke_task_network_authorization,
            commands::tasks::list_task_events_page,
            commands::tasks::list_task_requests_page,
            commands::tasks::get_task_integrity_passport,
            commands::tasks::recheck_task,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::reset_settings,
            commands::settings::list_sftp_known_hosts,
            commands::settings::forget_sftp_known_host,
            commands::backup::create_app_backup,
            commands::backup::validate_app_backup,
            commands::backup::restore_app_backup,
            commands::backup::describe_backup_source,
            commands::backup::restore_backup_subset,
            commands::backup::get_last_restore_report,
            commands::backup::dismiss_restore_report,
            commands::local_files::read_local_text_file,
            commands::local_files::write_export_file,
            commands::startup::get_startup_status,
            commands::startup::open_database_recovery_folder,
            commands::startup::open_startup_log_folder,
            commands::startup::open_startup_data_folder,
            commands::startup::reset_database_for_recovery,
            commands::startup::retry_startup_init,
            commands::ffmpeg::probe_ffmpeg_version,
            commands::environment::get_environment_health,
            commands::environment::run_environment_fix,
            commands::browser::get_browser_integration_status,
            commands::browser::install_browser_integration,
            commands::browser::uninstall_browser_integration,
            commands::browser::export_browser_extension_packages,
            commands::browser::get_browser_capture_settings,
            commands::browser::update_browser_capture_settings,
            commands::browser::create_browser_handoff_task,
            commands::browser::get_browser_handoff_history,
            commands::browser::validate_browser_handoff,
            commands::browser::list_expired_auth_header_tasks,
            commands::browser::run_browser_native_host_self_check,
            commands::classification::list_classification_rules,
            commands::classification::create_classification_rule,
            commands::classification::update_classification_rule,
            commands::classification::delete_classification_rule,
            commands::classification::reorder_classification_rules,
            commands::classification::preview_classification_match,
            commands::floating::show_floating_status_window,
            commands::floating::hide_floating_status_window,
            commands::floating::toggle_floating_status_window,
            commands::floating::focus_main_window_from_floating,
            commands::floating::show_tray_menu_at,
            commands::tray::run_tray_menu_action,
            commands::system::request_system_shutdown,
            commands::system::request_system_sleep,
            commands::system::request_system_hibernate,
            commands::system::request_lock_screen,
            commands::system::prepare_app_relaunch,
            commands::system::cancel_prepared_app_relaunch,
            commands::system::resolve_close_request,
            commands::system::update_desktop_status,
            commands::system::query_disk_space,
            commands::system::extract_system_file_icon,
            commands::tasks::probe_task,
            commands::tasks::probe_ftp_directory,
            commands::tasks::probe_sftp_directory,
            commands::tasks::probe_webdav_directory,
            commands::tasks::create_task,
            commands::tasks::import_urls,
            commands::tasks::update_task_transfer_options,
            commands::tasks::reorder_queued_tasks,
            commands::tasks::update_torrent_file_selection,
            commands::tasks::update_torrent_seeding,
            commands::tasks::update_task_proxy_settings,
            commands::tasks::verify_task_hash,
            commands::tasks::compute_file_hash,
            commands::tasks::pause_task,
            commands::tasks::resume_task,
            commands::tasks::retry_task,
            commands::tasks::redownload_task,
            commands::tasks::list_metalink_mirrors,
            commands::tasks::retry_task_with_mirror,
            commands::tasks::finish_live_recording,
            commands::tasks::resolve_task_attention,
            commands::tasks::cancel_task,
            commands::tasks::delete_task,
            commands::tasks::bulk_delete_tasks,
            commands::tasks::bulk_task_action,
            commands::tasks::bulk_task_action_global,
            commands::tasks::open_task_file,
            commands::tasks::open_task_folder,
            commands::storage::scan_storage,
            commands::storage::clean_storage_artifacts,
            commands::storage::cleanup_task_temp_files,
            commands::storage::get_last_storage_sweep,
            commands::recovery::bulk_resolve_attention,
            commands::recovery::update_task_credentials,
            commands::recovery::list_recovery_history,
            $($extra)*
        ]
    };
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    use tauri_specta::{collect_commands, Builder};

    // R-4: tauri-specta's `Builder::commands()` and Tauri's `invoke_handler()`
    // both have override semantics and cannot be appended to. The command list is defined in a single place via the `vibe_commands_base!` macro;
    // debug builds append `seed_mock_tasks` / `seed_scale_tasks` via the `extra` parameter.
    #[cfg(debug_assertions)]
    let builder = Builder::<tauri::Wry>::new().commands(vibe_commands_base!(
        collect_commands,
        commands::tasks::seed_mock_tasks,
        commands::tasks::seed_scale_tasks
    ));

    #[cfg(not(debug_assertions))]
    let builder = Builder::<tauri::Wry>::new().commands(vibe_commands_base!(collect_commands));

    builder
        .typ::<models::AppErrorPayload>()
        .typ::<models::AppSettings>()
        .typ::<clipboard::ClipboardLinkDetectedPayload>()
        .typ::<proxy::AppProxyMode>()
        .typ::<models::TaskUpdatedPayload>()
        .typ::<models::TaskProgressPayload>()
        .typ::<events::QueueChangedPayload>()
        .typ::<events::ProbePhasePayload>()
        .typ::<events::BrowserHandoffAuthorizationRequiredPayload>()
        .typ::<events::StorageCleanupProgressPayload>()
        .typ::<events::CloseRequestPayload>()
        .typ::<models::storage::StorageScanResult>()
        .typ::<models::storage::StorageCleanupResult>()
        .typ::<models::storage::StorageSweepRecord>()
        .typ::<models::storage::CleanupMode>()
        .typ::<models::recovery::RecoveryHistoryRecord>()
        .typ::<models::backup::BackupContents>()
        .typ::<models::backup::BackupPathPolicySummary>()
        .typ::<models::backup::BackupDiskCheck>()
        .typ::<models::backup::BackupSettingsPreview>()
        .typ::<models::backup::BackupSubsetSelection>()
        .typ::<models::backup::BackupSubsetRestoreResult>()
        .typ::<models::backup::RestoreReport>()
        .typ::<models::integrity::IntegrityPassport>()
        .typ::<models::integrity::PassportChecksum>()
        .typ::<models::integrity::PassportChecksumState>()
        .typ::<models::integrity::PassportStagingCleanup>()
        .typ::<models::integrity::RemoteValidatorKind>()
        .typ::<models::recovery::BulkRecoveryAction>()
        .typ::<models::recovery::BulkRecoveryResult>()
        .typ::<models::recovery::UpdateTaskCredentialsInput>()
        .typ::<models::RecoveryAction>()
        .typ::<models::TaskStatsSnapshot>()
        .typ::<models::TaskFailureCategory>()
        .typ::<models::TorrentRuntimeSnapshot>()
        .typ::<models::TorrentTrackerStatus>()
        .typ::<models::TaskProxySettings>()
        .typ::<models::TaskProxySettingsInput>()
        .typ::<models::TaskProxyMode>()
        .typ::<models::CompletionAction>()
        .typ::<models::CompletionActionRequestedPayload>()
        .typ::<models::FtpDirectoryProbe>()
        .typ::<models::FtpDirectoryEntry>()
        .typ::<models::SftpDirectoryProbe>()
        .typ::<models::SftpDirectoryEntry>()
        .typ::<models::WebDavDirectoryProbe>()
        .typ::<models::WebDavDirectoryEntry>()
        .typ::<commands::tasks::ListTasksResult>()
        .typ::<commands::tasks::ListTasksCursorResult>()
        .typ::<commands::tasks::TaskFilterOptions>()
        .typ::<commands::tasks::TaskEventsPageResult>()
        .typ::<commands::tasks::TaskRequestsPageResult>()
        .typ::<commands::tasks::TaskSegmentsPageResult>()
        .typ::<commands::tasks::HlsSegmentsPageResult>()
        .typ::<commands::tasks::HlsSegmentView>()
        .typ::<models::RequestDiagnostic>()
        .typ::<models::SegmentSummary>()
        .typ::<models::HashVerificationState>()
        .typ::<models::BatchImportResult>()
        .typ::<models::ScaleStateDistribution>()
        .typ::<models::BrowserIntegrationStatus>()
        .typ::<models::BrowserCaptureSettings>()
        .typ::<models::BrowserCaptureSettingsInput>()
        .typ::<models::BrowserForwardHeadersMode>()
        .typ::<models::BrowserSiteRule>()
        .typ::<models::BrowserSiteRuleMode>()
        .typ::<models::BrowserRealtimeStatus>()
        .typ::<models::BrowserExtensionPackage>()
        .typ::<models::BrowserExtensionExportResult>()
        .typ::<models::BrowserHandoffResult>()
        .typ::<models::BrowserHandoffHistory>()
        .typ::<models::BrowserHandoffRecord>()
        .typ::<models::ExpiredAuthHeaderTask>()
        .typ::<models::BrowserNativeHostSelfCheck>()
        .typ::<models::EnvironmentHealthReport>()
        .typ::<models::EnvironmentHealthItem>()
        .typ::<models::EnvironmentHealthStatus>()
        .typ::<models::EnvironmentFixAction>()
        .typ::<models::EnvironmentFixKind>()
        .typ::<models::EnvironmentFixInput>()
        .typ::<models::EnvironmentFixResult>()
        .typ::<commands::tray::TrayMenuAction>()
        .typ::<commands::system::CloseRequestAction>()
        .typ::<commands::system::DesktopStatusUpdate>()
}

pub fn export_typescript_bindings() -> Result<(), Box<dyn std::error::Error>> {
    use specta_typescript::Typescript;

    let bindings_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src")
        .join("generated")
        .join("bindings.ts");

    specta_builder().export(Typescript::default(), bindings_path)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    {
        export_typescript_bindings().expect("Failed to export TypeScript bindings");
    }

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets({
                    let mut targets = vec![tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::LogDir {
                            file_name: Some("vibe".to_string()),
                        },
                    )];
                    if cfg!(debug_assertions) {
                        targets.push(tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::Webview,
                        ));
                        targets.push(tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::Stdout,
                        ));
                    }
                    targets
                })
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Trace
                } else {
                    log::LevelFilter::Info
                })
                // PERF-12: the plugin defaults to a 40 KB cap with KeepOne, which
                // discards earlier content. A download manager running several
                // concurrent tasks fills that in minutes, so by the time a user
                // reports a problem the relevant lines are already gone -- which
                // also defeats the "open log directory" recovery entry from UX-01.
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!(args = ?args, "single-instance launch received");
            process_browser_handoff_files_from_args(app, args, "single-instance");
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            if window.label() == TRAY_MENU_WINDOW_LABEL {
                if let WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                let app = window.app_handle();
                // AppState may not be managed yet if the user closes the
                // window during the brief background-init window. In that
                // case, allow the close to proceed normally instead of
                // panicking on state access.
                let Some(state) = app.try_state::<AppState>() else {
                    return;
                };
                if state.lifecycle.phase() != AppLifecyclePhase::Running {
                    api.prevent_close();
                    request_graceful_exit(app.clone());
                    return;
                }

                let close_to_tray = tauri::async_runtime::block_on(async {
                    let default_dir =
                        commands::settings::default_download_dir(app).unwrap_or_default();
                    db::get_settings(&state.pool, default_dir)
                        .await
                        .map(|settings| settings.close_to_tray)
                        .unwrap_or(false)
                });
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }

                let stats = tauri::async_runtime::block_on(db::task_stats_snapshot(&state.pool));
                let (active, queued, stats_unavailable) = match stats {
                    Ok(snapshot) => {
                        let active = snapshot.active.parse::<u32>().ok();
                        let queued = snapshot.queued.parse::<u32>().ok();
                        match (active, queued) {
                            (Some(active), Some(queued)) => (active, queued, false),
                            _ => {
                                tracing::warn!(
                                    "task stats snapshot contained invalid counts during close"
                                );
                                (0, 0, true)
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "task stats snapshot failed during close");
                        (0, 0, true)
                    }
                };
                if active == 0 {
                    if stats_unavailable {
                        // Keep the window open and surface the decision dialog
                        // when ownership is unknown. Exiting here could
                        // terminate an active download without a checkpoint.
                        api.prevent_close();
                        if state.close_request_pending.swap(true, Ordering::SeqCst) {
                            return;
                        }
                        events::emit_close_requested(
                            app,
                            &events::CloseRequestPayload {
                                active,
                                queued,
                                stats_unavailable,
                            },
                        );
                        return;
                    }
                    // The lifecycle coordinator prevents repeated close events
                    // from bypassing the owner drain when nothing needs a UI
                    // decision.
                    api.prevent_close();
                    request_graceful_exit(app.clone());
                    return;
                }

                api.prevent_close();
                if state.close_request_pending.swap(true, Ordering::SeqCst) {
                    return;
                }
                events::emit_close_requested(
                    app,
                    &events::CloseRequestPayload {
                        active,
                        queued,
                        stats_unavailable,
                    },
                );
            }
        });

    // R-4: invoke_handler also has override semantics; reuse the vibe_commands_base! macro to keep a single command list source.
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(vibe_commands_base!(
        generate_handler,
        commands::tasks::seed_mock_tasks,
        commands::tasks::seed_scale_tasks
    ));

    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(vibe_commands_base!(generate_handler));

    let app = builder
        .setup(|app| {
            logging::init_logging(app.handle())?;

            let handle = app.handle().clone();
            app.manage(commands::startup::StartupState::initializing());

            // Show the main window as early as possible so the user sees the
            // splash placeholder (rendered by index.html before React mounts)
            // instead of a blank frame while the DB / settings / network stack
            // finishes initializing below.
            if let Some(window) = app.get_webview_window("main") {
                platform::configure_main_window(&window)?;
                let _ = window.show();
            }

            // All heavy initialization (DB, migrations, settings, scheduler,
            // tray, browser bridge) runs on a background task so the main
            // thread is free for the webview to paint the inline splash
            // immediately. Without this, the ~8 block_on calls below would
            // keep the main thread busy and WebView2 could not paint the
            // splash until all of them finished — which is why the loading
            // icon took a long time to appear. StartupState transitions to
            // "ready" once the backend can handle IPC; the frontend
            // StartupGate polls get_startup_status until then.
            tauri::async_runtime::spawn(async move {
                if let Err(error) = run_startup_init(&handle).await {
                    tracing::error!(error = %error, "startup initialization failed");
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|app_handle, event| {
        if matches!(&event, RunEvent::Resumed) {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.scheduler.notify_speed_policy_changed();
            }
        }
        if let RunEvent::ExitRequested { api, .. } = event {
            let Some(state) = app_handle.try_state::<AppState>() else {
                return;
            };
            if !matches!(
                state.lifecycle.phase(),
                AppLifecyclePhase::RestartReady | AppLifecyclePhase::ExitReady
            ) {
                api.prevent_exit();
                request_graceful_exit(app_handle.clone());
            }
        }
    });
}

/// Heavy startup work that runs after the main window is shown.
///
/// Runs on a background async task so the main thread stays free for the
/// webview to paint the splash. `StartupState` is transitioned to "ready"
/// once `AppState` is managed — at that point the frontend `StartupGate`
/// mounts `AppShell`. Ordinary failures before ready become `startup_failed`
/// so the UI can offer an idempotent Retry (UX-01).
pub(crate) async fn run_startup_init(handle: &tauri::AppHandle) -> Result<(), String> {
    let startup = handle.state::<commands::startup::StartupState>();
    if !startup.try_begin_init() {
        return Ok(());
    }
    let result = run_startup_init_inner(handle).await;
    startup.end_init();
    if let Err(ref error) = result {
        if startup.current_mode() == "initializing" {
            startup.set_failed(
                commands::startup::classify_startup_error(error),
                error.clone(),
            );
        }
    }
    result
}

async fn run_startup_init_inner(handle: &tauri::AppHandle) -> Result<(), String> {
    let startup = handle.state::<commands::startup::StartupState>();

    if !startup.app_state_managed() {
        let db_path = platform::db_path(handle)?;
        let db_connection = match db::connect_for_startup(&db_path).await? {
            db::DatabaseConnectOutcome::Ready(connection) => connection,
            db::DatabaseConnectOutcome::RecoveryRequired(recovery) => {
                startup.set_recovery(recovery);
                return Ok(());
            }
        };
        let pool = db_connection.pool;
        // Startup WAL checkpoint: if the `-wal` file exceeds 100 MB after
        // an unclean shutdown or a long session, truncate it now so the
        // app starts with a bounded journal.
        if db::wal_file_size_bytes(&db_path) > 100 * 1024 * 1024 {
            tracing::info!("WAL file exceeds 100MB, running checkpoint on startup");
            if let Err(e) = db::wal_checkpoint(&pool).await {
                tracing::warn!(error = %e, "startup WAL checkpoint failed");
            }
        }
        // Run non-critical maintenance so it finishes before scheduling starts.
        // Best-effort; failures are logged but never abort startup.
        if let Err(error) = db::clear_expired_task_request_headers(&pool).await {
            tracing::warn!(error = %error, "expired browser request header cleanup failed");
        }
        if let Err(error) = db::migrate_legacy_ftp_credentials(&pool).await {
            tracing::warn!(error = %error, "legacy FTP credential migration failed");
        }
        match db::prune_request_diagnostics(&pool).await {
            Ok(0) => {}
            Ok(removed) => {
                tracing::info!(removed, "startup request diagnostics prune");
            }
            Err(error) => {
                tracing::warn!(error = %error, "request diagnostics prune failed");
            }
        }
        match db::prune_task_events(&pool).await {
            Ok(0) => {}
            Ok(removed) => {
                tracing::info!(removed, "startup task events prune");
            }
            Err(error) => {
                tracing::warn!(error = %error, "task events prune failed");
            }
        }
        match db::prune_browser_messages(&pool).await {
            Ok(0) => {}
            Ok(removed) => {
                tracing::info!(removed, "startup browser messages prune");
            }
            Err(error) => {
                tracing::warn!(error = %error, "browser messages prune failed");
            }
        }
        // Hoisted before the sweep so the artifact sweep also covers the
        // default save dir even when no task row references it yet.
        let default_dir = commands::settings::default_download_dir(handle)?;
        let sweep_started = chrono::Utc::now().to_rfc3339();
        match download::artifacts::sweep_orphan_artifacts(
            &pool,
            std::slice::from_ref(&default_dir),
            download::artifacts::SweepOptions { include_dht: true },
        )
        .await
        {
            Ok(summary) => {
                if summary.removed > 0 || summary.failed > 0 {
                    tracing::info!(
                        removed = summary.removed,
                        failed = summary.failed,
                        "startup artifact sweep"
                    );
                }
                let record = models::StorageSweepRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    started_at: sweep_started,
                    finished_at: chrono::Utc::now().to_rfc3339(),
                    mode: "startup".to_string(),
                    removed_count: u32::try_from(summary.removed).unwrap_or(u32::MAX),
                    failed_count: u32::try_from(summary.failed).unwrap_or(u32::MAX),
                    reclaimed_bytes: summary.reclaimed_bytes.to_string(),
                };
                if let Err(error) = db::insert_sweep_record(&pool, &record, "{}").await {
                    tracing::warn!(error = %error, "could not persist startup sweep record");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "artifact sweep failed");
            }
        }
        let settings = db::get_settings(&pool, default_dir).await?;
        db::reset_interrupted_tasks(&pool, settings.auto_resume_on_startup).await?;
        let speed_limiter = Arc::new(download::GlobalSpeedLimiter::new(
            db::parse_speed_limit_bps(settings.global_speed_limit_bps.as_deref()),
        ));
        let engine_registry = Arc::new(download::EngineRegistry::new()?);
        engine_registry
            .set_proxy_config(proxy::ResolvedProxyConfig::from_settings(&settings))
            .await;
        let browser_realtime = browser_realtime::BrowserRealtimeState::new();
        let downloads = Arc::new(Mutex::new(HashMap::new()));
        let request_headers: TaskRequestHeaders = Arc::new(Mutex::new(HashMap::new()));
        let task_runtime_locks = Arc::new(TaskRuntimeLocks::default());
        let scheduler = Arc::new(scheduler::Scheduler::new(
            downloads.clone(),
            request_headers.clone(),
            speed_limiter.clone(),
            engine_registry.clone(),
            task_runtime_locks.clone(),
        ));

        handle.manage(AppState {
            pool: pool.clone(),
            downloads,
            request_headers,
            browser_realtime: browser_realtime.clone(),
            scheduler,
            speed_limiter,
            engine_registry,
            quit_requested: Arc::new(AtomicBool::new(false)),
            task_runtime_locks,
            lifecycle_gate: Arc::new(Mutex::new(())),
            lifecycle: Arc::new(AppLifecycle::default()),
            active_supervisors: Arc::new(AtomicUsize::new(0)),
            close_request_pending: Arc::new(AtomicBool::new(false)),
        });
        startup.mark_app_state_managed();
        run_post_app_state_services(handle, settings.floating_window_enabled).await?;
    } else {
        // AppState already exists from a prior partial attempt; finish services
        // without re-managing state or re-reading settings for floating sync.
        run_post_app_state_services(handle, false).await?;
    }

    Ok(())
}

async fn run_post_app_state_services(
    handle: &tauri::AppHandle,
    floating_window_enabled: bool,
) -> Result<(), String> {
    let startup = handle.state::<commands::startup::StartupState>();
    let app_state = handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not managed.".to_string())?;

    // AppState is managed — the backend can safely handle IPC. Transition
    // StartupState to "ready" so the frontend StartupGate mounts AppShell.
    if startup.current_mode() != "ready" {
        startup.set_ready();
    }

    if !startup.clipboard_started() {
        clipboard::start(handle.clone());
        startup.mark_clipboard_started();
    }

    if !startup.browser_bridge_started() {
        browser_realtime::start(handle.clone(), app_state.browser_realtime.clone()).await?;
        startup.mark_browser_bridge_started();
    }

    if !startup.scheduler_started() {
        let handle_clone = handle.clone();
        let scheduler = app_state.scheduler.clone();
        let pool_clone = app_state.pool.clone();
        tauri::async_runtime::spawn(async move {
            scheduler
                .clone()
                .dispatch(handle_clone.clone(), pool_clone.clone())
                .await;
            scheduler
                .schedule_retry_after_wakeup(handle_clone, pool_clone)
                .await;
        });
        startup.mark_scheduler_started();
    }

    if !startup.monitors_started() {
        commands::tasks::spawn_schedule_window_monitor(handle.clone(), app_state.inner());
        commands::tasks::spawn_request_diagnostics_cleanup(handle.clone());
        commands::tasks::spawn_wal_checkpoint_monitor(handle.clone());
        commands::tasks::spawn_desktop_status_monitor(handle.clone());
        startup.mark_monitors_started();
    }

    if !startup.tray_created() {
        create_tray(handle).map_err(|e| format!("{e}"))?;
        startup.mark_tray_created();
    }

    process_browser_handoffs_at_ready(handle, std::env::args().collect());

    if floating_window_enabled && !startup.floating_synced() {
        commands::floating::sync_floating_status_window(handle, true)?;
        startup.mark_floating_synced();
    }

    Ok(())
}

/// ARC-14: After AppState is ready, process CLI handoff args plus any leftover
/// `*.json` files in the handoff directory (written during the AppState gap).
fn process_browser_handoffs_at_ready(app: &tauri::AppHandle, args: Vec<String>) {
    let arg_files = browser_handoff_files_from_args(args);
    let scanned = commands::browser::collect_pending_handoff_files();
    let files = commands::browser::merge_handoff_file_paths(arg_files, scanned);
    if files.is_empty() {
        return;
    }
    tracing::info!(
        count = files.len(),
        "browser handoff files ready for startup replay"
    );
    process_browser_handoff_files(app, files, "startup-replay");
}

fn process_browser_handoff_files_from_args(
    app: &tauri::AppHandle,
    args: Vec<String>,
    source: &'static str,
) {
    let files = browser_handoff_files_from_args(args);
    if files.is_empty() {
        return;
    }
    tracing::info!(
        count = files.len(),
        source,
        "browser handoff files received"
    );
    process_browser_handoff_files(app, files, source);
}

fn process_browser_handoff_files(
    app: &tauri::AppHandle,
    files: Vec<std::path::PathBuf>,
    source: &'static str,
) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // AppState may not be managed yet if a second instance launches during
        // the background-init window. Keep handoff files on disk — ready-time
        // directory scan (ARC-14) will replay them. Do not claim the extension
        // will retry: native host already returned accepted.
        let Some(state) = handle.try_state::<AppState>() else {
            tracing::warn!(
                source,
                count = files.len(),
                "browser handoff received before AppState ready; retaining files for startup replay"
            );
            return;
        };
        commands::browser::process_browser_handoff_paths(
            handle.clone(),
            state.inner(),
            files,
            source,
        )
        .await;
    });
}

pub(crate) fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub(crate) fn open_downloads_dir(app: &tauri::AppHandle) {
    let default_dir = match commands::settings::default_download_dir(app) {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(error = %error, "failed to resolve default downloads directory");
            return;
        }
    };
    let state = app.state::<AppState>();
    let save_dir = tauri::async_runtime::block_on(async {
        db::get_settings(&state.pool, default_dir.clone())
            .await
            .map(|settings| settings.default_save_dir)
    })
    .unwrap_or_else(|error| {
        tracing::warn!(error = %error, "failed to load configured downloads directory");
        default_dir
    });

    let path = std::path::PathBuf::from(save_dir);
    if let Err(error) = std::fs::create_dir_all(&path) {
        tracing::warn!(
            path = %path.display(),
            error = %error,
            "failed to create downloads directory before opening"
        );
        return;
    }
    if let Err(error) = platform::open_path(&path) {
        tracing::warn!(
            path = %path.display(),
            error = %error,
            "failed to open downloads directory from tray menu"
        );
    }
}

fn create_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("Vibe Downloader")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                position,
                button,
                button_state,
                ..
            } if button == MouseButton::Right && button_state == MouseButtonState::Down => {
                if let Err(error) = show_tray_menu_window(tray.app_handle(), position) {
                    tracing::warn!(error = %error, "failed to show custom tray menu");
                }
            }
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                if let Some(window) = tray.app_handle().get_webview_window(TRAY_MENU_WINDOW_LABEL) {
                    let _ = window.hide();
                }
                focus_main_window(tray.app_handle());
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn show_tray_menu_window(
    app: &tauri::AppHandle,
    cursor: PhysicalPosition<f64>,
) -> tauri::Result<()> {
    let position = tray_menu_position(app, cursor);
    let window = if let Some(window) = app.get_webview_window(TRAY_MENU_WINDOW_LABEL) {
        window
    } else {
        WebviewWindowBuilder::new(
            app,
            TRAY_MENU_WINDOW_LABEL,
            WebviewUrl::App("index.html?surface=tray-menu".into()),
        )
        .title("Vibe Downloader")
        .inner_size(TRAY_MENU_WIDTH, TRAY_MENU_HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(true)
        .build()?
    };

    window.set_position(position)?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

fn tray_menu_position(
    app: &tauri::AppHandle,
    cursor: PhysicalPosition<f64>,
) -> PhysicalPosition<i32> {
    let monitor = app
        .available_monitors()
        .ok()
        .and_then(|monitors| {
            monitors.into_iter().find(|monitor| {
                let area = monitor.work_area();
                let left = area.position.x as f64;
                let top = area.position.y as f64;
                let right = left + area.size.width as f64;
                let bottom = top + area.size.height as f64;
                cursor.x >= left && cursor.x <= right && cursor.y >= top && cursor.y <= bottom
            })
        })
        .or_else(|| app.primary_monitor().ok().flatten());

    let scale_factor = monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let menu_width = TRAY_MENU_WIDTH * scale_factor;
    let menu_height = TRAY_MENU_HEIGHT * scale_factor;
    let margin = TRAY_MENU_SCREEN_MARGIN * scale_factor;
    let offset = 8.0 * scale_factor;

    let (left, top, right, bottom) = monitor
        .as_ref()
        .map(|monitor| {
            let area = monitor.work_area();
            (
                area.position.x as f64 + margin,
                area.position.y as f64 + margin,
                area.position.x as f64 + area.size.width as f64 - margin,
                area.position.y as f64 + area.size.height as f64 - margin,
            )
        })
        .unwrap_or((margin, margin, f64::MAX / 4.0, f64::MAX / 4.0));

    let mut x = cursor.x - menu_width + offset;
    let mut y = cursor.y - menu_height - offset;

    if y < top {
        y = cursor.y + offset;
    }
    if x < left {
        x = cursor.x - offset;
    }

    x = x.clamp(left, (right - menu_width).max(left));
    y = y.clamp(top, (bottom - menu_height).max(top));

    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

fn browser_handoff_files_from_args(args: Vec<String>) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--browser-handoff-file" {
            if let Some(path) = iter.next() {
                files.push(std::path::PathBuf::from(path));
            }
        }
    }
    files
}
