use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use reqwest::Url;
use serde::Deserialize;
use specta::Type;
use sqlx::Row;
use tauri::{AppHandle, Manager};

use super::task_file_planning::unique_final_path;
pub use super::task_resume::{
    local_resume_error, resume_decision_message, resume_mismatch_message, segment_resume_error,
};

use crate::{
    db,
    download::{DownloadContext, EngineRegistry, ProbeRequest},
    events::{
        emit_completion_action_requested, emit_queue_changed, emit_task_progress,
        emit_task_updated_record,
    },
    logging::sanitize_url,
    models::{
        AppErrorPayload, CompletionAction, CompletionActionRequestedPayload, FtpDirectoryProbe,
        HashVerificationStatus, RecoveryAction, SftpDirectoryProbe, Task, TaskChecksumRecord,
        TaskFileRecord, TaskProxySettings, TaskProxySettingsInput, TaskRecord, TaskStatus,
        WebDavDirectoryProbe,
    },
    platform, AppState, DownloadControl, TaskRequestHeaders,
};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTaskAttentionInput {
    pub id: String,
    pub action: RecoveryAction,
    pub file_name: Option<String>,
    pub save_dir: Option<String>,
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
}

mod create;

pub use create::*;

mod query;

pub use query::*;

mod actions;

pub use actions::*;

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
        }
        .command_error());
    }
    db::update_task_file_selection(&state.pool, &task.id, &selected).await?;
    db::update_task_status(
        &state.pool,
        &task.id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await?;
    db::insert_task_event(&state.pool, &task.id, "bt_file_selection_updated", None).await?;
    emit_queue_changed(&app);
    schedule_queued_tasks(app.clone(), state.inner()).await;
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
    url: String,
) -> Result<FtpDirectoryProbe, String> {
    let proxy_config = state.engine_registry.proxy_config().await;
    crate::download::ftp::probe_ftp_directory_url(&url, proxy_config).await
}

