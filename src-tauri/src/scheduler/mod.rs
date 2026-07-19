use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::{
    commands::settings::default_download_dir,
    commands::tasks::{
        emit_task_progress_snapshot, prepare_task_for_download, resolve_task_request_headers,
    },
    db,
    download::{DownloadContext, EngineRegistry, GlobalSpeedLimiter},
    events::{
        emit_completion_action_requested, emit_queue_changed, emit_queue_changed_with_ids,
        emit_task_updated_record,
    },
    logging::sanitize_url,
    models::{
        CompletionAction, CompletionActionRequestedPayload, HashVerificationStatus, TaskRecord,
        TaskStatus,
    },
    platform,
    state_machine::TransitionError,
    DownloadControl, TaskRequestHeaders,
};

/// Download scheduler: encapsulates active download map, request header cache, global speed limiter, engine registry, and other shared state.
///
/// All scheduling methods take `self: Arc<Self>` so an `Arc` clone can continue scheduling in a spawned task.
/// `app` and `pool` are not stored in the struct; they are passed in on each call (to avoid the Scheduler owning the DB connection pool lifetime).
pub struct Scheduler {
    /// Mutex ensuring scheduling runs serially (formerly AppState.scheduler: Arc<Mutex<()>>)
    lock: Mutex<()>,
    /// Active download map (shares the same Arc as AppState.downloads)
    downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
    /// Request header cache (shares the same Arc as AppState.request_headers)
    request_headers: TaskRequestHeaders,
    /// Global speed limiter (shares the same Arc as AppState.speed_limiter)
    speed_limiter: Arc<GlobalSpeedLimiter>,
    /// Engine registry (shares the same Arc as AppState.engine_registry)
    engine_registry: Arc<EngineRegistry>,
    /// R-2: Per-task runtime lock (shares the same Arc as AppState.task_runtime_locks)
    task_runtime_locks: Arc<crate::TaskRuntimeLocks>,
}

impl Scheduler {
    pub fn new(
        downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
        request_headers: TaskRequestHeaders,
        speed_limiter: Arc<GlobalSpeedLimiter>,
        engine_registry: Arc<EngineRegistry>,
        task_runtime_locks: Arc<crate::TaskRuntimeLocks>,
    ) -> Self {
        Self {
            lock: Mutex::new(()),
            downloads,
            request_headers,
            speed_limiter,
            engine_registry,
            task_runtime_locks,
        }
    }

    /// Scheduling entry point (formerly schedule_queued_tasks).
    pub async fn dispatch(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        self.dispatch_inner(app, pool).await;
    }

    /// Delayed scheduling entry point (formerly the schedule branch of spawn_schedule_queued_tasks_after).
    pub async fn schedule_retry_after_wakeup(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        let Some(next) = (match db::next_retry_after_at(&pool).await {
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
            self.dispatch(app, pool).await;
        } else {
            self.spawn_dispatch_after(app, pool, delay);
        }
    }

    /// Core scheduling logic (formerly schedule_queued_tasks_inner).
    async fn dispatch_inner(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            if state
                .quit_requested
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                tracing::debug!("scheduler dispatch exiting (shutdown requested)");
                return;
            }
        }

        let _guard = self.lock.lock().await;

