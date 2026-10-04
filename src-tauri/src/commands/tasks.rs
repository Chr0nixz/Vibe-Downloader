use std::{path::PathBuf, sync::atomic::Ordering};

use serde::Deserialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use super::task_file_planning::unique_final_path;
pub use super::task_resume::{
    local_resume_error, resume_decision_message, resume_mismatch_message, segment_resume_error,
};
// Shared with the Storage Center's cleanup commands (not part of the IPC
// surface).
pub(crate) use super::tasks::actions::{delete_paths_off_runtime, FileDeleteRequest};

use crate::{
    db,
    download::{EngineRegistry, ProbeRequest},
    events::{
        emit_desktop_status, emit_queue_changed_with_ids, emit_task_progress,
        emit_task_updated_record,
    },
    models::{
        AppErrorPayload, FtpDirectoryProbe, RecoveryAction, SftpDirectoryProbe, Task,
        TaskChecksumRecord, TaskFileRecord, TaskProxySettings, TaskProxySettingsInput, TaskRecord,
        TaskStatus, WebDavDirectoryProbe,
    },
    state_machine::TransitionError,
    AppState, TaskRequestHeaders,
};

#[tauri::command]
#[specta::specta]
pub async fn get_task_request_profile(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<crate::models::TaskRequestProfileView, String> {
    require_task(&state.pool, &task_id).await?;
    db::get_task_request_profile(&state.pool, &task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_request_profile(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    task_id: String,
    input: crate::models::TaskRequestProfileInput,
    replace_sensitive: bool,
) -> Result<crate::models::TaskRequestProfileView, String> {
    let _runtime_guard = state.task_runtime_locks.lock(&task_id).await;
    let task = require_task(&state.pool, &task_id).await?;
    if state.downloads.lock().await.contains_key(&task_id)
        || matches!(
            task.status,
            TaskStatus::Downloading | TaskStatus::Retrying | TaskStatus::Completed
        )
    {
        return Err(AppErrorPayload::new(
            "request_profile_active",
            "Pause the task before editing request headers.",
            false,
            vec![],
        )
        .command_error());
    }
    db::update_task_request_profile(&state.pool, &task_id, &task.url, &input, replace_sensitive)
        .await?;
    state.request_headers.lock().await.remove(&task_id);
    db::insert_task_event(&state.pool, &task_id, "request_profile_updated", None).await?;
    if replace_sensitive
        && matches!(task.status, TaskStatus::NeedsAttention | TaskStatus::Failed)
        && matches!(
            task.error_code.as_deref(),
            Some("auth_headers_expired" | "auth_headers_unavailable")
        )
    {
        queue_task_for_retry_with_event(&app, &state, &task_id, "auth_headers_refreshed", None)
            .await?;
    }
    db::get_task_request_profile(&state.pool, &task_id).await
}

#[derive(Debug, Clone, serde::Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskNetworkPolicyView {
    pub task_id: String,
    pub policy: crate::download::network_policy::NetworkPolicy,
}

#[tauri::command]
#[specta::specta]
pub async fn create_network_authorization(
    state: tauri::State<'_, AppState>,
    url: String,
    source: crate::download::network_policy::TaskSource,
) -> Result<crate::download::network_policy::NetworkAuthorizationDraft, String> {
    db::create_network_authorization(&state.pool, source, url.trim()).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_task_network_policy(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<TaskNetworkPolicyView, String> {
    Ok(TaskNetworkPolicyView {
        policy: db::task_network_policy(&state.pool, &task_id).await?,
        task_id,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn revoke_task_network_authorization(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<TaskNetworkPolicyView, String> {
    let task = require_task(&state.pool, &task_id).await?;
    if matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err("Pause the task before revoking its network authorization.".to_string());
    }
    db::revoke_task_network_policy(&state.pool, &task_id).await?;
    db::insert_task_event(&state.pool, &task_id, "network_authorization_revoked", None).await?;
    if let Some(updated) = db::get_task_record(&state.pool, &task_id).await? {
        emit_task_updated_record(&app, &state.pool, &updated).await;
    }
    Ok(TaskNetworkPolicyView {
        task_id,
        policy: db::task_network_policy(&state.pool, &task.id).await?,
    })
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTaskAttentionInput {
    pub id: String,
    pub action: RecoveryAction,
    pub file_name: Option<String>,
    pub save_dir: Option<String>,
    /// Which surface performed the resolution (`recovery_center`, `manual`,
    /// ...). Defaults to `manual` for existing callers; recorded in the
    /// recovery history for state-changing actions.
    #[serde(default)]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTorrentFileSelectionInput {
    pub task_id: String,
    pub selected_file_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTorrentSeedingInput {
    pub task_id: String,
    pub enabled: bool,
    pub ratio_limit: Option<f64>,
    pub time_limit_seconds: Option<String>,
    /// FUN-11: when false, only `enabled` is written; ratio/time stay unchanged.
    /// Toggle UI must pass false so opening/closing seeding cannot wipe policy.
    #[serde(default = "default_update_torrent_limits")]
    pub update_limits: bool,
}

fn default_update_torrent_limits() -> bool {
    true
}

mod create;

pub use create::*;

mod query;

pub use query::*;

mod actions;
mod deletion;
pub use deletion::TaskDeletion;

pub use actions::*;

mod integrity;

pub use integrity::*;

#[cfg(debug_assertions)]
mod mock_seed;

#[cfg(debug_assertions)]
pub use mock_seed::*;

#[tauri::command]
#[specta::specta]
pub async fn update_torrent_file_selection(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    input: UpdateTorrentFileSelectionInput,
) -> Result<Task, String> {
    let task = require_task(&state.pool, &input.task_id).await?;
    if task.protocol != "bt" {
        return Err("File selection is only available for BitTorrent tasks.".to_string());
    }
    if matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err("Pause the torrent before changing file selection.".to_string());
    }
    let selected = input
        .selected_file_paths
        .iter()
        .map(|value| value.trim().replace('\\', "/"))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(crate::models::AppErrorPayload {
            code: "bt_file_selection_required".to_string(),
            message: "Choose at least one torrent file before downloading.".to_string(),
            recoverable: true,
            actions: vec!["check_url".to_string()],
            retry_after_at: None,
        }
        .command_error());
    }
    let mut tx = state.pool.begin().await.map_err(|e| e.to_string())?;
    db::update_task_file_selection_in_tx(&mut tx, &task.id, &selected).await?;
    db::insert_task_event_in_tx(&mut tx, &task.id, "bt_file_selection_updated", None).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    crate::state_machine::transition_task(
        &app,
        &state.pool,
        &task.id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await
    .map_err(String::from)?;
    emit_queue_changed_with_ids(&app, Some(vec![task.id.clone()]));
    state
        .scheduler
        .clone()
        .dispatch(app.clone(), state.pool.clone())
        .await;
    task_payload(&state.pool, &task.id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_torrent_seeding(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    input: UpdateTorrentSeedingInput,
) -> Result<Task, String> {
    let task = require_task(&state.pool, &input.task_id).await?;
    if task.protocol != "bt" {
        return Err("Seeding is only available for BitTorrent tasks.".to_string());
    }
    let time_limit_seconds = input
        .time_limit_seconds
        .as_deref()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0);
    db::update_torrent_seeding(
        &state.pool,
        &task.id,
        input.enabled,
        input.ratio_limit.filter(|value| *value > 0.0),
        time_limit_seconds,
        input.update_limits,
    )
    .await?;
    db::insert_task_event(
        &state.pool,
        &task.id,
        if input.enabled {
            "bt_seeding_enabled"
        } else {
            "bt_seeding_disabled"
        },
        None,
    )
    .await?;
    if !input.enabled {
        state
            .engine_registry
            .delete_runtime_task(&task, false)
            .await;
    }
    if let Some(updated) = db::get_task_record(&state.pool, &task.id).await? {
        emit_task_updated_record(&app, &state.pool, &updated).await;
    }
    task_payload(&state.pool, &task.id).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_task_proxy_settings(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<TaskProxySettings, String> {
    db::get_task_proxy_settings(&state.pool, &task_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_proxy_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    input: TaskProxySettingsInput,
) -> Result<TaskProxySettings, String> {
    let task = require_task(&state.pool, &input.task_id).await?;
    if matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
        return Err("Pause the task before changing its proxy settings.".to_string());
    }
    if input.mode == crate::models::TaskProxyMode::Custom {
        if let Some(url) = input
            .proxy_url
            .as_deref()
            .and_then(crate::proxy::normalize_proxy_url)
        {
            db::validate_task_proxy_protocol(&task.protocol, &url)?;
        }
    }
    let settings = db::upsert_task_proxy_settings(&state.pool, input).await?;
    db::insert_task_event(&state.pool, &task.id, "task_proxy_updated", None).await?;
    if let Some(updated) = db::get_task_record(&state.pool, &task.id).await? {
        emit_task_updated_record(&app, &state.pool, &updated).await;
    }
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub async fn probe_ftp_directory(
    state: tauri::State<'_, AppState>,
    input: DirectoryProbeInput,
) -> Result<FtpDirectoryProbe, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a directory URL.".to_string());
    }
    let global_proxy = state.engine_registry.proxy_config().await;
    let proxy_config = db::resolve_probe_proxy_config(
        &global_proxy,
        "ftp",
        input.proxy_mode,
        input.proxy_url.as_deref(),
        input.proxy_username.as_deref(),
        input.proxy_password.as_deref(),
        input.proxy_no_proxy.as_deref(),
    )?;
    let credentials = directory_probe_credentials(&input);
    let source = input
        .source_kind
        .unwrap_or(crate::download::network_policy::TaskSource::Manual);
    let network_policy = db::draft_network_policy(
        &state.pool,
        source,
        url,
        input.network_authorization_id.as_deref(),
    )
    .await?;
    // ARC-55: the directory probe has a total budget via bounded_probe; the
    // token is plumbed for future IPC cancellation but today's entry point
    // always observes the deadline path.
    let cancel_token = tokio_util::sync::CancellationToken::new();
    crate::download::ftp::probe_ftp_directory_url_cancellable_with_policy(
        url,
        proxy_config,
        credentials.as_ref(),
        &network_policy,
        Some(&cancel_token),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn probe_sftp_directory(
    state: tauri::State<'_, AppState>,
    input: DirectoryProbeInput,
) -> Result<SftpDirectoryProbe, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a directory URL.".to_string());
    }
    let global_proxy = state.engine_registry.proxy_config().await;
    let proxy_config = db::resolve_probe_proxy_config(
        &global_proxy,
        "sftp",
        input.proxy_mode,
        input.proxy_url.as_deref(),
        input.proxy_username.as_deref(),
        input.proxy_password.as_deref(),
        input.proxy_no_proxy.as_deref(),
    )?;
    let credentials = directory_probe_credentials(&input);
    let source = input
        .source_kind
        .unwrap_or(crate::download::network_policy::TaskSource::Manual);
    let network_policy = db::draft_network_policy(
        &state.pool,
        source,
        url,
        input.network_authorization_id.as_deref(),
    )
    .await?;
    let cancel_token = tokio_util::sync::CancellationToken::new();
    crate::download::sftp::probe_sftp_directory_url_cancellable_with_policy(
        &state.pool,
        url,
        proxy_config,
        credentials.as_ref(),
        &network_policy,
        Some(&cancel_token),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn probe_webdav_directory(
    state: tauri::State<'_, AppState>,
    input: DirectoryProbeInput,
) -> Result<WebDavDirectoryProbe, String> {
    let url = input.url.trim();
    if url.is_empty() {
        return Err("Enter a directory URL.".to_string());
    }
    let global_proxy = state.engine_registry.proxy_config().await;
    let proxy_config = db::resolve_probe_proxy_config(
        &global_proxy,
        "webdav",
        input.proxy_mode,
        input.proxy_url.as_deref(),
        input.proxy_username.as_deref(),
        input.proxy_password.as_deref(),
        input.proxy_no_proxy.as_deref(),
    )?;
    let credentials = directory_probe_credentials(&input);
    let source = input
        .source_kind
        .unwrap_or(crate::download::network_policy::TaskSource::Manual);
    let network_policy = db::draft_network_policy(
        &state.pool,
        source,
        url,
        input.network_authorization_id.as_deref(),
    )
    .await?;
    // SEC-03: obtain the client from the shared network factory so the
    // directory probe shares pooling and the proxy policy stack.
    let client = state
        .engine_registry
        .http_engine()
        .client_for_network_policy(&proxy_config, &network_policy)
        .await?;
    crate::download::webdav::probe_webdav_directory_url_cancellable_with_policy(
        &client,
        url,
        credentials.as_ref(),
        &network_policy,
        Some(&tokio_util::sync::CancellationToken::new()),
    )
    .await
}

fn directory_probe_credentials(input: &DirectoryProbeInput) -> Option<db::TaskCredentials> {
    if input.username.is_some()
        || input.password.is_some()
        || input.private_key_data.is_some()
        || input.private_key_passphrase.is_some()
    {
        Some(db::TaskCredentials {
            username: input.username.clone().unwrap_or_default(),
            password: input.password.clone().unwrap_or_default(),
            private_key_data: input.private_key_data.clone(),
            private_key_passphrase: input.private_key_passphrase.clone(),
        })
    } else {
        None
    }
}

// --- migrated scheduler functions removed (see crate::scheduler) ---

/// Lists paused tasks whose latest pause reason is schedule auto-pause.
///
/// FUN-07: used both when the download window re-opens and when schedule
/// downloads are disabled, so manually paused tasks are never auto-resumed.
pub async fn list_tasks_paused_by_schedule(pool: &sqlx::SqlitePool) -> Result<Vec<String>, String> {
    let paused_ids = db::list_paused_schedulable_tasks(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut schedule_paused = Vec::new();
    for task_id in paused_ids {
        let latest_pause = db::get_latest_pause_event_type(pool, &task_id).await?;
        if latest_pause.as_deref() == Some("paused_by_schedule") {
            schedule_paused.push(task_id);
        }
    }
    Ok(schedule_paused)
}

/// Queues schedule-paused tasks back into the download queue.
///
/// When `app` is `None` (integration tests), transitions still persist but
/// queue/progress events and scheduler dispatch are skipped.
pub async fn resume_schedule_paused_tasks(
    app: Option<&AppHandle>,
    pool: &sqlx::SqlitePool,
    scheduler: Option<std::sync::Arc<crate::scheduler::Scheduler>>,
) -> Result<Vec<String>, String> {
    let paused_ids = list_tasks_paused_by_schedule(pool).await?;
    let mut resumed = Vec::new();
    let app_handle = app.cloned();
    for task_id in &paused_ids {
        tracing::info!(task_id = %task_id, "resuming task: schedule pause cleared");
        match crate::state_machine::transition_task_with_runtime_state(
            &app_handle,
            pool,
            task_id,
            TaskStatus::Queued,
            0,
            0,
            Some("Queued"),
            Some("resumed"),
            None,
            crate::models::SegmentStatus::Pending,
            None,
            None,
        )
        .await
        {
            Ok(_) => {
                resumed.push(task_id.clone());
            }
            Err(TransitionError::Conflict { .. }) => {
                tracing::warn!(
                    task_id = %task_id,
                    "schedule auto-resume skipped: concurrent state change"
                );
            }
            Err(error) => {
                tracing::warn!(task_id = %task_id, error = %error, "schedule auto-resume failed");
            }
        }
    }
    if !resumed.is_empty() {
        if let Some(app) = app {
            emit_queue_changed_with_ids(app, Some(resumed.clone()));
            if let Some(scheduler) = scheduler {
                let dispatch_app = app.clone();
                let dispatch_pool = pool.clone();
                tauri::async_runtime::spawn(async move {
                    scheduler.dispatch(dispatch_app, dispatch_pool).await;
                });
            }
        }
    }
    Ok(resumed)
}

/// Enforces the configured download-window schedule by pausing active tasks
/// when the window is closed and resuming previously schedule-paused tasks
/// when it opens.  Called periodically by the background monitor and
/// immediately after schedule-related settings change.
pub(crate) async fn check_schedule_preemption(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
    let settings = db::get_settings(&state.pool, default_dir).await?;
    // FUN-07: disabling schedule downloads must still clear schedule auto-pauses.
    if !settings.schedule_download_window_enabled {
        resume_schedule_paused_tasks(Some(&app), &state.pool, Some(state.scheduler.clone()))
            .await?;
        return Ok(());
    }

    let window_active = db::local_time_window_active(
        &settings.schedule_download_window_start,
        &settings.schedule_download_window_end,
    );

    if !window_active {
        // Window just closed — pause downloading tasks that obey the schedule.
        let active_ids: Vec<String> = {
            let downloads = state.downloads.lock().await;
            downloads.keys().cloned().collect()
        };
        for task_id in &active_ids {
            let record = match db::get_task_record(&state.pool, task_id).await? {
                Some(r) => r,
                None => continue,
            };
            if !record.obey_schedule {
                continue;
            }
            if record.status != TaskStatus::Downloading {
                continue;
            }
            tracing::info!(task_id = %task_id, "pausing task: schedule window closed");
            if let Err(err) = pause_task(app.clone(), state.clone(), task_id.clone()).await {
                tracing::warn!(task_id = %task_id, error = %err, "schedule auto-pause failed");
                continue;
            }
            // Tag as schedule-paused AFTER the normal "paused" event so this
            // row has the highest ID and is seen as the latest pause reason.
            let _ = db::insert_task_event(&state.pool, task_id, "paused_by_schedule", None).await;
        }
    } else {
        // Window just opened — resume tasks that were paused by schedule.
        resume_schedule_paused_tasks(Some(&app), &state.pool, Some(state.scheduler.clone()))
            .await?;
    }

    Ok(())
}

/// Recomputes scheduled download and transfer policies at wall-clock
/// boundaries, settings changes, and bounded fallback intervals.
pub(crate) fn spawn_schedule_window_monitor(app: AppHandle, _state: &AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("schedule window monitor exiting (shutdown requested)");
                return;
            }

            let sleep = match db::get_settings(
                &state_ref.pool,
                super::settings::default_download_dir(&app).unwrap_or_default(),
            )
            .await
            {
                Ok(settings) => {
                    let schedule_active = settings.schedule_download_window_enabled
                        || settings.schedule_speed_limit_window_enabled;
                    let fallback =
                        std::time::Duration::from_secs(if schedule_active { 60 } else { 300 });
                    [
                        settings.schedule_download_window_enabled.then(|| {
                            db::duration_until_next_window_boundary(
                                &settings.schedule_download_window_start,
                                &settings.schedule_download_window_end,
                            )
                        }),
                        settings.schedule_speed_limit_window_enabled.then(|| {
                            db::duration_until_next_window_boundary(
                                &settings.schedule_speed_limit_window_start,
                                &settings.schedule_speed_limit_window_end,
                            )
                        }),
                    ]
                    .into_iter()
                    .flatten()
                    .fold(fallback, std::time::Duration::min)
                    .min(fallback)
                }
                Err(error) => {
                    tracing::warn!(error = %error, "schedule monitor settings read failed");
                    std::time::Duration::from_secs(60)
                }
            };

            let scheduler = state_ref.scheduler.clone();
            tokio::select! {
                _ = tokio::time::sleep(sleep) => {}
                _ = scheduler.wait_for_speed_policy_change() => {}
            }

            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("schedule window monitor exiting (shutdown requested)");
                return;
            }
            if let Err(error) = check_schedule_preemption(app.clone(), state_ref).await {
                tracing::warn!(error = %error, "schedule preemption check failed");
            }
            let state_ref = app.state::<AppState>();
            let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
            if let Err(error) = state_ref
                .scheduler
                .refresh_speed_limit_policies(&state_ref.pool, default_dir)
                .await
            {
                tracing::warn!(error = %error, "scheduled speed policy refresh failed");
            }
        }
    });
}

/// Publishes a low-frequency aggregate snapshot for native shell indicators.
/// The frontend owns the localized tooltip text; Rust only supplies numeric
/// state and keeps the polling cadence independent of window visibility.
pub(crate) fn spawn_desktop_status_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            if state
                .quit_requested
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return;
            }
            match db::task_stats_snapshot(&state.pool).await {
                Ok(snapshot) => emit_desktop_status(&app, &snapshot),
                Err(error) => tracing::debug!(error = %error, "desktop status snapshot failed"),
            }
        }
    });
}

/// Interval between background `task_requests` cleanup passes. Long enough
/// that it never contends with hot download paths, short enough that the
/// table stays bounded for long-running sessions (HLS/live, high-retry).
const REQUEST_DIAGNOSTICS_CLEANUP_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// Spawns a background task that periodically prunes the `task_requests`
/// diagnostic table. The first pass also runs shortly after startup so a
/// long-closed app gets cleaned before any new traffic arrives.
pub(crate) fn spawn_request_diagnostics_cleanup(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            REQUEST_DIAGNOSTICS_CLEANUP_INTERVAL_SECS,
        ));
        // First tick fires immediately; skip it — startup cleanup runs
        // synchronously in `lib.rs` setup() before this spawns.
        interval.tick().await;
        loop {
            interval.tick().await;
            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("request diagnostics cleanup exiting (shutdown requested)");
                break;
            }
            match db::prune_request_diagnostics(&state_ref.pool).await {
                Ok(0) => {}
                Ok(removed) => {
                    tracing::info!(removed, "pruned stale request diagnostics");
                }
                Err(error) => {
                    tracing::warn!(error = %error, "request diagnostics prune failed");
                }
            }
            match db::prune_task_events(&state_ref.pool).await {
                Ok(0) => {}
                Ok(removed) => {
                    tracing::info!(removed, "pruned stale task events");
                }
                Err(error) => {
                    tracing::warn!(error = %error, "task events prune failed");
                }
            }
        }
    });
}