#[tauri::command]
#[specta::specta]
pub async fn probe_sftp_directory(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<SftpDirectoryProbe, String> {
    let proxy_config = state.engine_registry.proxy_config().await;
    crate::download::sftp::probe_sftp_directory_url(&state.pool, &url, proxy_config).await
}

#[tauri::command]
#[specta::specta]
pub async fn probe_webdav_directory(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<WebDavDirectoryProbe, String> {
    let proxy_config = state.engine_registry.proxy_config().await;
    crate::download::webdav::probe_webdav_directory_url(&url, proxy_config).await
}

pub(crate) async fn schedule_queued_tasks(app: AppHandle, state: &AppState) {
    schedule_queued_tasks_inner(
        app,
        state.pool.clone(),
        state.downloads.clone(),
        state.request_headers.clone(),
        state.scheduler.clone(),
        state.speed_limiter.clone(),
        state.engine_registry.clone(),
    )
    .await;
}

pub(crate) async fn schedule_retry_after_wakeup(app: AppHandle, state: &AppState) {
    let Some(next) = (match db::next_retry_after_at(&state.pool).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(error = %error, "failed to inspect retry-after queue");
            None
        }
    }) else {
        return;
    };
    let when = chrono::DateTime::parse_from_rfc3339(&next)
        .map(|value| value.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let now = chrono::Utc::now();
    let delay = when
        .signed_duration_since(now)
        .to_std()
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    if delay.is_zero() {
        schedule_queued_tasks(app, state).await;
    } else {
        spawn_schedule_queued_tasks_after(app, state, delay);
    }
}

async fn schedule_queued_tasks_inner(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
) {
    let _guard = scheduler.lock().await;

    // Read settings and queued list ONCE outside the loop.
    // Previously these were re-fetched every iteration (~240 DB round trips per burst).
    let default_dir = super::settings::default_download_dir(&app).unwrap_or_default();
    let settings = match db::get_settings(&pool, default_dir).await {
        Ok(settings) => settings,
        Err(error) => {
            tracing::error!(error = %error, "failed to load settings for scheduler");
            return;
        }
    };
    let queued_limit = i64::from(settings.max_active_tasks)
        .saturating_mul(i64::from(settings.max_connections_per_host).max(1))
        .clamp(
            i64::from(settings.max_active_tasks).max(1),
            db::DEFAULT_TASK_PAGE_SIZE,
        );
    let queued = match db::list_queued_task_records(&pool, queued_limit).await {
        Ok(tasks) => tasks,
        Err(error) => {
            tracing::error!(error = %error, "failed to load queued tasks");
            return;
        }
    };
    if queued.is_empty() {
        return;
    }

    let mut active_count = downloads.lock().await.len() as i32;
    let available = settings.max_active_tasks.saturating_sub(active_count);
    if available <= 0 {
        tracing::debug!(
            active_count,
            max_active_tasks = settings.max_active_tasks,
            "scheduler has no available slots"
        );
        return;
    }

    tracing::debug!(
        available,
        queued_count = queued.len(),
        "scheduler dispatching queued tasks"
    );

    let host_limit = usize::try_from(settings.max_connections_per_host)
        .unwrap_or(usize::try_from(db::DEFAULT_MAX_CONNECTIONS_PER_HOST).unwrap_or(8))
        .max(1);

    for task in queued {
        if active_count >= settings.max_active_tasks {
            break;
        }
        if settings.schedule_download_window_enabled
            && task.obey_schedule
            && !db::local_time_window_active(
                &settings.schedule_download_window_start,
                &settings.schedule_download_window_end,
            )
        {
            tracing::debug!(
                task_id = %task.id,
                "scheduler deferred task outside configured download window"
            );
            continue;
        }
        let host_used = host_connection_slots(&downloads, &task.source_key).await;
        if host_used >= host_limit {
            tracing::debug!(
                task_id = %task.id,
                source_key = %task.source_key,
                host_used,
                host_limit,
                "scheduler deferred task because host connection limit is full"
            );
            continue;
        }
        let planned_slots = db::planned_segment_count_with_plan(
            &task,
            db::parse_multi_connection_threshold_bytes(
                &settings.multi_connection_threshold_bytes,
            ),
            settings.segment_count,
        )
        .min(host_limit.saturating_sub(host_used))
        .max(1);
        let task_id = task.id.clone();
        match start_task_download(
            DownloadStartContext {
                app: app.clone(),
                pool: pool.clone(),
                downloads: downloads.clone(),
                request_headers: request_headers.clone(),
                scheduler: scheduler.clone(),
                speed_limiter: speed_limiter.clone(),
                engine_registry: engine_registry.clone(),
            },
            task,
            planned_slots,
        )
        .await
        {
            Ok(()) => {
                // Task was inserted into the downloads map — count it.
                active_count += 1;
            }
            Err(error) => {
                // start_task_download failed before (or during) insert; the
                // spawned task (if any) will clean up the downloads entry.
                // Do NOT increment active_count — the task is not consuming a slot.
                match db::get_task_record(&pool, &task_id).await {
                    Ok(Some(current)) if current.status == TaskStatus::Queued => {
                        mark_download_failed(&app, &pool, &task_id, error).await;
                    }
                    Ok(Some(current)) => {
                        emit_task_progress_snapshot(&app, &current);
                        emit_task_updated_record(&app, &pool, &current).await;
                    }
                    _ => {}
                }
            }
        }
    }

    emit_queue_changed(&app);
}

struct DownloadStartContext {
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
}

async fn start_task_download(
    context: DownloadStartContext,
    task: TaskRecord,
    connection_limit: usize,
) -> Result<(), String> {
    let DownloadStartContext {
        app,
        pool,
        downloads,
        request_headers,
        scheduler,
        speed_limiter,
        engine_registry,
    } = context;

    let task_request_headers =
        resolve_task_request_headers(&pool, request_headers.clone(), &task.id).await?;
    let global_proxy_config = engine_registry.proxy_config().await;
    let task_proxy_config =
        db::resolve_task_proxy_config(&pool, &task.id, &task.protocol, &global_proxy_config)
            .await?;
    let task =
        prepare_task_for_download(&pool, &engine_registry, task, &task_request_headers).await?;
    if downloads.lock().await.contains_key(&task.id) {
        tracing::debug!(task_id = %task.id, "download already active, skipping start");
        return Ok(());
    }

    tracing::info!(
        task_id = %task.id,
        url = %sanitize_url(&task.url),
        total_size = task.total_size,
        connection_limit,
        "starting task download"
    );

    let connection_count = i32::try_from(connection_limit.max(1)).unwrap_or(1);
    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        connection_count,
        Some("Downloading"),
        None,
    )
    .await?;
    db::insert_task_event(&pool, &task.id, "started", None).await?;
    if let Some(current) = db::get_task_record(&pool, &task.id).await? {
        emit_task_updated_record(&app, &pool, &current).await;
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let finish = Arc::new(AtomicBool::new(false));
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let downloads_map = downloads.clone();
    let task_id = task.id.clone();
    let map_task_id = task.id.clone();
    let source_key = task.source_key.clone();
    let task_app = app.clone();
    let task_cancel = cancel.clone();
    let task_cancel_token = cancel_token.clone();
    let task_finish = finish.clone();
    let task_pool = pool.clone();
    let task_scheduler = scheduler.clone();
    let state_speed_limiter = speed_limiter.clone();
    let task_engine_registry = engine_registry.clone();

    let handle = tokio::spawn(async move {
        let engine = match task_engine_registry.engine_for_uri(&task.url) {
            Ok(engine) => engine,
            Err(error) => {
                mark_download_failed(&task_app, &task_pool, &task_id, error).await;
                let _ = downloads_map.lock().await.remove(&task_id);
                spawn_schedule_queued_tasks(
                    task_app.clone(),
                    task_pool.clone(),
                    downloads_map.clone(),
                    request_headers.clone(),
                    task_scheduler.clone(),
                    state_speed_limiter.clone(),
                    task_engine_registry.clone(),
                );
                return;
            }
        };

        let task_limit_bps = db::parse_speed_limit_bps(task.task_speed_limit_bps.as_deref());
        let scheduled_limit_bps = db::get_settings(
            &task_pool,
            super::settings::default_download_dir(&task_app).unwrap_or_default(),
        )
        .await
        .ok()
        .and_then(|settings| {
            if settings.schedule_speed_limit_window_enabled
                && db::local_time_window_active(
                    &settings.schedule_speed_limit_window_start,
                    &settings.schedule_speed_limit_window_end,
                )
            {
                db::parse_speed_limit_bps(settings.schedule_speed_limit_bps.as_deref())
            } else {
                None
            }
        });
        let effective_task_limit = min_optional_limit(task_limit_bps, scheduled_limit_bps);
        let task_speed_limiter = crate::download::GlobalSpeedLimiter::with_parent(
            state_speed_limiter.clone(),
            effective_task_limit,
        );
        // Spawn the download as a nested task so that a panic inside the engine
        // is caught by tokio's JoinHandle (returns Err(JoinError) with is_panic())
        // instead of aborting the entire process. Requires panic = "unwind".
        // Clone the shared handles that are still needed after the nested spawn
        // so the outer task retains access once `async move` captures them.
        let inner_app = task_app.clone();
        let inner_pool = task_pool.clone();
        let inner_cancel = task_cancel.clone();
        let download_handle = tokio::spawn(async move {
            engine
                .download(DownloadContext {
                    app: inner_app,
                    pool: inner_pool,
                    task,
                    cancel: inner_cancel,
                    cancel_token: task_cancel_token.clone(),
                    finish: task_finish.clone(),
                    speed_limiter: task_speed_limiter,
                    connection_limit,
                    request_headers: task_request_headers.clone(),
                    proxy_config: task_proxy_config,
                })
                .await
        });
        let result = match download_handle.await {
            Ok(result) => result,
            Err(join_error) => {
                let message = if join_error.is_panic() {
                    let payload = join_error.into_panic();
                    payload
                        .downcast_ref::<String>()
                        .map(|s| s.as_str())
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("unknown panic")
                        .to_string()
                } else {
                    "Download task was cancelled.".to_string()
                };
                tracing::error!(
                    task_id = %task_id,
                    error = %message,
                    "download task terminated unexpectedly"
                );
                Err(message)
            }
        };
        let canceled = task_cancel.load(Ordering::SeqCst);
        let _ = downloads_map.lock().await.remove(&task_id);
        let _ = request_headers.lock().await.remove(&task_id);

        if let Err(error) = result {
            if !canceled {
                mark_download_failed(&task_app, &task_pool, &task_id, error).await;
            }
        } else if !canceled {
            match verify_task_hash_with_pool(&task_pool, &task_id).await {
                Ok(state) if state.status != HashVerificationStatus::NotRequested => {
                    tracing::info!(
                        task_id = %task_id,
                        status = ?state.status,
                        "hash verification completed"
                    );
                    if let Ok(Some(current)) = db::get_task_record(&task_pool, &task_id).await {
                        emit_task_updated_record(&task_app, &task_pool, &current).await;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        task_id = %task_id,
                        error = %error,
                        "hash verification failed to run"
                    );
                }
            }
            maybe_emit_completion_action(&task_app, &task_pool, downloads_map.clone()).await;
        }

        spawn_schedule_queued_tasks(
            task_app,
            task_pool,
            downloads_map,
            request_headers,
            task_scheduler,
            state_speed_limiter,
            task_engine_registry,
        );
    });

    downloads.lock().await.insert(
        map_task_id,
        DownloadControl {
            cancel,
            cancel_token,
            finish,
            handle,
            source_key,
            connection_slots: connection_limit.max(1),
        },
    );

    emit_queue_changed(&app);
    Ok(())
}

async fn host_connection_slots(
    downloads: &Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    source_key: &str,
) -> usize {
    downloads
        .lock()
        .await
        .values()
        .filter(|control| control.source_key == source_key)
        .map(|control| control.connection_slots)
        .sum()
}

fn min_optional_limit(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

async fn maybe_emit_completion_action(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
) {
    if !downloads.lock().await.is_empty() {
        return;
    }
    if db::list_queued_task_records(pool, 1)
        .await
        .map(|tasks| !tasks.is_empty())
        .unwrap_or(true)
    {
        return;
    }
    let Ok(settings) = db::get_settings(
        pool,
        super::settings::default_download_dir(app).unwrap_or_default(),
    )
    .await
    else {
        return;
    };
    if settings.completion_action == CompletionAction::None {
        return;
    }
    if settings.completion_action == CompletionAction::RunCommand {
        if let Err(error) = platform::run_user_command(&settings.completion_run_command) {
            tracing::warn!(error = %error, "completion run_command failed");
        }
        return;
    }
    emit_completion_action_requested(
        app,
        &CompletionActionRequestedPayload {
            action: settings.completion_action,
            countdown_seconds: settings.completion_countdown_seconds,
        },
    );
}

fn spawn_schedule_queued_tasks(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    downloads: Arc<tokio::sync::Mutex<std::collections::HashMap<String, DownloadControl>>>,
    request_headers: TaskRequestHeaders,
    scheduler: Arc<tokio::sync::Mutex<()>>,
    speed_limiter: Arc<crate::download::GlobalSpeedLimiter>,
    engine_registry: Arc<EngineRegistry>,
) {
    tokio::spawn(async move {
        schedule_queued_tasks_inner(
            app,
            pool,
            downloads,
            request_headers,
            scheduler,
            speed_limiter,
            engine_registry,
        )
        .await;
    });
}

fn spawn_schedule_queued_tasks_after(app: AppHandle, state: &AppState, delay: std::time::Duration) {
    let pool = state.pool.clone();
    let downloads = state.downloads.clone();
    let request_headers = state.request_headers.clone();
    let scheduler = state.scheduler.clone();
    let speed_limiter = state.speed_limiter.clone();
    let engine_registry = state.engine_registry.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        schedule_queued_tasks_inner(
            app,
            pool,
            downloads,
            request_headers,
            scheduler,
            speed_limiter,
            engine_registry,
        )
        .await;
    });
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
    if !settings.schedule_download_window_enabled {
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
            let _ = db::insert_task_event(
                &state.pool,
                task_id,
                "paused_by_schedule",
                None,
            )
            .await;
        }
    } else {
        // Window just opened — resume tasks that were paused by schedule.
        let paused_rows = sqlx::query(
            "SELECT id FROM tasks WHERE status = 'paused' AND obey_schedule = 1",
        )
        .fetch_all(&state.pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut resumed_any = false;
        for row in &paused_rows {
            let task_id: String = row.get("id");
            let latest_pause =
                db::get_latest_pause_event_type(&state.pool, &task_id).await?;
            if latest_pause.as_deref() != Some("paused_by_schedule") {
                continue;
            }
            tracing::info!(task_id = %task_id, "resuming task: schedule window opened");
            db::insert_task_event(&state.pool, &task_id, "resumed", None).await?;
            if let Err(err) = queue_task_for_retry(&app, state.inner(), &task_id).await {
                tracing::warn!(task_id = %task_id, error = %err, "schedule auto-resume failed");
            }
            resumed_any = true;
        }
        if resumed_any {
            emit_queue_changed(&app);
        }
    }

    Ok(())
}

