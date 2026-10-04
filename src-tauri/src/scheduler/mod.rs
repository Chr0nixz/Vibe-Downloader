use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, Notify};

mod queue;
pub use queue::DispatchQueue;

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
        AppSettings, CompletionAction, CompletionActionRequestedPayload, HashVerificationStatus,
        TaskRecord, TaskStatus,
    },
    platform,
    state_machine::TransitionError,
    DownloadControl, TaskRequestHeaders,
};

/// ARC-44: the three Ok paths of `start_task` have different slot-accounting
/// implications, so they must not be collapsed into a bare `Ok(())`.
/// `AlreadyActive` rows are already inside `downloads` (counted by the map
/// seeding in dispatch_inner), and `ConflictSkipped` never spawned a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartTaskOutcome {
    Started,
    AlreadyActive,
    ConflictSkipped,
}

/// ARC-44: honest slot accounting per start outcome. Only `Started` consumes a
/// new slot; counting the other outcomes inflated active/host usage for the
/// rest of the dispatch tick and conservatively deferred following tasks.
fn account_start_outcome(
    active_count: &mut usize,
    host_slot_map: &mut HashMap<String, usize>,
    outcome: StartTaskOutcome,
    source_key: &str,
    planned_slots: usize,
) {
    if outcome != StartTaskOutcome::Started {
        return;
    }
    *active_count += 1;
    *host_slot_map.entry(source_key.to_string()).or_insert(0) += planned_slots;
}

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
    speed_policy_lock: Mutex<()>,
    speed_policy_changed: Notify,
    /// Wakes the long-lived retry watcher whenever a task enters or leaves
    /// the delayed queue, so a stale deadline is never held after a manual
    /// pause/retry or an automatic failure.
    retry_schedule_changed: Notify,
    /// ARC-58: the current completion-action round (see [`CompletionRound`]).
    /// A std mutex: every critical section is a few field updates, never
    /// held across an await.
    completion_round: std::sync::Mutex<CompletionRound>,
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
            speed_policy_lock: Mutex::new(()),
            speed_policy_changed: Notify::new(),
            retry_schedule_changed: Notify::new(),
            completion_round: std::sync::Mutex::new(CompletionRound::default()),
        }
    }

    pub fn notify_speed_policy_changed(&self) {
        self.speed_policy_changed.notify_one();
    }

    pub async fn wait_for_speed_policy_change(&self) {
        self.speed_policy_changed.notified().await;
    }

    pub fn notify_retry_schedule_changed(&self) {
        self.retry_schedule_changed.notify_one();
    }

    pub async fn refresh_speed_limit_policies(
        &self,
        pool: &SqlitePool,
        default_dir: String,
    ) -> Result<(), String> {
        let _policy_guard = self.speed_policy_lock.lock().await;
        let settings = db::get_settings(pool, default_dir).await?;
        let scheduled_limit = active_scheduled_speed_limit(&settings);
        let controls = self
            .downloads
            .lock()
            .await
            .iter()
            .map(|(task_id, control)| (task_id.clone(), control.speed_limiter.clone()))
            .collect::<Vec<_>>();

        for (task_id, limiter) in controls {
            let Some(task) = db::get_task_record(pool, &task_id).await? else {
                continue;
            };
            let task_limit = db::parse_speed_limit_bps(task.task_speed_limit_bps.as_deref());
            limiter
                .set_limit(min_optional_limit(task_limit, scheduled_limit))
                .await;
        }
        Ok(())
    }

    /// Scheduling entry point (formerly schedule_queued_tasks).
    pub async fn dispatch(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        self.dispatch_inner(app, pool).await;
    }

    /// Fires a dispatch tick without awaiting it.
    ///
    /// ARC-32: user command paths (pause/cancel/delete/restart/retry) run while
    /// holding the per-task runtime lock. Awaiting [`Self::dispatch`] under that
    /// lock deadlocks — `dispatch` → `start_task` re-acquires task runtime locks
    /// (a Restarted task is already Queued, so dispatch picks that same task and
    /// blocks on the still-held non-reentrant lock), or forms an ABBA cycle with
    /// a concurrent dispatch that holds the scheduler lock. Spawning lets the
    /// caller unwind and release its lock first; slot bookkeeping is unchanged
    /// because the response to the user never depended on dispatch completing.
    pub fn dispatch_detached(self: &Arc<Self>, app: AppHandle, pool: SqlitePool) {
        let scheduler = self.clone();
        tauri::async_runtime::spawn(async move {
            scheduler.dispatch(app, pool).await;
        });
    }

    /// Delayed scheduling entry point (formerly the schedule branch of spawn_schedule_queued_tasks_after).
    pub async fn schedule_retry_after_wakeup(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        loop {
            if app.try_state::<crate::AppState>().is_some_and(|state| {
                state
                    .quit_requested
                    .load(std::sync::atomic::Ordering::SeqCst)
            }) {
                return;
            }

            let changed = self.retry_schedule_changed.notified();
            let next = match db::next_retry_after_at(&pool).await {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(error = %error, "failed to inspect retry-after queue");
                    None
                }
            };
            let delay = next.as_deref().and_then(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .ok()
                    .map(|when| {
                        when.with_timezone(&chrono::Utc)
                            .signed_duration_since(chrono::Utc::now())
                            .to_std()
                            .unwrap_or_default()
                    })
            });

            match delay {
                Some(delay) => {
                    tokio::select! {
                        _ = changed => {}
                        _ = tokio::time::sleep(delay) => {
                            self.clone().dispatch(app.clone(), pool.clone()).await;
                        }
                    }
                }
                None => {
                    // A short idle poll covers rows inserted by an older
                    // command path that predates the Notify call, while the
                    // notification keeps normal changes immediate.
                    tokio::select! {
                        _ = changed => {}
                        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                }
            }
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

        // Keep one settings snapshot per pass; candidate pages advance as slots fill.
        let default_dir = default_download_dir(&app).unwrap_or_default();
        let settings = match db::get_settings(&pool, default_dir).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::error!(error = %error, "failed to load settings for scheduler");
                return;
            }
        };
        // ARC-61: bound each page, but continue past blocked candidates until slots fill.
        let queued_limit = i64::from(settings.max_active_tasks)
            .saturating_mul(i64::from(settings.max_connections_per_host).max(1))
            .clamp(
                i64::from(settings.max_active_tasks).max(1),
                db::DEFAULT_TASK_PAGE_SIZE,
            );
        let mut queued = DispatchQueue::new(queued_limit);

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

        tracing::debug!(available, "scheduler dispatching queued tasks");

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

        while active_count < settings.max_active_tasks as usize {
            if app.try_state::<crate::AppState>().is_some_and(|state| {
                state
                    .quit_requested
                    .load(std::sync::atomic::Ordering::SeqCst)
            }) {
                break;
            }
            let time_window_active = db::local_time_window_active(
                &settings.schedule_download_window_start,
                &settings.schedule_download_window_end,
            );
            let task = match queued
                .next(
                    &pool,
                    settings.schedule_download_window_enabled && !time_window_active,
                    host_limit,
                    &host_slot_map,
                )
                .await
            {
                Ok(Some(task)) => task,
                Ok(None) => break,
                Err(error) => {
                    tracing::error!(error = %error, "failed to load queued tasks");
                    break;
                }
            };
            let host_used = host_slot_map.get(&task.source_key).copied().unwrap_or(0);
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
            // ARC-05: start_task only reserves the slot + transitions under this
            // global lock, then spawns a worker that runs resume probe off-lock.
            // Awaiting the short reservation path keeps host_slot_map accurate
            // without serializing remote probes across hosts.
            match self
                .clone()
                .start_task(app.clone(), pool.clone(), task, planned_slots)
                .await
            {
                Ok(outcome) => {
                    // ARC-44: only a genuinely started worker consumes a slot.
                    // AlreadyActive was already counted by the downloads-map
                    // seeding above, and ConflictSkipped never spawned one.
                    account_start_outcome(
                        &mut active_count,
                        &mut host_slot_map,
                        outcome,
                        &source_key,
                        planned_slots,
                    );
                }
                Err(error) => {
                    // start_task failed before spawning a worker; it has
                    // already removed its pending DownloadControl entry on
                    // all error paths (Conflict at the transition_task match,
                    // non-Conflict at the same match). Do NOT increment
                    // active_count — the task is not consuming a slot.
                    handle_start_failure(Some(&app), &pool, &task_id, error).await;
                }
            }
        }

        emit_queue_changed(&app);
    }

    /// Start a single task download (formerly start_task_download).
    ///
    /// ARC-05: Under the caller's scheduler lock this only reserves a pending
    /// `DownloadControl`, transitions Queued→Downloading, and spawns a worker.
    /// Resume probe (`prepare_task_for_download`) runs inside that worker so a
    /// slow remote host cannot block dispatch of other hosts.
    async fn start_task(
        self: Arc<Self>,
        app: AppHandle,
        pool: SqlitePool,
        task: TaskRecord,
        connection_limit: usize,
    ) -> Result<StartTaskOutcome, String> {
        // R-2.3: Per-task runtime lock serializes start vs pause/cancel/delete/retry.
        // Worker (download engine) does NOT hold this lock — it relies on R-1's
        // conditional DB update to avoid overwriting user-initiated state changes.
        let _runtime_guard = self.task_runtime_locks.lock(&task.id).await;
        let (lifecycle_gate, lifecycle, quit_requested, active_supervisors) = app
            .try_state::<crate::AppState>()
            .map(|state| {
                (
                    Some(state.lifecycle_gate.clone()),
                    Some(state.lifecycle.clone()),
                    Some(state.quit_requested.clone()),
                    Some(state.active_supervisors.clone()),
                )
            })
            .unwrap_or((None, None, None, None));
        let _lifecycle_guard = match lifecycle_gate {
            Some(gate) => Some(gate.lock_owned().await),
            None => None,
        };
        if lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.phase() != crate::AppLifecyclePhase::Running)
            || quit_requested
                .as_ref()
                .is_some_and(|requested| requested.load(std::sync::atomic::Ordering::SeqCst))
        {
            return Ok(StartTaskOutcome::ConflictSkipped);
        }

        // Header/proxy resolution is local DB work and stays on the reservation
        // path so the worker already has the values it needs for probe+download.
        let task_request_headers =
            resolve_task_request_headers(&pool, self.request_headers.clone(), &task.id).await?;
        let global_proxy_config = self.engine_registry.proxy_config().await;
        let task_proxy_config =
            db::resolve_task_proxy_config(&pool, &task.id, &task.protocol, &global_proxy_config)
                .await?;
        let task_network_policy = db::task_network_policy(&pool, &task.id).await?;
        if self.downloads.lock().await.contains_key(&task.id) {
            tracing::debug!(task_id = %task.id, "download already active, skipping start");
            return Ok(StartTaskOutcome::AlreadyActive);
        }

        let _speed_policy_guard = self.speed_policy_lock.lock().await;
        let settings =
            db::get_settings(&pool, default_download_dir(&app).unwrap_or_default()).await?;
        let task_limit_bps = db::parse_speed_limit_bps(task.task_speed_limit_bps.as_deref());
        let task_speed_limiter = GlobalSpeedLimiter::with_parent(
            self.speed_limiter.clone(),
            min_optional_limit(task_limit_bps, active_scheduled_speed_limit(&settings)),
        );

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
        // PERF-15: one Notify instance shared by the registered control (the
        // finish command notifies it) and the worker context (HLS waits on it).
        let finish_notify = Arc::new(tokio::sync::Notify::new());
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let source_key = task.source_key.clone();
        {
            let mut downloads = self.downloads.lock().await;
            downloads.insert(
                task.id.clone(),
                DownloadControl {
                    cancel_token: cancel_token.clone(),
                    finish: finish.clone(),
                    finish_notify: finish_notify.clone(),
                    speed_limiter: task_speed_limiter.clone(),
                    handle: None, // pending — updated to Some(handle) after spawn
                    source_key: source_key.clone(),
                    connection_slots: connection_limit.max(1),
                },
            );
        }
        drop(_speed_policy_guard);

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
            Ok(_) => {
                // ARC-58: the queue has real work again; this opens a new
                // completion round after a decided drain, or joins the
                // round that is still running.
                lock_round(&self.completion_round).record_start();
            }
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
                return Ok(StartTaskOutcome::ConflictSkipped);
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
        let task_finish_notify = finish_notify.clone();
        let task_pool = pool.clone();
        let task_engine_registry = self.engine_registry.clone();
        let scheduler = self.clone();
        let supervisor_guard = active_supervisors.map(crate::ActiveSupervisorGuard::new);

        let handle = tokio::spawn(async move {
            let _supervisor_guard = supervisor_guard;
            // ARC-05: network resume validation runs here, after the scheduler
            // global lock has been released by dispatch_inner's next iteration.
            let task = match prepare_task_for_download(
                &task_app,
                &task_pool,
                &task_engine_registry,
                task,
                &task_request_headers,
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    // Atomic slot release: drop pending control before any
                    // failure transition so host/active counts cannot leak.
                    let canceled = task_cancel_token.is_cancelled();
                    converge_download_outcome(
                        &downloads_map,
                        &scheduler.request_headers,
                        &scheduler.task_runtime_locks,
                        Some(&task_app),
                        &task_pool,
                        &task_id,
                        canceled,
                        Err(error),
                    )
                    .await;
                    scheduler.notify_retry_schedule_changed();
                    scheduler.task_runtime_locks.evict(&task_id).await;
                    scheduler
                        .clone()
                        .spawn_dispatch(task_app.clone(), task_pool.clone());
                    return;
                }
            };

            let engine = match task_engine_registry.engine_for_uri(&task.url) {
                Ok(engine) => engine,
                Err(error) => {
                    converge_download_outcome(
                        &downloads_map,
                        &scheduler.request_headers,
                        &scheduler.task_runtime_locks,
                        Some(&task_app),
                        &task_pool,
                        &task_id,
                        task_cancel_token.is_cancelled(),
                        Err(error),
                    )
                    .await;
                    scheduler.notify_retry_schedule_changed();
                    scheduler.task_runtime_locks.evict(&task_id).await;
                    scheduler
                        .clone()
                        .spawn_dispatch(task_app.clone(), task_pool.clone());
                    return;
                }
            };

            // ARC-03: run the engine directly inside this supervisor task. A nested
            // tokio::spawn previously detached the engine when the outer handle was
            // aborted, leaving workers/ffmpeg running after pause/delete/shutdown.
            // Panics still surface through this outer JoinHandle (panic=unwind).
            let download = engine.download(DownloadContext {
                app: Some(task_app.clone()),
                pool: task_pool.clone(),
                task,
                cancel_token: task_cancel_token.clone(),
                finish: task_finish.clone(),
                finish_notify: task_finish_notify.clone(),
                speed_limiter: task_speed_limiter,
                connection_limit,
                request_headers: task_request_headers.clone(),
                proxy_config: task_proxy_config,
                network_policy: task_network_policy,
            });
            // ARC-40: an engine panic must not skip the convergence body below —
            // that would leak the downloads_map slot and the runtime lock, and
            // leave the task stuck in Downloading. Catch the unwind, release the
            // runtime state, and transition to Failed like any other error.
            let result =
                match futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(download))
                    .await
                {
                    Ok(result) => result.map_err(String::from),
                    Err(panic_payload) => Err(describe_engine_panic(panic_payload)),
                };
            let canceled = task_cancel_token.is_cancelled();
            let failed = result.is_err();
            // ARC-58: record the failure BEFORE convergence releases the
            // downloads-map slot. Another supervisor can only observe the
            // drained queue after that release, so it is guaranteed to see
            // this failure when it decides the round.
            if failed && !canceled {
                lock_round(&scheduler.completion_round).record_failure(&task_id);
            }
            converge_download_outcome(
                &downloads_map,
                &scheduler.request_headers,
                &scheduler.task_runtime_locks,
                Some(&task_app),
                &task_pool,
                &task_id,
                canceled,
                result,
            )
            .await;
            scheduler.notify_retry_schedule_changed();

            if !failed && !canceled {
                match crate::commands::tasks::verify_task_hash_with_pool(&task_pool, &task_id).await
                {
                    Ok(state) if state.status != HashVerificationStatus::NotRequested => {
                        // ARC-58: a checksum mismatch is a failed download for
                        // the "only when every download succeeded" semantics.
                        if state.status == HashVerificationStatus::Failed {
                            lock_round(&scheduler.completion_round).record_failure(&task_id);
                        }
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
            } else if !canceled {
                // ARC-58: a failing last task used to skip the completion
                // action entirely. It now decides the round like a success;
                // whether the action runs depends on the round's failures and
                // `completion_include_failures`. A user pause/cancel is not a
                // drain the user asked to act on, so it never decides.
                scheduler
                    .maybe_emit_completion_action(&task_app, &task_pool)
                    .await;
            }

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
        Ok(StartTaskOutcome::Started)
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
    ///
    /// ARC-58: every finishing supervisor runs this gate independently, so
    /// two near-simultaneous completions both passed the liveness check and
    /// fired the action twice (run_command executed twice, shutdown/exit
    /// prompts stacked). Closing the round elects exactly one decider. The
    /// round is closed even when the action is off or declined, so the next
    /// batch starts from a clean slate.
    async fn maybe_emit_completion_action(&self, app: &AppHandle, pool: &SqlitePool) {
        if !should_emit_completion_action(&self.downloads, pool).await {
            return;
        }
        let Some(round_had_failure) = decide_completion_round(&self.completion_round, pool).await
        else {
            tracing::debug!("completion round already decided or still growing; skipping");
            return;
        };
        let Ok(settings) =
            db::get_settings(pool, default_download_dir(app).unwrap_or_default()).await
        else {
            return;
        };
        if settings.completion_action == CompletionAction::None {
            return;
        }
        if !completion_action_permitted(round_had_failure, settings.completion_include_failures) {
            tracing::info!("completion action skipped: a download in this batch failed");
            return;
        }
        if settings.completion_action == CompletionAction::RunCommand {
            if let Err(error) = platform::run_user_command(&settings.completion_run_command).await {
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

/// ARC-58: one completion-action "round": every task started since the
/// previous round was decided.
///
/// The round opens on the first task start after a decision and records the
/// tasks that failed in it. The first supervisor that sees the queue drained
/// closes it; that single winner is the dedup. Failures are re-checked against
/// the stored task state at decision time, so a failure the user retried to
/// success no longer blocks the action, while an earlier failure still does
/// even though a later task happened to finish last. A round that ends in a
/// user pause/cancel is never decided and carries its failures into the next
/// batch, the conservative choice for shutdown/sleep actions.
#[derive(Debug, Default)]
struct CompletionRound {
    open: bool,
    /// Bumped on every start so a decision that raced a new task is dropped.
    generation: u64,
    failed_task_ids: HashSet<String>,
}

impl CompletionRound {
    fn record_start(&mut self) {
        if !self.open {
            self.open = true;
            self.failed_task_ids.clear();
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn record_failure(&mut self, task_id: &str) {
        self.failed_task_ids.insert(task_id.to_string());
    }

    /// `None` when another supervisor already decided this round.
    fn snapshot(&self) -> Option<(u64, Vec<String>)> {
        self.open.then(|| {
            (
                self.generation,
                self.failed_task_ids.iter().cloned().collect(),
            )
        })
    }

    /// Closes the round if no task started since `generation` was taken.
    /// Returns true for exactly one caller per round.
    fn close(&mut self, generation: u64) -> bool {
        if !self.open || self.generation != generation {
            return false;
        }
        self.open = false;
        true
    }
}

/// Poison-tolerant lock: the guarded data is plain bookkeeping that stays
/// consistent even if a holder panicked mid-update.
fn lock_round(
    round: &std::sync::Mutex<CompletionRound>,
) -> std::sync::MutexGuard<'_, CompletionRound> {
    round
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// ARC-58: closes the current round and reports whether it still holds an
/// unresolved failure. `None` means another supervisor decided it already,
/// or a task started meanwhile (that task's supervisor decides later).
async fn decide_completion_round(
    round: &std::sync::Mutex<CompletionRound>,
    pool: &SqlitePool,
) -> Option<bool> {
    let (generation, failed_ids) = lock_round(round).snapshot()?;
    let mut had_failure = false;
    for task_id in &failed_ids {
        match db::get_task_record(pool, task_id).await {
            // Deleted since it failed: nothing is left for the user to act on.
            Ok(None) => {}
            Ok(Some(task)) if !failure_is_unresolved(&task) => {}
            // Still failed, or unknown: a DB error must not let a destructive
            // action through on incomplete information.
            _ => {
                had_failure = true;
                break;
            }
        }
    }
    lock_round(round).close(generation).then_some(had_failure)
}

/// A recorded failure still counts while the task is failed/needs attention
/// or completed with a checksum mismatch. A retry that completed resolves it.
fn failure_is_unresolved(task: &TaskRecord) -> bool {
    matches!(task.status, TaskStatus::Failed | TaskStatus::NeedsAttention)
        || task.hash_status == HashVerificationStatus::Failed
}

/// ARC-58: `completion_include_failures` off means "only when every download
/// in the batch succeeded"; on means "whenever the queue drains".
fn completion_action_permitted(round_had_failure: bool, include_failures: bool) -> bool {
    include_failures || !round_had_failure
}

/// DB persistence half of a download failure (status write + failed event +
/// segment failure marks) with no UI emits. Kept separate from the convergence
/// wrapper so the supervisor can run headlessly in tests (ARC-40).
async fn mark_download_failure_state(pool: &SqlitePool, task_id: &str, error: &str) {
    persist_failure_state(pool, task_id, error, FailureRowScope::Active).await;
}

/// Which DB rows a failure write may overwrite. The active-only scope keeps
/// the R-2.4 guarantee for worker errors; the queued scope exists because the
/// dispatch start path can fail before any state change (ARC-41).
enum FailureRowScope {
    Active,
    Queued,
}

/// Shared body of both failure scopes — one place for the status choice
/// (ARC-16 code dispatch), the event insert, and the segment failure marks.
async fn persist_failure_state(
    pool: &SqlitePool,
    task_id: &str,
    error: &str,
    scope: FailureRowScope,
) {
    tracing::error!(task_id = task_id, error = %error, "download failed");
    // ARC-16: dispatch only on structured code — never on human message text.
    let code = crate::models::AppErrorPayload::code_from_stored(None, Some(error));
    let status = if code
        .as_deref()
        .is_some_and(crate::models::AppErrorPayload::is_needs_attention_code)
    {
        TaskStatus::NeedsAttention
    } else {
        TaskStatus::Failed
    };
    // R-2.4: Conditional UPDATE — never overwrite a user-initiated state
    // change (pause/cancel/delete) that raced the failure.
    let updated = match scope {
        FailureRowScope::Active => {
            db::mark_task_failed_if_active(pool, task_id, status, Some(error), Some(error)).await
        }
        FailureRowScope::Queued => {
            db::mark_task_failed_if_queued(pool, task_id, status, Some(error), Some(error)).await
        }
    };
    let updated = match updated {
        Ok(updated) => updated,
        Err(db_error) => {
            tracing::warn!(
                task_id = task_id,
                error = %db_error,
                "failed to persist task failure status"
            );
            return;
        }
    };
    if !updated {
        tracing::warn!(
            task_id = task_id,
            "mark_download_failed: task state changed concurrently, skipping emit"
        );
        return;
    }
    // The failure event names its class: needs_attention entries get their
    // own event so recovery surfaces can find them without re-classifying
    // the payload.
    let event_type = if status == TaskStatus::NeedsAttention {
        "needs_attention"
    } else {
        "failed"
    };
    if let Err(db_error) = db::insert_task_event(pool, task_id, event_type, Some(error)).await {
        tracing::warn!(
            task_id = task_id,
            error = %db_error,
            "failed to persist task failure event"
        );
    }
    if let Err(db_error) = db::update_segments_status_for_task(
        pool,
        task_id,
        crate::models::SegmentStatus::Failed,
        Some(error),
    )
    .await
    {
        tracing::warn!(
            task_id = task_id,
            error = %db_error,
            "failed to persist segment failure status"
        );
    }
}

/// ARC-41: failure handling for a start that failed while the row was still
/// Queued (header/proxy resolution or transition errors happen before any
/// state change). The worker-side failure paths match active rows; this one
/// matches the queued row so the task becomes a visible failure instead of
/// sitting at the queue head, silently re-failing every dispatch tick.
async fn mark_queued_start_failed(
    app: Option<&AppHandle>,
    pool: &SqlitePool,
    task_id: &str,
    error: String,
) {
    persist_failure_state(pool, task_id, &error, FailureRowScope::Queued).await;
    let Some(app) = app else { return };
    if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
        emit_task_progress_snapshot(app, &task);
        emit_task_updated_record(app, pool, &task).await;
    }
    emit_queue_changed_with_ids(app, Some(vec![task_id.to_string()]));
}

/// Shared tail of the dispatch start-failure branch (ARC-41). Re-reads the
/// task: a still-Queued row is failed via the queued matcher; any other state
/// only gets a progress snapshot (its transition already happened elsewhere);
/// `app: None` (headless tests) skips every emit.
async fn handle_start_failure(
    app: Option<&AppHandle>,
    pool: &SqlitePool,
    task_id: &str,
    error: String,
) {
    match db::get_task_record(pool, task_id).await {
        Ok(Some(current)) if current.status == TaskStatus::Queued => {
            mark_queued_start_failed(app, pool, task_id, error).await;
        }
        Ok(Some(current)) => {
            let Some(app) = app else { return };
            emit_task_progress_snapshot(app, &current);
            emit_task_updated_record(app, pool, &current).await;
        }
        _ => {}
    }
}

/// ARC-46: the completion-action liveness gate. Hash verification runs AFTER
/// the worker released its control entry, so a downloads-only check fires the
/// action while the last SHA-256 is still running — the machine powers off
/// with hash_status stuck at Pending and the user must re-verify manually.
/// Both verify paths set `hash_status = 'pending'` before hashing, so "some
/// completed row is mid-hash" is the exact window to hold the action for.
/// A DB error is treated the same way as a non-empty queue: skip firing this
/// tick rather than risk a destructive action on incomplete information.
async fn should_emit_completion_action(
    downloads: &Arc<Mutex<HashMap<String, DownloadControl>>>,
    pool: &SqlitePool,
) -> bool {
    if !downloads.lock().await.is_empty() {
        return false;
    }
    if db::list_queued_task_records(pool, 1)
        .await
        .map(|tasks| !tasks.is_empty())
        .unwrap_or(true)
    {
        return false;
    }
    matches!(db::any_completed_task_hash_pending(pool).await, Ok(false))
}

/// Supervisor convergence shared by every engine outcome (panic, error,
/// success): release the active slot and the request-header cache, persist the
/// failure when the engine ended in error and the user did not cancel, and
/// evict the A-4 runtime-lock entry.
///
/// Extracted from the supervisor closure so the panic path can be driven
/// headlessly in tests (ARC-40) — a real `AppHandle<Wry>` cannot be built
/// outside a running Tauri app, so in-crate tests pass `app: None` and skip
/// the emits. Production passes `Some(&task_app)`.
///
/// Note: the A-4 evict now runs before the success-path hash verification.
/// The evict only drops an idle registry entry (guards are never held by this
/// worker), so the reorder is unobservable to hash verification and user
/// actions.
// The parameters mirror the supervisor's local bindings one-to-one; a
// parameter struct would only relocate this list without a second call site.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn converge_download_outcome(
    downloads: &Arc<Mutex<HashMap<String, DownloadControl>>>,
    request_headers: &TaskRequestHeaders,
    task_runtime_locks: &Arc<crate::TaskRuntimeLocks>,
    app: Option<&AppHandle>,
    pool: &SqlitePool,
    task_id: &str,
    canceled: bool,
    result: Result<(), String>,
) {
    // ARC-62: a timed-out user stop retains the slot. Converge to Paused when
    // the engine eventually drains, even though that command already returned.
    if canceled {
        if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
            if matches!(task.status, TaskStatus::Downloading | TaskStatus::Retrying) {
                let _ = db::update_task_status(
                    pool,
                    task_id,
                    TaskStatus::Paused,
                    Some(task.status),
                    0,
                    0,
                    Some("Paused"),
                    None,
                )
                .await;
                if let Some(app) = app {
                    if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
                        emit_task_progress_snapshot(app, &task);
                        emit_task_updated_record(app, pool, &task).await;
                    }
                }
            }
        }
    }
    if let Err(error) = result {
        if !canceled {
            let mut handled = false;
            match db::auto_retry_attempt(pool, task_id).await {
                Ok(previous_attempt) => {
                    if let Some(plan) =
                        crate::download::retry::task_retry_plan(&error, previous_attempt)
                    {
                        match db::schedule_auto_retry(
                            pool,
                            task_id,
                            plan.attempt,
                            &plan.retry_after_at,
                            &plan.reason,
                            &plan.error,
                        )
                        .await
                        {
                            Ok(db::AutoRetryOutcome::Scheduled { .. })
                            | Ok(db::AutoRetryOutcome::Exhausted { .. })
                            | Ok(db::AutoRetryOutcome::StateChanged) => {
                                handled = true;
                            }
                            Err(db_error) => {
                                tracing::warn!(
                                    task_id = %task_id,
                                    error = %db_error,
                                    "automatic retry state update failed; preserving terminal failure"
                                );
                            }
                        }
                    }
                }
                Err(db_error) => {
                    tracing::warn!(
                        task_id = %task_id,
                        error = %db_error,
                        "automatic retry budget lookup failed"
                    );
                }
            }
            if !handled {
                mark_download_failure_state(pool, task_id, &error).await;
            }
            if let Some(app) = app {
                if let Ok(Some(task)) = db::get_task_record(pool, task_id).await {
                    emit_task_progress_snapshot(app, &task);
                    emit_task_updated_record(app, pool, &task).await;
                }
                emit_queue_changed_with_ids(app, Some(vec![task_id.to_string()]));
            }
        }
    } else if !canceled {
        if let Err(db_error) = db::clear_auto_retry_state(pool, task_id).await {
            tracing::warn!(
                task_id = %task_id,
                error = %db_error,
                "failed to clear automatic retry state after success"
            );
        }
    }
    let _ = downloads.lock().await.remove(task_id);
    let _ = request_headers.lock().await.remove(task_id);
    // A-4: Evict the runtime lock entry now that the worker has finished
    // and the downloads_map/request_headers entries are removed. If a user
    // action (pause/cancel/delete/retry) is concurrently holding the lock,
    // strong_count > 1 and evict is a safe no-op. This prevents completed/
    // failed/paused task entries from accumulating indefinitely in the
    // registry (only delete_* previously evicted).
    task_runtime_locks.evict(task_id).await;
}

fn min_optional_limit(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn active_scheduled_speed_limit(settings: &AppSettings) -> Option<i64> {
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
}

/// ARC-40: renders a caught engine panic as the download failure message.
pub(crate) fn describe_engine_panic(payload: Box<dyn std::any::Any + Send>) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    format!("The download engine crashed: {detail}")
}

#[cfg(test)]
mod engine_panic_tests {
    use super::describe_engine_panic;

    #[test]
    fn str_payload_is_described() {
        let message = describe_engine_panic(Box::new("boom"));
        assert_eq!(message, "The download engine crashed: boom");
    }

    #[test]
    fn string_payload_is_described() {
        let message = describe_engine_panic(Box::new(String::from("blew up")));
        assert_eq!(message, "The download engine crashed: blew up");
    }

    #[test]
    fn opaque_payload_falls_back() {
        let message = describe_engine_panic(Box::new(7_u32));
        assert_eq!(message, "The download engine crashed: unknown panic");
    }
}

#[cfg(test)]
mod convergence_tests {
    //! ARC-40: the supervisor's convergence after an engine panic must release
    //! the active/host slot, transition the task to Failed, and leave nothing
    //! behind that blocks the next task.
    //!
    //! The full dispatch path needs a real `AppHandle<Wry>`, which cannot be
    //! constructed headlessly, so these tests drive the same
    //! [`super::converge_download_outcome`] the supervisor calls, feeding it a
    //! REAL caught panic rendered through [`super::describe_engine_panic`] —
    //! the identical glue the supervisor uses between `catch_unwind` and
    //! convergence.

    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    use super::{converge_download_outcome, describe_engine_panic, Scheduler};
    use crate::download::{EngineRegistry, GlobalSpeedLimiter};
    use crate::models::task::now_iso;
    use crate::models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus};
    use crate::{db, DownloadControl, TaskRequestHeaders, TaskRuntimeLocks};

    fn task_record(id: &str, status: TaskStatus) -> TaskRecord {
        let now = now_iso();
        TaskRecord {
            id: id.to_string(),
            url: "https://panic-host/file.bin".to_string(),
            final_url: None,
            protocol: "https".to_string(),
            task_kind: TaskKind::SingleFile,
            file_name: format!("{id}.bin"),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: None,
            final_path: None,
            total_size: 0,
            downloaded_bytes: 0,
            status,
            etag: None,
            last_modified: None,
            content_type: None,
            supports_resume: true,
            supports_parallel: false,
            supports_multi_file: false,
            source_key: "panic-host".to_string(),
            connection_count: 0,
            speed_bps: 0,
            task_speed_limit_bps: None,
            priority: TaskPriority::Normal,
            queue_position: 0,
            category_key: None,
            obey_schedule: false,
            health_summary: None,
            error_message: None,
            error_code: None,
            recovery_actions: Vec::new(),
            retry_after_at: None,
            expected_hash_sha256: None,
            actual_hash_sha256: None,
            hash_status: HashVerificationStatus::NotRequested,
            hash_error: None,
            hash_verified_at: None,
            created_at: now.clone(),
            updated_at: now,
            files_version: 0,
        }
    }

    async fn test_pool(label: &str) -> sqlx::SqlitePool {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vibe-sched-conv-{label}-{id}.sqlite"));
        db::connect(&path)
            .await
            .expect("database connect with migrations")
            .pool
    }

    fn control(source_key: &str) -> DownloadControl {
        DownloadControl {
            cancel_token: CancellationToken::new(),
            finish: Arc::new(AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: GlobalSpeedLimiter::disabled(),
            handle: None,
            source_key: source_key.to_string(),
            connection_slots: 1,
        }
    }

    /// A download future that panics when polled — the fake-engine shape the
    /// audit's acceptance asks for. The supervisor wraps exactly such a future
    /// in `catch_unwind`.
    fn panicking_download() -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> {
        Box::pin(async { panic!("engine exploded mid-download") })
    }

    #[tokio::test]
    async fn engine_panic_releases_slot_and_fails_task() {
        let pool = test_pool("panic").await;
        let task = task_record("task-arc40-panic", TaskStatus::Downloading);
        db::insert_task_record(&pool, &task).await.expect("insert");

        let downloads: Arc<Mutex<HashMap<String, DownloadControl>>> =
            Arc::new(Mutex::new(HashMap::new()));
        downloads
            .lock()
            .await
            .insert(task.id.clone(), control("panic-host"));
        let request_headers: TaskRequestHeaders = Arc::new(Mutex::new(HashMap::new()));
        request_headers.lock().await.insert(
            task.id.clone(),
            vec![("authorization".into(), "Bearer x".into())],
        );
        let task_runtime_locks = Arc::new(TaskRuntimeLocks::default());

        // Mirror the supervisor glue exactly: catch_unwind around the engine
        // future, panic payload rendered by describe_engine_panic, then the
        // shared convergence body.
        let result = match futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(
            panicking_download(),
        ))
        .await
        {
            Ok(result) => result,
            Err(payload) => Err(describe_engine_panic(payload)),
        };
        converge_download_outcome(
            &downloads,
            &request_headers,
            &task_runtime_locks,
            None,
            &pool,
            &task.id,
            false,
            result,
        )
        .await;

        assert!(
            downloads.lock().await.is_empty(),
            "the panicked control must not keep occupying the active/host slot"
        );
        assert!(
            request_headers.lock().await.is_empty(),
            "the header cache entry must be evicted"
        );
        let stored = db::get_task_record(&pool, &task.id)
            .await
            .expect("query")
            .expect("task exists");
        assert_eq!(stored.status, TaskStatus::Failed);
        let message = stored.error_message.as_deref().unwrap_or_default();
        assert!(
            message.contains("download engine crashed") && message.contains("engine exploded"),
            "panic detail must reach the failure message, got: {message}"
        );

        // A follow-up task on the same host converges cleanly: the released
        // slot is reusable and a healthy outcome touches nothing.
        let next = task_record("task-arc40-next", TaskStatus::Queued);
        db::insert_task_record(&pool, &next).await.expect("insert");
        downloads
            .lock()
            .await
            .insert(next.id.clone(), control("panic-host"));
        converge_download_outcome(
            &downloads,
            &request_headers,
            &task_runtime_locks,
            None,
            &pool,
            &next.id,
            false,
            Ok(()),
        )
        .await;
        assert!(downloads.lock().await.is_empty());
        let stored_next = db::get_task_record(&pool, &next.id)
            .await
            .expect("query")
            .expect("task exists");
        assert_eq!(
            stored_next.status,
            TaskStatus::Queued,
            "a healthy convergence must not touch the task status"
        );
    }

    #[tokio::test]
    async fn transient_failure_after_coordinator_fail_state_is_queued_for_retry() {
        let pool = test_pool("auto-retry").await;
        let task = task_record("task-auto-retry", TaskStatus::Downloading);
        db::insert_task_record(&pool, &task).await.expect("insert");
        assert!(db::mark_task_failed_if_active(
            &pool,
            &task.id,
            TaskStatus::Failed,
            Some("Failed"),
            Some("temporary server failure"),
        )
        .await
        .expect("coordinator marks failure"));

        let downloads: Arc<Mutex<HashMap<String, DownloadControl>>> = Arc::new(Mutex::new(
            HashMap::from([(task.id.clone(), control("panic-host"))]),
        ));
        let request_headers: TaskRequestHeaders = Arc::new(Mutex::new(HashMap::new()));
        let task_runtime_locks = Arc::new(TaskRuntimeLocks::default());
        let error = crate::models::AppErrorPayload::new(
            "server_error",
            "The server returned HTTP 503.",
            true,
            vec!["retry_later"],
        )
        .command_error();

        converge_download_outcome(
            &downloads,
            &request_headers,
            &task_runtime_locks,
            None,
            &pool,
            &task.id,
            false,
            Err(error),
        )
        .await;

        let queued = db::get_task_record(&pool, &task.id)
            .await
            .expect("load retried task")
            .expect("task exists");
        assert_eq!(queued.status, TaskStatus::Queued);
        assert!(queued.retry_after_at.is_some());
        assert_eq!(db::auto_retry_attempt(&pool, &task.id).await.unwrap(), 1);
        assert!(downloads.lock().await.is_empty());
    }

    #[tokio::test]
    async fn active_speed_policy_refresh_updates_task_and_scheduled_limits() {
        let pool = test_pool("speed-policy").await;
        let mut task = task_record("task-speed-policy", TaskStatus::Downloading);
        task.task_speed_limit_bps = Some("400000".to_string());
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert task");

        let mut settings = db::get_settings(&pool, std::env::temp_dir().to_string_lossy().into())
            .await
            .expect("load settings");
        settings.schedule_speed_limit_window_enabled = true;
        settings.schedule_speed_limit_window_start = "00:00".to_string();
        settings.schedule_speed_limit_window_end = "00:00".to_string();
        settings.schedule_speed_limit_bps = Some("200000".to_string());
        db::upsert_settings(&pool, &settings)
            .await
            .expect("save scheduled limit");

        let global = Arc::new(GlobalSpeedLimiter::new(Some(900_000)));
        let task_limiter = GlobalSpeedLimiter::with_parent(global.clone(), None);
        let downloads = Arc::new(Mutex::new(HashMap::from([(
            task.id.clone(),
            DownloadControl {
                cancel_token: CancellationToken::new(),
                finish: Arc::new(AtomicBool::new(false)),
                finish_notify: Arc::new(tokio::sync::Notify::new()),
                speed_limiter: task_limiter.clone(),
                handle: None,
                source_key: task.source_key.clone(),
                connection_slots: 1,
            },
        )])));
        let scheduler = Scheduler::new(
            downloads,
            Arc::new(Mutex::new(HashMap::new())),
            global.clone(),
            Arc::new(EngineRegistry::new().expect("engine registry")),
            Arc::new(crate::TaskRuntimeLocks::default()),
        );

        scheduler
            .refresh_speed_limit_policies(&pool, std::env::temp_dir().to_string_lossy().into())
            .await
            .expect("apply scheduled policy");
        assert_eq!(task_limiter.current_limit_bps(), Some(200_000));

        sqlx::query("UPDATE tasks SET task_speed_limit_bps = ? WHERE id = ?")
            .bind("100000")
            .bind(&task.id)
            .execute(&pool)
            .await
            .expect("change task limit");
        scheduler
            .refresh_speed_limit_policies(&pool, std::env::temp_dir().to_string_lossy().into())
            .await
            .expect("apply task limit");
        assert_eq!(task_limiter.current_limit_bps(), Some(100_000));

        settings.schedule_speed_limit_window_enabled = false;
        settings.schedule_speed_limit_bps = None;
        db::upsert_settings(&pool, &settings)
            .await
            .expect("clear scheduled limit");
        sqlx::query("UPDATE tasks SET task_speed_limit_bps = NULL WHERE id = ?")
            .bind(&task.id)
            .execute(&pool)
            .await
            .expect("clear task limit");
        scheduler
            .refresh_speed_limit_policies(&pool, std::env::temp_dir().to_string_lossy().into())
            .await
            .expect("clear child limit");
        assert_eq!(task_limiter.current_limit_bps(), Some(900_000));

        global.set_limit(Some(500_000)).await;
        assert_eq!(task_limiter.current_limit_bps(), Some(500_000));
    }

    #[tokio::test]
    async fn canceled_outcome_cleans_up_without_overwriting_user_state() {
        // R-2.4 / ARC-62: after a timed-out stop drains, reflect the user's pause
        // without allowing a teardown error to overwrite it as Failed.
        let pool = test_pool("cancel").await;
        let task = task_record("task-arc40-cancel", TaskStatus::Downloading);
        db::insert_task_record(&pool, &task).await.expect("insert");

        let downloads: Arc<Mutex<HashMap<String, DownloadControl>>> =
            Arc::new(Mutex::new(HashMap::new()));
        downloads
            .lock()
            .await
            .insert(task.id.clone(), control("panic-host"));
        let request_headers: TaskRequestHeaders = Arc::new(Mutex::new(HashMap::new()));
        let task_runtime_locks = Arc::new(TaskRuntimeLocks::default());

        converge_download_outcome(
            &downloads,
            &request_headers,
            &task_runtime_locks,
            None,
            &pool,
            &task.id,
            true,
            Err("engine exploded during cancellation".to_string()),
        )
        .await;

        assert!(
            downloads.lock().await.is_empty(),
            "runtime slot must still be released on cancel"
        );
        let stored = db::get_task_record(&pool, &task.id)
            .await
            .expect("query")
            .expect("task exists");
        assert_eq!(
            stored.status,
            TaskStatus::Paused,
            "a drained canceled task must settle as paused, not failed"
        );
    }

    #[test]
    fn arc44_start_outcome_accounting_counts_only_started() {
        // ARC-44: only Started may consume a slot. AlreadyActive is already
        // inside the downloads map the dispatch counts were seeded from, and
        // ConflictSkipped never spawned a worker — counting either inflated
        // active/host usage for the rest of the tick.
        let mut active_count = 3usize;
        let mut host_slot_map = HashMap::from([("panic-host".to_string(), 4usize)]);

        super::account_start_outcome(
            &mut active_count,
            &mut host_slot_map,
            super::StartTaskOutcome::Started,
            "panic-host",
            2,
        );
        assert_eq!(active_count, 4);
        assert_eq!(host_slot_map["panic-host"], 6);

        super::account_start_outcome(
            &mut active_count,
            &mut host_slot_map,
            super::StartTaskOutcome::AlreadyActive,
            "panic-host",
            2,
        );
        super::account_start_outcome(
            &mut active_count,
            &mut host_slot_map,
            super::StartTaskOutcome::ConflictSkipped,
            "other-host",
            1,
        );
        assert_eq!(active_count, 4, "non-Started outcomes must not count");
        assert_eq!(host_slot_map["panic-host"], 6, "no double counting");
        assert!(
            !host_slot_map.contains_key("other-host"),
            "ConflictSkipped must not reserve host slots"
        );
    }

    #[tokio::test]
    async fn corrupt_headers_leave_queue_and_do_not_block_next_reservation() {
        use base64::Engine as _;
        crate::secure_headers::install_test_secret_key(
            &base64::engine::general_purpose::STANDARD.encode([3; 32]),
        );
        let pool = test_pool("corrupt-metadata").await;
        for id in ["bad-headers", "good-next"] {
            db::insert_task_record(&pool, &task_record(id, TaskStatus::Queued))
                .await
                .unwrap();
        }
        db::upsert_task_request_headers(
            &pool,
            "bad-headers",
            &[("cookie".into(), "session=private".into())],
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE task_request_headers SET nonce = '' WHERE task_id = 'bad-headers'")
            .execute(&pool)
            .await
            .unwrap();
        let headers: TaskRequestHeaders = Arc::default();
        let mut started = Vec::new();
        // Drive the production reservation precondition and failure convergence
        // with real encrypted rows. No AppHandle or fake decryption is needed.
        for task in db::list_queued_task_records(&pool, 8).await.unwrap() {
            match crate::commands::tasks::resolve_task_request_headers(
                &pool,
                headers.clone(),
                &task.id,
            )
            .await
            {
                Err(error) => super::handle_start_failure(None, &pool, &task.id, error).await,
                Ok(_) => {
                    db::update_task_status(
                        &pool,
                        &task.id,
                        TaskStatus::Downloading,
                        Some(TaskStatus::Queued),
                        0,
                        1,
                        None,
                        None,
                    )
                    .await
                    .unwrap()
                    .unwrap();
                    started.push(task.id);
                }
            }
        }
        assert_eq!(started, ["good-next"]);
        assert!(db::list_queued_task_records(&pool, 8)
            .await
            .unwrap()
            .is_empty());
        let failed = db::get_task_record(&pool, "bad-headers")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            failed.error_code.as_deref(),
            Some("auth_headers_unavailable")
        );
        assert_ne!(failed.status, TaskStatus::Downloading);
        pool.close().await;
    }

    #[tokio::test]
    async fn arc41_queued_start_failure_becomes_visible_failure() {
        // ARC-41: a start failure on a still-Queued row (header/proxy
        // resolution or transition error) must produce a visible failure, not
        // a silent no-op that leaves the task re-failing at the queue head.
        let pool = test_pool("queued-start-fail").await;
        let task = task_record("task-arc41-queued", TaskStatus::Queued);
        db::insert_task_record(&pool, &task).await.expect("insert");

        super::handle_start_failure(
            None,
            &pool,
            &task.id,
            "request header resolution failed".to_string(),
        )
        .await;

        let stored = db::get_task_record(&pool, &task.id)
            .await
            .expect("query")
            .expect("task exists");
        assert_eq!(
            stored.status,
            TaskStatus::Failed,
            "queued start failure must leave the queue instead of idling"
        );
        assert!(
            stored.error_message.is_some(),
            "failure must carry a diagnosable error message"
        );
        assert_eq!(stored.error_code, None, "plain errors must not fake a code");
    }

    #[tokio::test]
    async fn arc41_queued_start_failure_maps_needs_attention_codes() {
        // Structured needs-attention payloads (e.g. remote_changed) route to
        // NeedsAttention with recovery actions, same as worker failures.
        let pool = test_pool("queued-start-na").await;
        let task = task_record("task-arc41-na", TaskStatus::Queued);
        db::insert_task_record(&pool, &task).await.expect("insert");

        let payload = crate::models::AppErrorPayload::new(
            "remote_changed",
            "The remote file changed since the download started.",
            false,
            vec!["restart", "check_url"],
        );
        let error = serde_json::to_string(&payload).expect("serialize payload");

        super::handle_start_failure(None, &pool, &task.id, error).await;

        let stored = db::get_task_record(&pool, &task.id)
            .await
            .expect("query")
            .expect("task exists");
        assert_eq!(stored.status, TaskStatus::NeedsAttention);
        assert_eq!(
            stored.error_code.as_deref(),
            Some("remote_changed"),
            "the structured code must be persisted for the recovery surface"
        );
    }

    #[tokio::test]
    async fn arc41_non_queued_start_failure_keeps_snapshot_semantics() {
        // Regression: for rows that already left the queue, the branch must
        // not write any state — it only refreshes the UI snapshot.
        let pool = test_pool("queued-start-active").await;
        let task = task_record("task-arc41-active", TaskStatus::Downloading);
        db::insert_task_record(&pool, &task).await.expect("insert");

        super::handle_start_failure(None, &pool, &task.id, "boom".to_string()).await;

        let stored = db::get_task_record(&pool, &task.id)
            .await
            .expect("query")
            .expect("task exists");
        assert_eq!(
            stored.status,
            TaskStatus::Downloading,
            "non-queued rows must keep their state (snapshot-only branch)"
        );
    }

    /// ARC-58: two supervisors observing the same drained queue must not both
    /// fire; only the next task start opens a round that may fire again.
    #[test]
    fn arc58_round_is_decided_once_until_the_next_start() {
        let mut round = super::CompletionRound::default();
        assert!(
            round.snapshot().is_none(),
            "no task ran yet: nothing to decide"
        );

        round.record_start();
        let (generation, _) = round.snapshot().expect("round opened by the start");
        assert!(round.close(generation), "the first decider wins");
        assert!(!round.close(generation), "a concurrent decider is deduped");
        assert!(round.snapshot().is_none(), "a decided round stays decided");

        round.record_start();
        let (generation, _) = round.snapshot().expect("the next start re-arms");
        assert!(round.close(generation), "the new round may fire once more");
    }

    /// ARC-58: a task that starts between the snapshot and the close belongs
    /// to the round; closing on the stale snapshot would strand it.
    #[test]
    fn arc58_round_close_rejects_a_snapshot_that_raced_a_start() {
        let mut round = super::CompletionRound::default();
        round.record_start();
        let (stale, _) = round.snapshot().expect("open");
        round.record_start();
        assert!(
            !round.close(stale),
            "the late start must keep the round open"
        );
        let (fresh, _) = round.snapshot().expect("still open");
        assert!(round.close(fresh));
    }

    /// ARC-58: failures survive later starts in the same round (a failure
    /// followed by a successful last task must still count) and are cleared
    /// only when a new round opens after a decision.
    #[test]
    fn arc58_round_failures_persist_within_round_and_clear_on_next_round() {
        let mut round = super::CompletionRound::default();
        round.record_start();
        round.record_failure("early-failure");
        round.record_start();
        let (generation, failed) = round.snapshot().expect("open");
        assert_eq!(failed, vec!["early-failure".to_string()]);
        assert!(round.close(generation));

        round.record_start();
        let (_, failed) = round.snapshot().expect("new round");
        assert!(failed.is_empty(), "a new batch starts from a clean slate");
    }

    #[test]
    fn arc58_include_failures_gates_only_rounds_with_failures() {
        assert!(super::completion_action_permitted(false, false));
        assert!(
            !super::completion_action_permitted(true, false),
            "default off: a failed download in the batch holds the action"
        );
        assert!(super::completion_action_permitted(true, true));
        assert!(super::completion_action_permitted(false, true));
    }

    /// ARC-58 end to end against stored task state: an unresolved failure
    /// blocks, a failure the user retried to completion does not, and a
    /// checksum mismatch counts as a failure.
    #[tokio::test]
    async fn arc58_round_decision_rechecks_recorded_failures() {
        let pool = test_pool("completion-round").await;
        let failed = task_record("round-failed", TaskStatus::Failed);
        db::insert_task_record(&pool, &failed)
            .await
            .expect("insert");
        let round = std::sync::Mutex::new(super::CompletionRound::default());

        super::lock_round(&round).record_start();
        super::lock_round(&round).record_failure(&failed.id);
        super::lock_round(&round).record_start();
        assert_eq!(
            super::decide_completion_round(&round, &pool).await,
            Some(true),
            "an earlier failure counts even when the last task succeeded"
        );
        assert_eq!(
            super::decide_completion_round(&round, &pool).await,
            None,
            "the round was already decided"
        );

        // Retried to completion inside the next round: resolved.
        super::lock_round(&round).record_start();
        super::lock_round(&round).record_failure(&failed.id);
        db::update_task_status(
            &pool,
            &failed.id,
            TaskStatus::Completed,
            None,
            0,
            0,
            None,
            None,
        )
        .await
        .expect("retry completed");
        assert_eq!(
            super::decide_completion_round(&round, &pool).await,
            Some(false)
        );

        // A completed download whose checksum failed is still a failure.
        let mut mismatch = task_record("round-hash-mismatch", TaskStatus::Completed);
        mismatch.hash_status = HashVerificationStatus::Failed;
        db::insert_task_record(&pool, &mismatch)
            .await
            .expect("insert");
        super::lock_round(&round).record_start();
        super::lock_round(&round).record_failure(&mismatch.id);
        assert_eq!(
            super::decide_completion_round(&round, &pool).await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn arc46_completion_action_holds_for_pending_hash() {
        // ARC-46: hash verification runs after the worker released its slot,
        // so a completed row with hash_status=pending is mid-hash right now —
        // firing shutdown here would interrupt it and stick the status.
        let pool = test_pool("completion-hash-pending").await;
        let mut task = task_record("task-arc46-pending", TaskStatus::Completed);
        task.hash_status = HashVerificationStatus::Pending;
        db::insert_task_record(&pool, &task).await.expect("insert");

        let downloads: Arc<Mutex<HashMap<String, DownloadControl>>> =
            Arc::new(Mutex::new(HashMap::new()));
        assert!(
            !super::should_emit_completion_action(&downloads, &pool).await,
            "a mid-hash completion must hold the action"
        );

        db::update_hash_verification(
            &pool,
            &task.id,
            None,
            HashVerificationStatus::Verified,
            None,
        )
        .await
        .expect("resolve hash status");
        assert!(
            super::should_emit_completion_action(&downloads, &pool).await,
            "once the hash resolves the gate must open"
        );
    }

    #[tokio::test]
    async fn arc46_completion_action_unaffected_without_checksums() {
        // Tasks without an expected hash never reach Pending; the gate must
        // not hold completion actions hostage for them.
        let pool = test_pool("completion-hash-notreq").await;
        let task = task_record("task-arc46-plain", TaskStatus::Completed);
        db::insert_task_record(&pool, &task).await.expect("insert");
        let downloads: Arc<Mutex<HashMap<String, DownloadControl>>> =
            Arc::new(Mutex::new(HashMap::new()));
        assert!(super::should_emit_completion_action(&downloads, &pool).await);
    }

    #[tokio::test]
    async fn arc46_completion_action_still_gated_on_activity_and_queue() {
        // The hash gate extends the existing liveness checks, never replaces
        // them: active downloads and a non-empty queue keep holding the
        // action regardless of hash state.
        let pool = test_pool("completion-hash-queue").await;
        let mut task = task_record("task-arc46-done", TaskStatus::Completed);
        task.hash_status = HashVerificationStatus::Verified;
        db::insert_task_record(&pool, &task).await.expect("insert");
        let queued = task_record("task-arc46-queued", TaskStatus::Queued);
        db::insert_task_record(&pool, &queued)
            .await
            .expect("insert");

        let downloads: Arc<Mutex<HashMap<String, DownloadControl>>> =
            Arc::new(Mutex::new(HashMap::new()));
        downloads
            .lock()
            .await
            .insert("task-x".to_string(), control("panic-host"));
        assert!(
            !super::should_emit_completion_action(&downloads, &pool).await,
            "active downloads hold the action"
        );

        downloads.lock().await.clear();
        assert!(
            !super::should_emit_completion_action(&downloads, &pool).await,
            "a non-empty queue holds the action"
        );
    }
}