/// Interval between background WAL checkpoint passes when downloads are
/// active. Long enough that it never contends with hot download paths.
const WAL_CHECKPOINT_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// E-5: Shorter WAL checkpoint interval when no downloads are active.
/// Allows WAL to be checkpointed sooner during idle periods, bounding
/// `-wal` file growth without waiting up to 6 hours.
const WAL_CHECKPOINT_IDLE_INTERVAL_SECS: u64 = 30 * 60;

/// Spawns a background task that periodically runs `PRAGMA wal_checkpoint(TRUNCATE)`
/// to bound `-wal` file growth for long-running sessions.
///
/// E-5: Uses a shorter interval (30 min) when there are no active downloads,
/// so WAL gets checkpointed sooner during idle periods. When downloads are
/// active, keeps the conservative 6-hour interval to avoid interfering with I/O.
pub(crate) fn spawn_wal_checkpoint_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // First pass: wait a short grace period after startup so the initial
        // download burst doesn't trigger an immediate checkpoint.
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        loop {
            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("wal checkpoint monitor exiting (shutdown requested)");
                break;
            }
            // E-5: Choose interval based on whether downloads are active.
            let is_idle = {
                let downloads = state_ref.downloads.lock().await;
                downloads.is_empty()
            };
            let interval_secs = if is_idle {
                WAL_CHECKPOINT_IDLE_INTERVAL_SECS
            } else {
                WAL_CHECKPOINT_INTERVAL_SECS
            };
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("wal checkpoint monitor exiting (shutdown requested)");
                break;
            }
            if let Err(error) = db::wal_checkpoint(&state_ref.pool).await {
                tracing::warn!(error = %error, "periodic WAL checkpoint failed");
            }
        }
    });
}