/// Spawns a background task that checks the schedule window every 60 seconds
/// and preempts running tasks or resumes paused tasks as needed.
pub(crate) fn spawn_schedule_window_monitor(app: AppHandle, _state: &AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        // First tick fires immediately; skip it.
        interval.tick().await;
        loop {
            interval.tick().await;
            // Exit gracefully when the app is shutting down so we don't
            // access dropped state.
            let state_ref = app.state::<AppState>();
            if state_ref.quit_requested.load(Ordering::SeqCst) {
                tracing::debug!("schedule window monitor exiting (shutdown requested)");
                break;
            }
            if let Err(error) =
                check_schedule_preemption(app.clone(), state_ref).await
            {
                tracing::warn!(error = %error, "schedule preemption check failed");
            }
        }
    });
}

async fn resolve_task_request_headers(
    pool: &sqlx::SqlitePool,
    request_headers: TaskRequestHeaders,
    task_id: &str,
) -> Result<Vec<(String, String)>, String> {
    if let Some(headers) = request_headers.lock().await.get(task_id).cloned() {
        return Ok(headers);
    }
    let persisted = db::resolve_task_request_headers(pool, task_id).await?;
    if !persisted.is_empty() {
        request_headers
            .lock()
            .await
            .insert(task_id.to_string(), persisted.clone());
    }
    Ok(persisted)
}

