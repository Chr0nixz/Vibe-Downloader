use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::{
    commands::tasks::{emit_task_progress_snapshot, prepare_task_for_download, resolve_task_request_headers},
    commands::settings::default_download_dir,
    db,
    download::{DownloadContext, EngineRegistry, GlobalSpeedLimiter},
    events::{emit_completion_action_requested, emit_queue_changed, emit_task_updated_record},
    logging::sanitize_url,
    models::{
        CompletionAction, CompletionActionRequestedPayload, HashVerificationStatus, TaskStatus,
        TaskRecord,
    },
    platform, DownloadControl, TaskRequestHeaders,
};

/// 下载调度器，封装活跃下载映射、请求头缓存、全局限速器、引擎注册表等共享状态。
///
/// 所有调度方法通过 `self: Arc<Self>` 调用，以便在 spawned task 中克隆 `Arc` 继续调度。
/// `app` 和 `pool` 不存入结构体，每次调用时传入（避免 Scheduler 持有 DB 连接池生命周期）。
pub struct Scheduler {
    /// 互斥锁，保证调度串行执行（原 AppState.scheduler: Arc<Mutex<()>>）
    lock: Mutex<()>,
    /// 活跃下载映射（与 AppState.downloads 共享同一 Arc）
    downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
    /// 请求头缓存（与 AppState.request_headers 共享同一 Arc）
    request_headers: TaskRequestHeaders,
    /// 全局限速器（与 AppState.speed_limiter 共享同一 Arc）
    speed_limiter: Arc<GlobalSpeedLimiter>,
    /// 引擎注册表（与 AppState.engine_registry 共享同一 Arc）
    engine_registry: Arc<EngineRegistry>,
}

impl Scheduler {
    pub fn new(
        downloads: Arc<Mutex<HashMap<String, DownloadControl>>>,
        request_headers: TaskRequestHeaders,
        speed_limiter: Arc<GlobalSpeedLimiter>,
        engine_registry: Arc<EngineRegistry>,
    ) -> Self {
        Self {
            lock: Mutex::new(()),
            downloads,
            request_headers,
            speed_limiter,
            engine_registry,
        }
    }

    /// 调度入口（原 schedule_queued_tasks）。
    pub async fn dispatch(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        self.dispatch_inner(app, pool).await;
    }

    /// 延迟调度入口（原 spawn_schedule_queued_tasks_after 的调度分支）。
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

    /// 核心调度逻辑（原 schedule_queued_tasks_inner）。
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