pub(crate) async fn resolve_task_request_headers(
    pool: &sqlx::SqlitePool,
    request_headers: TaskRequestHeaders,
    task_id: &str,
) -> Result<Vec<(String, String)>, String> {
    // Persistence checks run before cache reads so queued/retried tasks cannot
    // bypass expiry while the application remains open.
    let profile = match db::resolve_task_request_profile_headers(pool, task_id).await {
        Ok(headers) => headers,
        Err(error) => {
            request_headers.lock().await.remove(task_id);
            return Err(error);
        }
    };
    let persisted = match db::resolve_task_request_headers(pool, task_id).await {
        Ok(headers) => headers,
        Err(error) => {
            request_headers.lock().await.remove(task_id);
            return Err(error);
        }
    };
    // A browser handoff can remain in memory when encrypted persistence fails
    // during task creation. It is usable only after both database checks have
    // succeeded; an expiry/decryption error above must fail closed.
    let cached = request_headers.lock().await.get(task_id).cloned();
    Ok(merge_request_header_sources(persisted, cached, profile))
}

fn merge_request_header_sources(
    persisted: Vec<(String, String)>,
    cached: Option<Vec<(String, String)>>,
    profile: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let mut headers = persisted;
    if let Some(cached) = cached {
        for (name, value) in cached {
            headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
            headers.push((name, value));
        }
    }
    for (name, value) in profile {
        headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        headers.push((name, value));
    }
    headers
}

