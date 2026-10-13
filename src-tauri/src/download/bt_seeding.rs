//! B6: background torrents own their workers and session leases independently of downloads.

use super::bt::{
    seeding_limit_reached, torrent_dht_status, torrent_piece_bitfield, torrent_runtime_error,
    SessionRefGuard,
};
use crate::{db, events::emit_task_updated_record, models::TaskRecord};
use librqbit::api::{Api, TorrentIdOrHash};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

pub(super) struct BtSeedRuntime {
    pub api: Arc<Api>,
    pub torrent_id: TorrentIdOrHash,
    pub guard: SessionRefGuard,
    pub pool: sqlx::SqlitePool,
    pub task: TaskRecord,
    pub app: Option<AppHandle>,
    /// Uploads in previous sessions; never add a session's counter twice.
    pub uploaded_base: i64,
    pub upload_limiter: Arc<super::GlobalSpeedLimiter>,
}

struct SeedingControl {
    cancel: CancellationToken,
    reason: StdMutex<&'static str>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Default)]
pub struct BtSeedingManager {
    controls: Arc<Mutex<HashMap<String, Arc<SeedingControl>>>>,
    starting: Arc<std::sync::atomic::AtomicUsize>,
}

pub(crate) struct SeedStartGuard(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for SeedStartGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl BtSeedingManager {
    pub async fn active_count(&self) -> usize {
        self.controls.lock().await.len() + self.starting.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub(crate) fn begin_start(&self) -> SeedStartGuard {
        self.starting
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SeedStartGuard(self.starting.clone())
    }
    pub async fn contains(&self, id: &str) -> bool {
        self.controls.lock().await.contains_key(id)
    }

    pub(super) async fn register(&self, runtime: BtSeedRuntime) -> Result<(), String> {
        let id = runtime.task.id.clone();
        let control = Arc::new(SeedingControl {
            cancel: CancellationToken::new(),
            reason: StdMutex::new("shutdown"),
            handle: Mutex::new(None),
        });
        let mut controls = self.controls.lock().await;
        let error = if controls.contains_key(&id) {
            Some(super::error::engine_error(
                "bt_seeding_active",
                "This torrent already has a seeding owner",
                false,
            ))
        } else {
            db::begin_bt_seeding(&runtime.pool, &id).await.err()
        };
        if let Some(error) = error {
            drop(controls);
            runtime.guard.cancel_storage();
            let _ = runtime
                .api
                .api_torrent_action_forget(runtime.torrent_id)
                .await;
            drop(runtime.api);
            runtime.guard.release().await;
            return Err(error);
        }
        controls.insert(id.clone(), control.clone());
        let manager = self.clone();
        let worker_control = control.clone();
        let handle = tokio::spawn(async move {
            let BtSeedRuntime {
                api,
                torrent_id,
                guard,
                pool,
                task,
                app,
                uploaded_base,
                upload_limiter,
            } = runtime;
            let outcome =
                futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(run_seeding(
                    &api,
                    torrent_id,
                    &pool,
                    &task,
                    &worker_control,
                    uploaded_base,
                    &upload_limiter,
                )))
                .await;
            let reason = match outcome {
                Ok(Ok(reason)) => reason,
                Ok(Err(error)) => {
                    let _ =
                        db::insert_task_event(&pool, &task.id, "bt_seeding_failed", Some(&error))
                            .await;
                    let _ = sqlx::query("UPDATE torrent_runtime_snapshots SET last_error_code = 'bt_runtime_failed', last_error_message = NULL WHERE task_id = ?").bind(&task.id).execute(&pool).await;
                    "runtime_error"
                }
                Err(_) => {
                    let _ = db::insert_task_event(
                        &pool,
                        &task.id,
                        "bt_seeding_failed",
                        Some("bt_runtime_failed"),
                    )
                    .await;
                    "runtime_error"
                }
            };
            guard.cancel_storage();
            let _ = api.api_torrent_action_forget(torrent_id).await;
            // Drop every API reference before draining storage leases. stop()
            // signals librqbit; it does not establish that files are released.
            drop(api);
            guard.release().await;
            if let Ok(Some(state)) = db::bt_seeding_status(&pool, &task.id).await {
                let _ = db::checkpoint_bt_seeding(
                    &pool,
                    &task.id,
                    state.uploaded_bytes,
                    state.elapsed_seconds,
                    Some(reason),
                )
                .await;
            }
            let _ =
                db::insert_task_event(&pool, &task.id, "bt_seeding_stopped", Some(reason)).await;
            if let Ok(Some(current)) = db::get_task_record(&pool, &task.id).await {
                emit_task_updated_record(&app, &pool, &current).await;
            }
            manager.controls.lock().await.remove(&task.id);
            if let Some(app) = app.as_ref() {
                if let Some(state) = app.try_state::<crate::AppState>() {
                    state
                        .scheduler
                        .maybe_emit_completion_action(app, &pool)
                        .await;
                }
            }
        });
        *control.handle.lock().await = Some(handle);
        drop(controls);
        Ok(())
    }

    pub async fn stop(
        &self,
        id: &str,
        reason: &'static str,
        timeout: Option<Duration>,
    ) -> Result<(), String> {
        let control = self.controls.lock().await.get(id).cloned();
        let Some(control) = control else {
            return Ok(());
        };
        *control
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = reason;
        control.cancel.cancel();
        let mut handle = control.handle.lock().await;
        if let Some(worker) = handle.as_mut() {
            match timeout {
                Some(budget) => {
                    tokio::time::timeout(budget, worker)
                        .await
                        .map_err(|_| crate::task_stop_pending_error())?
                        .map_err(|e| e.to_string())?;
                }
                None => {
                    worker.await.map_err(|e| e.to_string())?;
                }
            }
            *handle = None;
        }
        Ok(())
    }

    pub async fn drain(&self, timeout: Option<Duration>) -> Result<(), String> {
        let ids = {
            let controls = self.controls.lock().await;
            for control in controls.values() {
                control.cancel.cancel();
            }
            controls.keys().cloned().collect::<Vec<_>>()
        };
        let stop = async {
            for id in ids {
                self.stop(&id, "shutdown", None).await?;
            }
            Ok::<_, String>(())
        };
        match timeout {
            Some(budget) => tokio::time::timeout(budget, stop)
                .await
                .map_err(|_| crate::task_stop_pending_error())?,
            None => stop.await,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_seeding(
    api: &Api,
    torrent_id: TorrentIdOrHash,
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
    control: &SeedingControl,
    uploaded_base: i64,
    upload_limiter: &super::GlobalSpeedLimiter,
) -> Result<&'static str, String> {
    let initial = db::bt_seeding_status(pool, &task.id)
        .await?
        .ok_or_else(|| "Missing seeding state".to_string())?;
    let started = Instant::now();
    let mut last_checkpoint = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);
    let (bitfield, piece_count) = torrent_piece_bitfield(api, torrent_id);
    let mut last_snapshot = None;
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! { _ = control.cancel.cancelled() => {
            let stats = api.api_stats_v1(torrent_id).ok();
            let uploaded = stats.map(|stats| uploaded_base.saturating_add(i64::try_from(stats.uploaded_bytes).unwrap_or(i64::MAX))).unwrap_or(initial.uploaded_bytes);
            db::checkpoint_bt_seeding(pool, &task.id, uploaded, initial.elapsed_seconds.saturating_add(started.elapsed().as_secs() as i64), None).await?;
            return Ok(*control.reason.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        }, _ = interval.tick() => {} }
        let stats = api
            .api_stats_v1(torrent_id)
            .map_err(|e| super::error::engine_error("bt_runtime_failed", format!("{e:#}"), true))?;
        if let Some(error) = torrent_runtime_error(api, torrent_id, &stats) {
            return Err(error);
        }
        let policy = db::bt_seeding_status(pool, &task.id)
            .await?
            .ok_or_else(|| "Missing seeding state".to_string())?;
        let uploaded =
            uploaded_base.saturating_add(i64::try_from(stats.uploaded_bytes).unwrap_or(i64::MAX));
        let elapsed = initial
            .elapsed_seconds
            .saturating_add(i64::try_from(started.elapsed().as_secs()).unwrap_or(i64::MAX));
        if !policy.enabled {
            db::checkpoint_bt_seeding(pool, &task.id, uploaded, elapsed, None).await?;
            return Ok("manual");
        }
        let total = task
            .total_size
            .max(i64::try_from(stats.progress_bytes).unwrap_or(i64::MAX));
        let ratio = if total > 0 {
            uploaded as f64 / total as f64
        } else {
            0.0
        };
        if last_checkpoint.elapsed() >= Duration::from_secs(5) || uploaded > policy.uploaded_bytes {
            db::checkpoint_bt_seeding(pool, &task.id, uploaded, elapsed, None).await?;
            last_checkpoint = Instant::now();
        }
        if seeding_limit_reached(
            ratio,
            policy.ratio_limit,
            Duration::from_secs(elapsed.max(0) as u64),
            policy
                .time_limit_seconds
                .filter(|s| *s > 0)
                .map(|s| Duration::from_secs(s as u64)),
        ) {
            db::checkpoint_bt_seeding(pool, &task.id, uploaded, elapsed, None).await?;
            return Ok("limit");
        }
        upload_limiter
            .set_limit_preserving_budget(db::get_bt_upload_limit_bps_setting(pool).await)
            .await;
        let speed = stats
            .live
            .as_ref()
            .map(|live| i64::try_from(live.upload_speed.as_bytes()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let peers = stats
            .live
            .as_ref()
            .map(|live| i64::from(live.snapshot.peer_stats.live))
            .unwrap_or(0);
        let dht = torrent_dht_status(api);
        let fingerprint = (uploaded, speed, peers, dht.clone());
        if last_snapshot.as_ref() != Some(&fingerprint) {
            db::upsert_torrent_runtime_snapshot(
                pool,
                &task.id,
                db::TorrentRuntimeSnapshotUpsert {
                    metadata_status: "seeding",
                    completed_pieces: piece_count,
                    verified_pieces: piece_count,
                    piece_count,
                    piece_bitfield_base64: if last_snapshot.is_none() {
                        bitfield.as_deref()
                    } else {
                        None
                    },
                    peer_count: peers,
                    seed_count: 0,
                    dht_status: dht.as_deref(),
                    trackers_json: None,
                    upload_bytes: uploaded,
                    upload_speed_bps: speed,
                    ratio,
                    seeding_enabled: true,
                    seeding_state: "seeding",
                    last_error_code: None,
                    last_error_message: None,
                },
            )
            .await?;
            last_snapshot = Some(fingerprint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timed_out_stop_keeps_the_owner_until_its_worker_drains() {
        let manager = BtSeedingManager::default();
        let control = Arc::new(SeedingControl {
            cancel: CancellationToken::new(),
            reason: StdMutex::new("shutdown"),
            handle: Mutex::new(None),
        });
        let release = Arc::new(tokio::sync::Notify::new());
        manager
            .controls
            .lock()
            .await
            .insert("late".into(), control.clone());
        let worker_manager = manager.clone();
        let worker_release = release.clone();
        let worker_cancel = control.cancel.clone();
        *control.handle.lock().await = Some(tokio::spawn(async move {
            worker_cancel.cancelled().await;
            worker_release.notified().await;
            worker_manager.controls.lock().await.remove("late");
        }));
        assert!(manager
            .stop("late", "manual", Some(Duration::from_millis(20)))
            .await
            .unwrap_err()
            .contains("task_stop_pending"));
        assert_eq!(manager.active_count().await, 1);
        release.notify_one();
        manager.stop("late", "manual", None).await.unwrap();
        assert_eq!(manager.active_count().await, 0);
    }
}