async fn queue_task_for_retry(
    app: &AppHandle,
    state: &AppState,
    id: &str,
) -> Result<TaskRecord, String> {
    queue_task_for_retry_at(app, state, id, None).await
}

async fn queue_task_for_retry_at(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    retry_after_at: Option<&str>,
) -> Result<TaskRecord, String> {
    db::update_task_status(
        &state.pool,
        id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        None,
    )
    .await?;
    db::update_task_retry_after(&state.pool, id, retry_after_at).await?;
    db::update_segments_status_for_task(
        &state.pool,
        id,
        crate::models::SegmentStatus::Pending,
        None,
    )
    .await?;
    let task = require_task(&state.pool, id).await?;
    emit_task_progress_snapshot(app, &task);
    emit_task_updated_record(app, &state.pool, &task).await;
    emit_queue_changed(app);
    if retry_after_at.is_none() {
        schedule_queued_tasks(app.clone(), state).await;
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
    if let Some(control) = state.downloads.lock().await.remove(&task.id) {
        control.cancel.store(true, Ordering::SeqCst);
        control.handle.abort();
    }
    if let Some(temp_path) = task.temp_path.as_deref() {
        remove_task_path(temp_path)?;
    }

    let engine = state.engine_registry.engine_for_uri(&task.url)?;
    let request_headers =
        resolve_task_request_headers(&state.pool, state.request_headers.clone(), &task.id).await?;
    let probe = engine
        .probe(ProbeRequest {
            uri: task.url.clone(),
            source: None,
            request_headers,
            pool: Some(state.pool.clone()),
            task_id: Some(task.id.clone()),
            username: None,
            password: None,
            private_key_data: None,
            private_key_passphrase: None,
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
    emit_queue_changed(app);
    schedule_queued_tasks(app.clone(), state).await;
    require_task(&state.pool, &task.id).await
}

fn restart_required_error_code(code: &str) -> bool {
    matches!(
        code,
        "remote_changed"
            | "resume_unavailable"
            | "temp_file_missing"
            | "temp_file_smaller_than_progress"
    )
}

fn task_error_code(task: &TaskRecord) -> Option<String> {
    if let Some(code) = task.error_code.clone() {
        return Some(code);
    }
    let error = task.error_message.as_deref()?;
    if let Ok(payload) = serde_json::from_str::<AppErrorPayload>(error) {
        return Some(payload.code);
    }
    if error.contains("Remote file changed") {
        return Some("remote_changed".to_string());
    }
    if error.contains("Server no longer supports resume") || error.contains("Resume unavailable") {
        return Some("resume_unavailable".to_string());
    }
    if error.contains("Temporary file is missing") {
        return Some("temp_file_missing".to_string());
    }
    if error.contains("Temporary file is smaller") {
        return Some("temp_file_smaller_than_progress".to_string());
    }
    None
}

async fn prepare_task_for_download(
    pool: &sqlx::SqlitePool,
    engine_registry: &EngineRegistry,
    task: TaskRecord,
    request_headers: &[(String, String)],
) -> Result<TaskRecord, String> {
    if task.status == TaskStatus::NeedsAttention {
        return Err("Remote file changed. Restart download to avoid corruption.".to_string());
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
    let temp_exists = temp_path.exists();
    let temp_size = std::fs::metadata(&temp_path)
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    if let Some(message) = segment_resume_error(
        &segments,
        task.downloaded_bytes,
        temp_exists,
        temp_size,
        task.total_size,
        task.supports_resume,
    ) {
        fail_task_and_segments(pool, &task.id, message).await?;
        db::insert_task_event(pool, &task.id, "resume_blocked", Some(message)).await?;
        return Err(message.to_string());
    }

    if temp_size > 0 {
        let uri = task.final_url.as_deref().unwrap_or(&task.url).to_string();
        let engine = engine_registry.engine_for_uri(&uri)?;
        let probe = engine
            .probe(ProbeRequest {
                uri,
                source: None,
                request_headers: request_headers.to_vec(),
                pool: Some(pool.clone()),
                task_id: Some(task.id.clone()),
                username: None,
                password: None,
                private_key_data: None,
                private_key_passphrase: None,
            })
            .await?;
        if let Some(message) = resume_mismatch_message(&task, &probe) {
            db::update_task_status(
                pool,
                &task.id,
                TaskStatus::NeedsAttention,
                0,
                0,
                Some(&message),
                Some(&message),
            )
            .await?;
            db::update_segments_status_for_task(
                pool,
                &task.id,
                crate::models::SegmentStatus::Failed,
                Some(&message),
            )
            .await?;
            db::insert_task_event(pool, &task.id, "resume_blocked", Some(&message)).await?;
            return Err(message);
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

fn is_torrent_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".torrent"))
}

pub(crate) fn is_metalink_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.ends_with(".meta4") || name.ends_with(".metalink")
            })
}

pub(crate) fn is_dash_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".mpd"))
}