pub(crate) async fn queue_task_for_retry_with_event(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    event_type: &str,
    event_message: Option<&str>,
) -> Result<TaskRecord, String> {
    queue_task_for_retry_at(app, state, id, None, Some(event_type), event_message).await
}

pub(crate) async fn queue_task_for_retry_at(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    retry_after_at: Option<&str>,
    event_type: Option<&str>,
    event_message: Option<&str>,
) -> Result<TaskRecord, String> {
    match crate::state_machine::transition_task_with_runtime_state(
        app,
        &state.pool,
        id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        event_type,
        event_message,
        crate::models::SegmentStatus::Pending,
        None,
        retry_after_at,
    )
    .await
    {
        Ok(_) => {}
        Err(TransitionError::Conflict { .. }) => {
            return Err("Task state changed concurrently, please refresh.".to_string());
        }
        Err(error) => return Err(error.into()),
    }
    // A user-directed retry starts a fresh automatic budget. Keeping the old
    // attempt count would make a transient failure exhaust immediately after
    // recovery, while clearing it here cannot race an active worker because
    // the caller holds the task runtime lock.
    db::clear_auto_retry_state(&state.pool, id).await?;
    let task = require_task(&state.pool, id).await?;
    emit_task_progress_snapshot(app, &task);
    emit_queue_changed_with_ids(app, Some(vec![id.to_string()]));
    state.scheduler.notify_retry_schedule_changed();
    if retry_after_at.is_none() {
        // ARC-32: same constraint as restart — the caller (retry_task/
        // resume_task/resolve_task_attention) holds the per-task runtime lock,
        // so dispatch must run detached and only take task locks after the
        // caller unwinds.
        state
            .scheduler
            .dispatch_detached(app.clone(), state.pool.clone());
    }
    require_task(&state.pool, id).await
}