        // Read settings and queued list ONCE outside the loop.
        // Previously these were re-fetched every iteration (~240 DB round trips per burst).
        let default_dir = default_download_dir(&app).unwrap_or_default();
        let settings = match db::get_settings(&pool, default_dir).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::error!(error = %error, "failed to load settings for scheduler");
                return;
            }
        };
        // Fetch enough queued tasks to potentially fill every host slot (active × per-host),
        // but cap at one page to avoid loading the entire queue each dispatch tick.
        // The lower clamp ensures we always fetch at least max_active_tasks candidates.
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

        let mut active_count = self.downloads.lock().await.len();
        let available = Self::compute_available_slots(settings.max_active_tasks, active_count);
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

        // E-6: Build the host → used slots count map in one pass, avoiding per-task lock +
        // full downloads traversal inside the loop (O(N×M) + N Mutex locks).
        let mut host_slot_map: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        {
            let downloads = self.downloads.lock().await;
            for control in downloads.values() {
                *host_slot_map.entry(control.source_key.clone()).or_insert(0) +=
                    control.connection_slots;
            }
        }

        for task in queued {
            if active_count >= settings.max_active_tasks as usize {
                break;
            }
            let time_window_active = db::local_time_window_active(
                &settings.schedule_download_window_start,
                &settings.schedule_download_window_end,
            );
            if Self::should_skip_for_schedule_window(
                settings.schedule_download_window_enabled,
                task.obey_schedule,
                time_window_active,
            ) {
                tracing::debug!(
                    task_id = %task.id,
                    "scheduler deferred task outside configured download window"
                );
                continue;
            }
            let host_used = host_slot_map.get(&task.source_key).copied().unwrap_or(0);
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
            let planned = db::planned_segment_count_with_plan(
                &task,
                db::parse_multi_connection_threshold_bytes(
                    &settings.multi_connection_threshold_bytes,
                ),
                settings.segment_count,
            );
            let planned_slots = Self::compute_planned_slots(planned, host_limit, host_used);
            let task_id = task.id.clone();
            let source_key = task.source_key.clone();
            match self
                .clone()
                .start_task(app.clone(), pool.clone(), task, planned_slots)
                .await
            {
                Ok(()) => {
                    // Task was inserted into the downloads map — count it.
                    active_count += 1;
                    // Update the local count map synchronously so subsequent tasks see the latest slot usage.
                    *host_slot_map.entry(source_key).or_insert(0) += planned_slots;
                }
                Err(error) => {
                    // start_task failed before spawning a worker; it has
                    // already removed its pending DownloadControl entry on
                    // all error paths (Conflict at the transition_task match,
                    // non-Conflict at the same match). Do NOT increment
                    // active_count — the task is not consuming a slot.
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

    /// Start a single task download (formerly start_task_download).
    async fn start_task(
        self: Arc<Self>,
        app: AppHandle,
        pool: SqlitePool,
        task: TaskRecord,
        connection_limit: usize,
    ) -> Result<(), String> {
        // R-2.3: Per-task runtime lock serializes start vs pause/cancel/delete/retry.
        // Worker (download engine) does NOT hold this lock — it relies on R-1's
        // conditional DB update to avoid overwriting user-initiated state changes.
        let _runtime_guard = self.task_runtime_locks.lock(&task.id).await;

        let task_request_headers =
            resolve_task_request_headers(&pool, self.request_headers.clone(), &task.id).await?;
        let global_proxy_config = self.engine_registry.proxy_config().await;
        let task_proxy_config =
            db::resolve_task_proxy_config(&pool, &task.id, &task.protocol, &global_proxy_config)
                .await?;
        let task = prepare_task_for_download(
            &app,
            &pool,
            &self.engine_registry,
            task,
            &task_request_headers,
        )
        .await?;
        if self.downloads.lock().await.contains_key(&task.id) {
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

        // R-2.3: Create cancel_token + finish BEFORE transition_task so a pending
        // DownloadControl can be inserted with the same token instance the worker
        // will listen on. This closes the race window where pause/cancel sees
        // status=Downloading in DB but no control entry to cancel the worker.
        let finish = Arc::new(AtomicBool::new(false));
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let source_key = task.source_key.clone();
        {
            let mut downloads = self.downloads.lock().await;
            downloads.insert(
                task.id.clone(),
                DownloadControl {
                    cancel_token: cancel_token.clone(),
                    finish: finish.clone(),
                    handle: None, // pending — updated to Some(handle) after spawn
                    source_key: source_key.clone(),
                    connection_slots: connection_limit.max(1),
                },
            );
        }

        match crate::state_machine::transition_task(
            &app,
            &pool,
            &task.id,
            TaskStatus::Downloading,
            0,
            connection_count,
            Some("Downloading"),
            Some("started"),
        )
        .await
        {
            Ok(_) => {}
            Err(TransitionError::Conflict {
                task_id,
                current,
                attempted,
            }) => {
                // R-2.3: Remove the pending control — no worker will be spawned.
                self.downloads.lock().await.remove(&task.id);
                tracing::warn!(
                    task_id = %task_id,
                    current = ?current,
                    attempted = ?attempted,
                    "start_task: task state changed concurrently, skipping"
                );
                return Ok(());
            }
            Err(error) => {
                // R-2.3: Non-Conflict transition failure (e.g. Database, Illegal,
                // NotFound). No worker will be spawned, so remove the pending
                // control to avoid leaking a slot that would silently starve the
                // scheduler (active_count and per-host slots are derived from
                // downloads.len()).
                self.downloads.lock().await.remove(&task.id);
                return Err(error.into());
            }
        }

        let downloads_map = self.downloads.clone();
        let task_id = task.id.clone();
        let map_task_id = task.id.clone();
        let task_app = app.clone();
        let task_cancel_token = cancel_token.clone();
        let task_finish = finish.clone();
        let task_pool = pool.clone();
        let state_speed_limiter = self.speed_limiter.clone();
        let task_engine_registry = self.engine_registry.clone();
        let scheduler = self.clone();

        let handle = tokio::spawn(async move {
            let engine = match task_engine_registry.engine_for_uri(&task.url) {
                Ok(engine) => engine,
                Err(error) => {
                    mark_download_failed(&task_app, &task_pool, &task_id, error).await;
                    let _ = downloads_map.lock().await.remove(&task_id);
                    scheduler
                        .clone()
                        .spawn_dispatch(task_app.clone(), task_pool.clone());
                    return;
                }
            };

            let task_limit_bps = db::parse_speed_limit_bps(task.task_speed_limit_bps.as_deref());
            let scheduled_limit_bps = db::get_settings(
                &task_pool,
                default_download_dir(&task_app).unwrap_or_default(),
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
            // Per-task limit and scheduled-window limit both apply; the stricter (minimum) wins.
            // If the scheduled window is inactive, only the per-task limit applies.
            let effective_task_limit = min_optional_limit(task_limit_bps, scheduled_limit_bps);
            let task_speed_limiter =
                GlobalSpeedLimiter::with_parent(state_speed_limiter.clone(), effective_task_limit);
            // ARC-03: run the engine directly inside this supervisor task. A nested
            // tokio::spawn previously detached the engine when the outer handle was
            // aborted, leaving workers/ffmpeg running after pause/delete/shutdown.
            // Panics still surface through this outer JoinHandle (panic=unwind).
            let result = engine
                .download(DownloadContext {
                    app: Some(task_app.clone()),
                    pool: task_pool.clone(),
                    task,
                    cancel_token: task_cancel_token.clone(),
                    finish: task_finish.clone(),
                    speed_limiter: task_speed_limiter,
                    connection_limit,
                    request_headers: task_request_headers.clone(),
                    proxy_config: task_proxy_config,
                })
                .await
                .map_err(String::from);
            let canceled = task_cancel_token.is_cancelled();
            let _ = downloads_map.lock().await.remove(&task_id);
            let _ = scheduler.request_headers.lock().await.remove(&task_id);

            if let Err(error) = result {
                if !canceled {
                    mark_download_failed(&task_app, &task_pool, &task_id, error).await;
                }
            } else if !canceled {
                match crate::commands::tasks::verify_task_hash_with_pool(&task_pool, &task_id).await
                {
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
                scheduler
                    .maybe_emit_completion_action(&task_app, &task_pool)
                    .await;
            }

            // A-4: Evict the runtime lock entry now that the worker has finished
            // and the downloads_map/request_headers entries are removed. If a user
            // action (pause/cancel/delete/retry) is concurrently holding the lock,
            // strong_count > 1 and evict is a safe no-op. This prevents completed/
            // failed/paused task entries from accumulating indefinitely in the
            // registry (only delete_* previously evicted).
            scheduler.task_runtime_locks.evict(&task_id).await;

            scheduler.clone().spawn_dispatch(task_app, task_pool);
        });

        // R-2.3: Update the pending control's handle now that the worker is spawned.
        // If the control was removed (e.g. by a concurrent cancel), the worker is
        // already self-cleaning via downloads_map.lock().await.remove(&task_id).
        {
            let mut downloads = self.downloads.lock().await;
            if let Some(control) = downloads.get_mut(&map_task_id) {
                control.handle = Some(handle);
            } else {
                tracing::warn!(
                    task_id = %map_task_id,
                    "pending control removed before handle update — task was likely cancelled"
                );
            }
        }

        emit_queue_changed_with_ids(&app, Some(vec![map_task_id.clone()]));
        Ok(())
    }

    /// Count used connection slots for the given host (formerly host_connection_slots).
    pub async fn host_used(&self, source_key: &str) -> usize {
        self.downloads
            .lock()
            .await
            .values()
            .filter(|control| control.source_key == source_key)
            .map(|control| control.connection_slots)
            .sum()
    }

    /// Compute available slots (pure function, no side effects).
    pub fn compute_available_slots(max_active_tasks: i32, active_count: usize) -> i32 {
        max_active_tasks.saturating_sub(active_count as i32).max(0)
    }

    /// Determine whether a task should be skipped by the scheduling window (pure function).
    pub fn should_skip_for_schedule_window(
        schedule_window_enabled: bool,
        obey_schedule: bool,
        time_window_active: bool,
    ) -> bool {
        schedule_window_enabled && obey_schedule && !time_window_active
    }

    /// Compute planned_slots, considering host_limit and host_used (pure function).
    pub fn compute_planned_slots(planned: usize, host_limit: usize, host_used: usize) -> usize {
        // Floor at 1: even single-stream downloads need one slot, and we allow a new task
        // to exceed host_limit by 1 rather than starving it. host_limit is a soft target,
        // not a hard cap.
        planned.min(host_limit.saturating_sub(host_used)).max(1)
    }

    /// Asynchronously trigger scheduling (formerly spawn_schedule_queued_tasks).
    fn spawn_dispatch(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        tokio::spawn(async move {
            self.dispatch_inner(app, pool).await;
        });
    }

    /// Asynchronously trigger scheduling with a delay (formerly spawn_schedule_queued_tasks_after).
    pub fn spawn_dispatch_after(
        self: Arc<Self>,
        app: AppHandle,
        pool: SqlitePool,
        delay: std::time::Duration,
    ) {
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            self.dispatch_inner(app, pool).await;
        });
    }

    /// Emit the completion action when the queue is empty and no tasks are active (formerly maybe_emit_completion_action).
    async fn maybe_emit_completion_action(&self, app: &AppHandle, pool: &SqlitePool) {
        if !self.downloads.lock().await.is_empty() {
            return;
        }
        if db::list_queued_task_records(pool, 1)
            .await
            .map(|tasks| !tasks.is_empty())
            .unwrap_or(true)
        {
            return;
        }
        let Ok(settings) =
            db::get_settings(pool, default_download_dir(app).unwrap_or_default()).await
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
}

/// Mark a task download as failed and update DB/events (formerly commands::tasks::mark_download_failed).
/// Internal to the scheduler module.
async fn mark_download_failed(app: &AppHandle, pool: &SqlitePool, task_id: &str, error: String) {
    tracing::error!(task_id = %task_id, error = %error, "download failed");
    let payload = serde_json::from_str::<crate::models::AppErrorPayload>(&error).ok();
    // These errors are recoverable with user action (resolve a file conflict, re-auth
    // the browser) → NeedsAttention, not Failed. Keep this list in sync with
    // AppErrorPayload code constants.
    let status = match payload.as_ref().map(|payload| payload.code.as_str()) {
        Some("final_path_conflict" | "auth_headers_expired" | "auth_headers_unavailable") => {
            TaskStatus::NeedsAttention
        }
        _ => TaskStatus::Failed,
    };
    // R-2.4: Conditional UPDATE — only mark failed if still Downloading/Retrying.
    // If the user paused/canceled/deleted while the worker was erroring out,
    // skip the failure write to avoid overwriting their action.
    let updated =
        match db::mark_task_failed_if_active(pool, task_id, status, Some(&error), Some(&error))
            .await
        {
            Ok(updated) => updated,
            Err(db_error) => {
                tracing::warn!(
                    task_id = %task_id,
                    error = %db_error,
                    "failed to persist task failure status"
                );
                return;
            }
        };
    if !updated {
        tracing::warn!(
            task_id = %task_id,
            "mark_download_failed: task state changed concurrently, skipping emit"
        );
        return;
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
    emit_queue_changed_with_ids(app, Some(vec![task_id.to_string()]));
}

fn min_optional_limit(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}