        // E-6: 一次性构建 host → 已用槽位 计数表，避免循环内每个任务都 lock +
        // 全量遍历 downloads（O(N×M) + N 次 Mutex lock）。
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
                    // 同步更新本地计数表，后续任务看到最新槽位占用。
                    *host_slot_map.entry(source_key).or_insert(0) += planned_slots;
                }
                Err(error) => {
                    // start_task failed before (or during) insert; the
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

    /// 启动单个任务下载（原 start_task_download）。
    async fn start_task(
        self: Arc<Self>,
        app: AppHandle,
        pool: SqlitePool,
        task: TaskRecord,
        connection_limit: usize,
    ) -> Result<(), String> {
        let task_request_headers =
            resolve_task_request_headers(&pool, self.request_headers.clone(), &task.id).await?;
        let global_proxy_config = self.engine_registry.proxy_config().await;
        let task_proxy_config =
            db::resolve_task_proxy_config(&pool, &task.id, &task.protocol, &global_proxy_config)
                .await?;
        let task = prepare_task_for_download(&app, &pool, &self.engine_registry, task, &task_request_headers)
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
        crate::state_machine::transition_task(
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
        .map_err(String::from)?;

        let finish = Arc::new(AtomicBool::new(false));
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let downloads_map = self.downloads.clone();
        let task_id = task.id.clone();
        let map_task_id = task.id.clone();
        let source_key = task.source_key.clone();
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
                    scheduler.clone().spawn_dispatch(task_app.clone(), task_pool.clone());
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
            let effective_task_limit = min_optional_limit(task_limit_bps, scheduled_limit_bps);
            let task_speed_limiter = GlobalSpeedLimiter::with_parent(
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
            let inner_cancel_token = task_cancel_token.clone();
            let download_handle = tokio::spawn(async move {
                engine
                    .download(DownloadContext {
                        app: inner_app,
                        pool: inner_pool,
                        task,
                        cancel_token: inner_cancel_token,
                        finish: task_finish.clone(),
                        speed_limiter: task_speed_limiter,
                        connection_limit,
                        request_headers: task_request_headers.clone(),
                        proxy_config: task_proxy_config,
                    })
                    .await
            });
            let result = match download_handle.await {
                Ok(result) => result.map_err(String::from),
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
            let canceled = task_cancel_token.is_cancelled();
            let _ = downloads_map.lock().await.remove(&task_id);
            let _ = scheduler
                .request_headers
                .lock()
                .await
                .remove(&task_id);

            if let Err(error) = result {
                if !canceled {
                    mark_download_failed(&task_app, &task_pool, &task_id, error).await;
                }
            } else if !canceled {
                match crate::commands::tasks::verify_task_hash_with_pool(&task_pool, &task_id).await {
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
                scheduler.maybe_emit_completion_action(&task_app, &task_pool).await;
            }

            scheduler.clone().spawn_dispatch(task_app, task_pool);
        });

        self.downloads.lock().await.insert(
            map_task_id,
            DownloadControl {
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

    /// 计算指定 host 已用的连接槽位数（原 host_connection_slots）。
    pub async fn host_used(&self, source_key: &str) -> usize {
        self.downloads
            .lock()
            .await
            .values()
            .filter(|control| control.source_key == source_key)
            .map(|control| control.connection_slots)
            .sum()
    }

    /// 计算可用槽位数（纯函数，无副作用）。
    pub fn compute_available_slots(max_active_tasks: i32, active_count: usize) -> i32 {
        max_active_tasks.saturating_sub(active_count as i32).max(0)
    }

    /// 判断任务是否应被调度窗口跳过（纯函数）。
    pub fn should_skip_for_schedule_window(
        schedule_window_enabled: bool,
        obey_schedule: bool,
        time_window_active: bool,
    ) -> bool {
        schedule_window_enabled && obey_schedule && !time_window_active
    }

    /// 计算 planned_slots，考虑 host_limit 和 host_used（纯函数）。
    pub fn compute_planned_slots(planned: usize, host_limit: usize, host_used: usize) -> usize {
        planned
            .min(host_limit.saturating_sub(host_used))
            .max(1)
    }

    /// 异步触发调度（原 spawn_schedule_queued_tasks）。
    fn spawn_dispatch(self: Arc<Self>, app: AppHandle, pool: SqlitePool) {
        tokio::spawn(async move {
            self.dispatch_inner(app, pool).await;
        });
    }

    /// 延迟异步触发调度（原 spawn_schedule_queued_tasks_after）。
    pub fn spawn_dispatch_after(self: Arc<Self>, app: AppHandle, pool: SqlitePool, delay: std::time::Duration) {
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            self.dispatch_inner(app, pool).await;
        });
    }

    /// 队列空且无活跃任务时发射完成动作（原 maybe_emit_completion_action）。
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

/// 标记任务下载失败并更新 DB/事件（原 commands::tasks::mark_download_failed）。
/// 仅在 scheduler 模块内部使用。
async fn mark_download_failed(app: &AppHandle, pool: &SqlitePool, task_id: &str, error: String) {
    tracing::error!(task_id = %task_id, error = %error, "download failed");
    let payload = serde_json::from_str::<crate::models::AppErrorPayload>(&error).ok();
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

fn min_optional_limit(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}