async fn update_recovery_target(
    app: &AppHandle,
    state: &AppState,
    task: &TaskRecord,
    input: &ResolveTaskAttentionInput,
) -> Result<(), String> {
    let save_dir = input
        .save_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&task.save_dir));
    std::fs::create_dir_all(&save_dir)
        .map_err(|e| format!("Could not create the download directory: {e}"))?;

    let requested_file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&task.file_name);
    let final_path = unique_final_path(&save_dir, requested_file_name);
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested_file_name)
        .to_string();

    db::update_task_save_target(
        &state.pool,
        &task.id,
        &file_name,
        &save_dir.to_string_lossy(),
        &final_path.to_string_lossy(),
    )
    .await?;
    if let Some(updated) = db::get_task_record(&state.pool, &task.id).await? {
        emit_task_updated_record(app, &state.pool, &updated).await;
    }
    Ok(())
}

async fn restart_task_from_beginning(
    app: &AppHandle,
    state: &AppState,
    task: &TaskRecord,
) -> Result<TaskRecord, String> {
    // R-2.3: Caller (resolve_task_attention) must already hold the per-task
    // runtime lock. tokio::sync::Mutex is not re-entrant, so we do not
    // re-acquire here.
    // ARC-45: cancel + drain, not bare abort — abort takes effect at the
    // next await, so a worker mid-write keeps the temp handle open and on
    // Windows the removal below becomes delete-pending while the new
    // worker's create() hits ACCESS_DENIED.
    crate::remove_and_drain_control(
        &state.downloads,
        &state.request_headers,
        &task.id,
        crate::USER_ACTION_DRAIN_GRACE,
    )
    .await?;
    // ARC-45: removal failures no longer abort the restart — a stale artifact
    // must not leave the task half-reset; the new worker creates/truncates
    // its temp files anyway.
    if let Some(temp_path) = task.temp_path.as_deref() {
        if let Err(error) = remove_task_path(temp_path) {
            tracing::warn!(task_id = %task.id, path = %temp_path, error = %error, "restart: stale temp removal failed, continuing");
        }
    }
    // Restart-from-beginning discards every resumable artifact, not just the
    // recorded temp: DASH staging survived here (its recorded temp is the
    // remux output, not the staging dir) and metalink `.part-N` siblings were
    // never removed at all (enumeration lives in download::artifacts).
    let file_temps: Vec<String> = db::list_task_file_records(&state.pool, &task.id)
        .await?
        .iter()
        .filter_map(|file| file.temp_path.clone())
        .collect();
    for artifact in crate::download::artifacts::task_auxiliary_artifacts(task, &file_temps).await {
        if let Err(error) = remove_task_path(&artifact.to_string_lossy()) {
            tracing::warn!(task_id = %task.id, path = %artifact.to_string_lossy(), error = %error, "restart: stale artifact removal failed, continuing");
        }
    }

    let engine = state.engine_registry.engine_for_uri(&task.url)?;
    let request_headers =
        resolve_task_request_headers(&state.pool, state.request_headers.clone(), &task.id).await?;
    let credentials = db::resolve_task_credentials(&state.pool, &task.id).await?;
    let global_proxy = state.engine_registry.proxy_config().await;
    let proxy_config =
        db::resolve_task_proxy_config(&state.pool, &task.id, &task.protocol, &global_proxy).await?;
    let probe = engine
        .probe(ProbeRequest {
            uri: task.url.clone(),
            source: None,
            request_headers,
            pool: Some(state.pool.clone()),
            task_id: Some(task.id.clone()),
            credentials,
            proxy_config: Some(proxy_config),
            app: None,
            request_id: None,
            cancel_token: None,
            network_policy: db::task_network_policy(&state.pool, &task.id).await?,
        })
        .await?;
    db::update_task_remote_metadata(
        &state.pool,
        &task.id,
        db::TaskRemoteMetadataUpdate {
            final_url: &probe.resolved_uri,
            total_size: probe.total_size,
            etag: probe.etag.as_deref(),
            last_modified: probe.last_modified.as_deref(),
            content_type: probe.content_type.as_deref(),
            supports_resume: probe.capabilities.supports_resume,
            supports_parallel: probe.capabilities.supports_parallel,
            supports_multi_file: probe.capabilities.supports_multi_file,
            source_key: &probe.source_key,
        },
    )
    .await?;
    db::delete_segments_for_task(&state.pool, &task.id).await?;
    if task.protocol == "hls" {
        db::reset_hls_segments_for_task(&state.pool, &task.id).await?;
    }
    db::reset_task_download_state(&state.pool, &task.id).await?;
    let settings = db::get_settings(
        &state.pool,
        super::settings::default_download_dir(app).unwrap_or_default(),
    )
    .await?;
    let task = require_task(&state.pool, &task.id).await?;
    db::ensure_task_segments_with_settings(&state.pool, &task, &settings).await?;
    emit_task_progress_snapshot(app, &task);
    emit_task_updated_record(app, &state.pool, &task).await;
    emit_queue_changed_with_ids(app, Some(vec![task.id.clone()]));
    // ARC-32: the caller (resolve_task_attention) still holds this task's
    // runtime lock, and the task is already Queued — awaiting dispatch here
    // self-deadlocks when start_task re-acquires the same non-reentrant lock.
    // Detached dispatch runs after the caller unwinds and releases the lock.
    state
        .scheduler
        .dispatch_detached(app.clone(), state.pool.clone());
    require_task(&state.pool, &task.id).await
}