async fn fail_task_and_segments(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    message: &str,
) -> Result<(), String> {
    db::update_task_status(
        pool,
        task_id,
        TaskStatus::Failed,
        0,
        0,
        Some(message),
        Some(message),
    )
    .await?;
    db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(message),
    )
    .await
}

async fn mark_download_failed(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    task_id: &str,
    error: String,
) {
    tracing::error!(task_id = %task_id, error = %error, "download failed");
    let payload = serde_json::from_str::<AppErrorPayload>(&error).ok();
    let status = match payload.as_ref().map(|payload| payload.code.as_str()) {
        Some("final_path_conflict" | "auth_headers_expired" | "auth_headers_unavailable") => {
            TaskStatus::NeedsAttention
        }
        _ => TaskStatus::Failed,
    };
    if let Err(db_error) =
        db::update_task_status(pool, task_id, status, 0, 0, Some(&error), Some(&error)).await
    {
        tracing::warn!(
            task_id = %task_id,
            error = %db_error,
            "failed to persist task failure status"
        );
    }
    if let Err(db_error) = db::insert_task_event(pool, task_id, "failed", Some(&error)).await {
        tracing::warn!(
            task_id = %task_id,
            error = %db_error,
            "failed to persist task failure event"
        );
    }
    if let Err(db_error) = db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(&error),
    )
    .await
    {
        tracing::warn!(
            task_id = %task_id,
            error = %db_error,
            "failed to persist segment failure status"
        );
    }
    if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
        emit_task_progress_snapshot(app, &task);
        emit_task_updated_record(app, pool, &task).await;
    }
    emit_queue_changed(app);
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
    Ok(records
        .into_iter()
        .map(|record| {
            let files = files_by_task_id.remove(&record.id).unwrap_or_default();
            let checksums = checksums_by_task_id.remove(&record.id).unwrap_or_default();
            task_from_record_and_files(record, files, checksums)
        })
        .collect())
}

async fn task_from_record_with_files(
    pool: &sqlx::SqlitePool,
    record: TaskRecord,
) -> Result<Task, String> {
    let files = db::list_task_file_records(pool, &record.id).await?;
    let checksums = db::list_task_checksum_records(pool, &record.id).await?;
    Ok(task_from_record_and_files(record, files, checksums))
}

fn task_from_record_and_files(
    record: TaskRecord,
    files: Vec<TaskFileRecord>,
    checksums: Vec<TaskChecksumRecord>,
) -> Task {
    let mut task = Task::from(record);
    task.files = files.into_iter().map(Into::into).collect();
    task.checksums = checksums.into_iter().map(Into::into).collect();
    task
}

fn emit_task_progress_snapshot(app: &AppHandle, task: &TaskRecord) {
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