pub(crate) fn restart_required_error_code(code: &str) -> bool {
    matches!(
        code,
        "remote_changed"
            | "resume_unavailable"
            | "temp_file_missing"
            | "temp_file_smaller_than_progress"
    )
}

fn task_error_code(task: &TaskRecord) -> Option<String> {
    // ARC-16: column or JSON `.code` only — no English substring inference.
    AppErrorPayload::code_from_stored(task.error_code.as_deref(), task.error_message.as_deref())
}

pub(crate) async fn prepare_task_for_download(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    engine_registry: &EngineRegistry,
    task: TaskRecord,
    request_headers: &[(String, String)],
) -> Result<TaskRecord, String> {
    if task.status == TaskStatus::NeedsAttention {
        return Err(AppErrorPayload::new(
            "remote_changed",
            "Remote file changed. Restart download to avoid corruption.",
            false,
            vec!["restart", "check_url"],
        )
        .command_error());
    }

    if is_bt_protocol(&task.protocol)
        || is_hls_protocol(&task.protocol)
        || is_dash_protocol(&task.protocol)
        || is_metalink_protocol(&task.protocol)
        || is_sftp_protocol(&task.protocol)
    {
        db::ensure_task_segments(pool, &task).await?;
        return require_task(pool, &task.id).await;
    }

    let segments = db::ensure_task_segments(pool, &task).await?;
    let temp_path = task
        .temp_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "Task is missing a temporary path.".to_string())?;
    let temp_exists = tokio::fs::try_exists(&temp_path).await.unwrap_or(false);
    let temp_size = match tokio::fs::metadata(&temp_path).await {
        Ok(metadata) => i64::try_from(metadata.len()).unwrap_or(i64::MAX),
        Err(_) => 0,
    };
    if let Some(message) = segment_resume_error(
        &segments,
        task.downloaded_bytes,
        temp_exists,
        temp_size,
        task.total_size,
        // Unknown-size tasks must reach the fresh probe before rejecting a
        // partial file: an older row may have been created before the server
        // exposed a verifiable 206 + validator pair. The probe below is the
        // authority for whether that partial file can be resumed safely.
        task.supports_resume || (task.total_size == 0 && temp_size > 0),
    ) {
        fail_task_and_segments(app, pool, &task.id, &message).await?;
        db::insert_task_event(pool, &task.id, "resume_blocked", Some(&message)).await?;
        return Err(message);
    }

    if temp_size > 0 {
        let uri = if matches!(task.protocol.as_str(), "http" | "https") {
            task.url.clone()
        } else {
            task.final_url.as_deref().unwrap_or(&task.url).to_string()
        };
        let engine = engine_registry.engine_for_uri(&uri)?;
        let credentials = db::resolve_task_credentials(pool, &task.id).await?;
        let global_proxy = engine_registry.proxy_config().await;
        let proxy_config =
            db::resolve_task_proxy_config(pool, &task.id, &task.protocol, &global_proxy).await?;
        let probe = engine
            .probe(ProbeRequest {
                uri,
                source: None,
                request_headers: request_headers.to_vec(),
                pool: Some(pool.clone()),
                task_id: Some(task.id.clone()),
                credentials,
                proxy_config: Some(proxy_config),
                app: None,
                request_id: None,
                cancel_token: None,
                network_policy: db::task_network_policy(pool, &task.id).await?,
            })
            .await?;
        if let Some(message) = resume_mismatch_message(&task, &probe) {
            crate::state_machine::transition_task(
                app,
                pool,
                &task.id,
                TaskStatus::NeedsAttention,
                0,
                0,
                Some(&message),
                Some("resume_blocked"),
            )
            .await
            .map_err(String::from)?;
            db::update_segments_status_for_task(
                pool,
                &task.id,
                crate::models::SegmentStatus::Failed,
                Some(&message),
            )
            .await?;
            return Err(message);
        }
        if task.total_size == 0 && probe.total_size == 0 {
            // Persist the validator/capability pair established by the fresh
            // unknown-size range probe so the download worker applies the same
            // safety contract when it opens the resumed stream.
            db::update_task_remote_metadata(
                pool,
                &task.id,
                db::TaskRemoteMetadataUpdate {
                    final_url: &probe.resolved_uri,
                    total_size: probe.total_size,
                    etag: probe.etag.as_deref(),
                    last_modified: probe.last_modified.as_deref(),
                    content_type: probe.content_type.as_deref(),
                    supports_resume: probe.capabilities.supports_resume,
                    supports_parallel: probe.capabilities.supports_parallel,
                    supports_multi_file: probe.capabilities.supports_multi_file,
                    source_key: &probe.source_key,
                },
            )
            .await?;
        }
        if let Some(message) = resume_decision_message(&task, &probe) {
            db::insert_task_event(pool, &task.id, "resume_checked", Some(&message)).await?;
        }
    }

    if segments.len() == 1 && temp_size > segments[0].downloaded_until {
        db::update_task_and_segment_progress(
            pool,
            &task.id,
            &segments[0].id,
            temp_size,
            0,
            0,
            task.status,
        )
        .await?;
    }

    require_task(pool, &task.id).await
}

fn is_bt_protocol(protocol: &str) -> bool {
    matches!(protocol, "bt" | "magnet")
}

fn is_hls_protocol(protocol: &str) -> bool {
    protocol == "hls"
}

fn is_dash_protocol(protocol: &str) -> bool {
    protocol == "dash"
}

fn is_metalink_protocol(protocol: &str) -> bool {
    protocol == "metalink"
}

fn is_sftp_protocol(protocol: &str) -> bool {
    protocol == "sftp"
}

// URL classification functions have been consolidated into `crate::download::url_classify`; re-exported here to keep
// call sites like `super::is_torrent_url` unchanged.
pub(crate) use crate::download::url_classify::{is_dash_url, is_metalink_url, is_torrent_url};

async fn fail_task_and_segments(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    task_id: &str,
    message: &str,
) -> Result<(), String> {
    crate::state_machine::transition_task(
        app,
        pool,
        task_id,
        TaskStatus::Failed,
        0,
        0,
        Some(message),
        None,
    )
    .await
    .map_err(String::from)?;
    db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(message),
    )
    .await
}

async fn require_task(pool: &sqlx::SqlitePool, id: &str) -> Result<TaskRecord, String> {
    db::get_task_record(pool, id)
        .await?
        .ok_or_else(|| "Task not found.".to_string())
}

async fn task_payload(pool: &sqlx::SqlitePool, id: &str) -> Result<Task, String> {
    let record = require_task(pool, id).await?;
    task_from_record_with_files(pool, record).await
}

async fn tasks_from_records_with_files(
    pool: &sqlx::SqlitePool,
    records: Vec<TaskRecord>,
) -> Result<Vec<Task>, String> {
    let task_ids = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let mut files_by_task_id = db::list_task_file_records_for_tasks(pool, &task_ids).await?;
    let mut checksums_by_task_id =
        db::list_task_checksum_records_for_tasks(pool, &task_ids).await?;
    let completed_at_by_task_id = db::completed_at_for_tasks(pool, &task_ids).await?;
    Ok(records
        .into_iter()
        .map(|record| {
            let files = files_by_task_id.remove(&record.id).unwrap_or_default();
            let checksums = checksums_by_task_id.remove(&record.id).unwrap_or_default();
            let completed_at = completed_at_by_task_id.get(&record.id).cloned().flatten();
            task_from_record_and_files(record, files, checksums, completed_at)
        })
        .collect())
}

pub(crate) async fn task_from_record_with_files(
    pool: &sqlx::SqlitePool,
    record: TaskRecord,
) -> Result<Task, String> {
    let files = db::list_task_file_records(pool, &record.id).await?;
    let checksums = db::list_task_checksum_records(pool, &record.id).await?;
    let completed_at = db::completed_at_for_task(pool, &record.id).await?;
    Ok(task_from_record_and_files(
        record,
        files,
        checksums,
        completed_at,
    ))
}

fn task_from_record_and_files(
    record: TaskRecord,
    files: Vec<TaskFileRecord>,
    checksums: Vec<TaskChecksumRecord>,
    completed_at: Option<String>,
) -> Task {
    let mut task = Task::from(record);
    task.completed_at = completed_at;
    task.files = files.into_iter().map(Into::into).collect();
    task.checksums = checksums.into_iter().map(Into::into).collect();
    task
}

pub(crate) fn emit_task_progress_snapshot(app: &AppHandle, task: &TaskRecord) {
    let payload = crate::models::TaskProgressPayload {
        task_id: task.id.clone(),
        downloaded_bytes: task.downloaded_bytes.to_string(),
        total_size: task.total_size.to_string(),
        speed_bps: task.speed_bps.to_string(),
        connection_count: task.connection_count,
        status: task.status,
    };
    emit_task_progress(app, &payload);
}

/// Delete a file or directory, optionally sending to the OS trash/recycle bin.
///
/// When `use_trash` is true, the `trash` crate attempts to move the path to
/// the system's recycle bin. If that fails, an error is returned — the file
/// is **not** permanently deleted, giving the caller a chance to warn the user.
/// When `use_trash` is false, deletes permanently. `NotFound` errors are
/// silently ignored in both modes.
pub(super) fn delete_path(path: &str, use_trash: bool) -> Result<(), String> {
    let path_obj = std::path::Path::new(path);
    if !path_obj.exists() {
        return Ok(());
    }

    if use_trash {
        if let Err(error) = trash::delete(path_obj) {
            // Return the error — do NOT fall through to permanent deletion.
            // Silent permanent deletion when the user expected trash is a
            // data-loss footgun (the user checks the recycle bin and finds nothing).
            return Err(format!(
                "Could not move {path} to the recycle bin: {error}. The file was not deleted."
            ));
        }
        return Ok(());
    }

    // Permanent deletion
    if path_obj.is_dir() {
        match std::fs::remove_dir_all(path_obj) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not delete {path}: {error}")),
        }
    } else {
        match std::fs::remove_file(path_obj) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not delete {path}: {error}")),
        }
    }
}

/// Delete a task's temporary file/folder. Always permanent (temp files
/// should not clutter the recycle bin).
fn remove_task_path(path: &str) -> Result<(), String> {
    delete_path(path, false)
}

#[cfg(test)]
mod request_header_tests {
    use super::merge_request_header_sources;
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::Mutex;

    #[test]
    fn browser_cache_fills_persistence_gap_and_profile_wins_by_name() {
        let merged = merge_request_header_sources(
            vec![("User-Agent".to_string(), "stored-agent".to_string())],
            Some(vec![
                ("Cookie".to_string(), "session=memory".to_string()),
                ("User-Agent".to_string(), "browser-agent".to_string()),
            ]),
            vec![("user-agent".to_string(), "profile-agent".to_string())],
        );

        assert_eq!(
            merged,
            vec![
                ("Cookie".to_string(), "session=memory".to_string()),
                ("user-agent".to_string(), "profile-agent".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn resolver_uses_cached_browser_headers_when_persistence_row_is_missing() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        sqlx::query(
            "CREATE TABLE task_request_profiles (
                task_id TEXT PRIMARY KEY, origin TEXT NOT NULL,
                public_ciphertext TEXT NOT NULL, public_nonce TEXT NOT NULL,
                sensitive_ciphertext TEXT, sensitive_nonce TEXT,
                sensitive_names_json TEXT NOT NULL, sensitive_expires_at TEXT,
                sensitive_expired INTEGER NOT NULL, updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("profile table");
        sqlx::query(
            "CREATE TABLE task_request_headers (
                task_id TEXT PRIMARY KEY, headers_json TEXT NOT NULL,
                headers_ciphertext TEXT, nonce TEXT, expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL, last_used_at TEXT, source_browser TEXT
            )",
        )
        .execute(&pool)
        .await
        .expect("headers table");

        let cached = Arc::new(Mutex::new(HashMap::from([(
            "task-1".to_string(),
            vec![("Cookie".to_string(), "session=memory".to_string())],
        )])));
        let resolved = super::resolve_task_request_headers(&pool, cached, "task-1")
            .await
            .expect("memory fallback");
        assert_eq!(
            resolved,
            vec![("Cookie".to_string(), "session=memory".to_string())]
        );
    }
}
