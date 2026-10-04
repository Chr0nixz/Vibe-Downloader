//! ARC-57: a checkpoint that lands after a user action (pause/cancel/retry)
//! must persist byte progress but must NOT resurrect `downloading`.
//!
//! The pre-fix shape wrote `status = ?` unconditionally, so a worker detached
//! by the old `timeout(5s, handle)` drain (NAS/SMB flush stall, busy DB) could
//! flip a Paused task back to `downloading` — a worker-less zombie row that
//! rejected the user's next resume (Downloading → Queued is an illegal
//! transition) until an app restart ran `reset_interrupted_tasks`.
//!
//! Part A of the fix (`remove_and_drain_control` on every control path)
//! makes the detached worker impossible; the first test pins it with a
//! stalled-flush worker. The rest pin Part B, the DB-level conditional guard
//! that is the second half of the contract and also covers the
//! FTP/SFTP/HLS/DASH/Metalink shared progress writers.
mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri_app_lib::{
    db,
    models::task::now_iso,
    models::{
        HashVerificationStatus, SegmentStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus,
    },
    remove_and_drain_control, state_machine, DownloadControl,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn sample_task_record(id: &str, status: TaskStatus) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("https://example.com/{id}"),
        final_url: None,
        protocol: "https".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 1024 * 1024,
        downloaded_bytes: 0,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("https://example.com/{id}"),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
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

async fn status_of(pool: &sqlx::SqlitePool, id: &str) -> TaskStatus {
    db::get_task_record(pool, id)
        .await
        .expect("query task")
        .expect("task exists")
        .status
}

/// Clears its flag when the worker future is dropped, i.e. finished or aborted.
struct AliveGuard(Arc<AtomicBool>);

impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// ARC-57 / ARC-62: a slow flush survives the command deadline, but cannot
/// be followed by a state transition until a later stop confirms it exited.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_drain_retains_stalled_flush_until_a_later_stop_completes() {
    let (_db, pool) = common::test_pool("arc57-pause-drain").await;
    let task = sample_task_record("arc57-pause-drain", TaskStatus::Downloading);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");
    let segment_id = segments[0].id.clone();

    let worker_alive = Arc::new(AtomicBool::new(true));
    let late_checkpoint_ran = Arc::new(AtomicBool::new(false));
    let cancel = CancellationToken::new();
    let worker = {
        let pool = pool.clone();
        let task_id = task.id.clone();
        let segment_id = segment_id.clone();
        let alive = worker_alive.clone();
        let ran = late_checkpoint_ran.clone();
        tokio::spawn(async move {
            let _alive = AliveGuard(alive);
            // The stalled flush: the cancel token is never consulted.
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let _ = db::checkpoint_task_progress(
                &pool,
                db::TaskProgressCheckpoint {
                    task_id: &task_id,
                    downloaded_bytes: 8192,
                    speed_bps: 1024,
                    connection_count: 1,
                    status: "downloading",
                    update_files: false,
                },
                &[(segment_id, 8192, 1024, "downloading".to_string())],
            )
            .await;
            ran.store(true, Ordering::SeqCst);
        })
    };
    let downloads = Mutex::new(HashMap::from([(
        task.id.clone(),
        DownloadControl {
            cancel_token: cancel.clone(),
            finish: Arc::new(AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
            handle: Some(worker),
            source_key: task.source_key.clone(),
            connection_slots: 1,
        },
    )]));
    let request_headers = Mutex::new(HashMap::from([(
        task.id.clone(),
        vec![("Cookie".to_string(), "stale=1".to_string())],
    )]));

    let started = Instant::now();
    remove_and_drain_control(
        &downloads,
        &request_headers,
        &task.id,
        Duration::from_millis(200),
    )
    .await
    .expect_err("slow flush still owns the run");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the drain must be bounded by the grace, not by the stalled flush"
    );
    assert!(
        worker_alive.load(Ordering::SeqCst),
        "timeout must retain the writer instead of pretending it exited"
    );
    assert!(downloads.lock().await.contains_key(&task.id));
    assert!(request_headers.lock().await.contains_key(&task.id));
    remove_and_drain_control(
        &downloads,
        &request_headers,
        &task.id,
        Duration::from_secs(3),
    )
    .await
    .expect("flush finished before state change");
    assert!(!worker_alive.load(Ordering::SeqCst));
    assert!(downloads.lock().await.is_empty());
    assert!(request_headers.lock().await.is_empty());

    let no_app = Option::<tauri::AppHandle>::None;
    state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &task.id,
        TaskStatus::Paused,
        0,
        0,
        Some("Paused"),
        Some("paused"),
        Some("Paused"),
        SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("pause transition");

    // The checkpoint completed under the old owner, before the pause transition.
    assert!(
        late_checkpoint_ran.load(Ordering::SeqCst),
        "the flush checkpoint must finish before ownership is released"
    );
    assert_eq!(status_of(&pool, &task.id).await, TaskStatus::Paused);
    state_machine::transition_task(
        &no_app,
        &pool,
        &task.id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        Some("resumed"),
    )
    .await
    .expect("resume must be a legal transition from the stable Paused state");

    pool.close().await;
}

/// The live path must stay exactly as before: while the task is owned by a
/// running engine, the checkpoint writes status, bytes and work units.
#[tokio::test]
async fn live_checkpoint_still_writes_status_and_bytes() {
    let (_db, pool) = common::test_pool("arc57-live").await;
    let task = sample_task_record("arc57-live", TaskStatus::Downloading);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");
    let segment_id = segments[0].id.clone();

    db::checkpoint_task_progress(
        &pool,
        db::TaskProgressCheckpoint {
            task_id: &task.id,
            downloaded_bytes: 4096,
            speed_bps: 2048,
            connection_count: 2,
            status: "downloading",
            update_files: false,
        },
        &[(segment_id.clone(), 4096, 2048, "downloading".to_string())],
    )
    .await
    .expect("checkpoint");

    let record = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(record.status, TaskStatus::Downloading);
    assert_eq!(record.downloaded_bytes, 4096);
    assert_eq!(record.speed_bps, 2048);
    let segment = db::get_first_segment_record(&pool, &task.id)
        .await
        .expect("segment query")
        .expect("segment");
    assert_eq!(segment.downloaded_until, 4096);
    assert_eq!(segment.status, SegmentStatus::Downloading);

    pool.close().await;
}

/// The zombie sequence: the control plane moved the task to Paused, then a
/// late checkpoint from the (pre-fix detached) worker arrives. Bytes must
/// persist for an honest resume offset; status and work-unit status must not
/// be resurrected to downloading/pending-with-speed.
#[tokio::test]
async fn late_checkpoint_after_pause_keeps_bytes_but_not_status() {
    let (_db, pool) = common::test_pool("arc57-late-pause").await;
    let task = sample_task_record("arc57-late-pause", TaskStatus::Downloading);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");
    let segment_id = segments[0].id.clone();

    // The user action: pause already ran the state machine and reset units.
    db::update_segments_status_for_task(&pool, &task.id, SegmentStatus::Pending, None)
        .await
        .expect("reset segments");
    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Paused,
        None,
        0,
        0,
        Some("Paused"),
        None,
    )
    .await
    .expect("pause transition");

    // The late checkpoint from the worker that outlived the 5s drain.
    db::checkpoint_task_progress(
        &pool,
        db::TaskProgressCheckpoint {
            task_id: &task.id,
            downloaded_bytes: 8192,
            speed_bps: 1024,
            connection_count: 1,
            status: "downloading",
            update_files: false,
        },
        &[(segment_id.clone(), 8192, 1024, "downloading".to_string())],
    )
    .await
    .expect("late checkpoint must not error");

    let record = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(
        record.status,
        TaskStatus::Paused,
        "a late checkpoint must not resurrect downloading"
    );
    assert_eq!(
        record.downloaded_bytes, 8192,
        "durable bytes must still persist for an honest resume"
    );
    let segment = db::get_first_segment_record(&pool, &task.id)
        .await
        .expect("segment query")
        .expect("segment");
    assert_eq!(
        segment.downloaded_until, 8192,
        "the resume offset must persist even when the status write is gated"
    );
    assert_ne!(
        segment.status,
        SegmentStatus::Downloading,
        "the unit status belongs to the control plane once it moved the task"
    );

    // The user's next resume must be a legal transition from the stored state.
    assert!(
        TaskStatus::Paused.can_transition_to(TaskStatus::Queued),
        "resume path (Paused -> Queued) must stay legal"
    );

    pool.close().await;
}

/// Same guard on the shared engine progress writer used by FTP/SFTP/HLS/DASH
/// (update_task_progress): a late `downloading` write after cancel degrades to
/// bytes-only, while the engine's own finalization writes (Paused,
/// WaitingNetwork) stay unconditional.
#[tokio::test]
async fn late_engine_progress_write_is_gated_after_cancel() {
    let (_db, pool) = common::test_pool("arc57-late-cancel").await;
    let task = sample_task_record("arc57-late-cancel", TaskStatus::Downloading);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    // cancel_task transitioned the row to Failed.
    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Failed,
        None,
        0,
        0,
        Some("Canceled by user."),
        None,
    )
    .await
    .expect("cancel transition");

    // Late live-progress write from a detached worker: bytes only.
    db::update_task_progress(&pool, &task.id, 512_000, 4096, 2, TaskStatus::Downloading)
        .await
        .expect("late write");
    let record = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(record.status, TaskStatus::Failed);
    assert_eq!(record.downloaded_bytes, 512_000);
    assert_eq!(record.speed_bps, 0, "gated write must not publish a speed");

    // The engine's own cancel-path finalization (Paused) is unconditional.
    db::update_task_progress(&pool, &task.id, 512_000, 0, 0, TaskStatus::Paused)
        .await
        .expect("engine finalization");
    assert_eq!(status_of(&pool, &task.id).await, TaskStatus::Paused);

    pool.close().await;
}

/// update_task_runtime_progress (Metalink live path) carries the same gate,
/// and a gated write must not touch the health summary either.
#[tokio::test]
async fn late_metalink_runtime_progress_is_gated() {
    let (_db, pool) = common::test_pool("arc57-metalink-gate").await;
    let mut task = sample_task_record("arc57-metalink-gate", TaskStatus::Downloading);
    task.protocol = "metalink".to_string();
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Paused,
        None,
        0,
        0,
        Some("Paused"),
        None,
    )
    .await
    .expect("pause");

    db::update_task_runtime_progress(
        &pool,
        &task.id,
        262_144,
        1024,
        1,
        TaskStatus::Downloading,
        Some("Downloading Metalink file"),
    )
    .await
    .expect("late runtime write");

    let record = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(record.status, TaskStatus::Paused);
    assert_eq!(record.downloaded_bytes, 262_144);
    assert_ne!(
        record.health_summary.as_deref(),
        Some("Downloading Metalink file"),
        "a gated write must not publish engine health copy over the paused row"
    );

    pool.close().await;
}

/// update_task_and_segment_progress (HTTP direct path) gates task AND unit
/// together — the unit must not be reset to downloading/pending-with-speed
/// after the control plane moved the task.
#[tokio::test]
async fn late_task_and_segment_progress_is_gated() {
    let (_db, pool) = common::test_pool("arc57-direct-gate").await;
    let task = sample_task_record("arc57-direct-gate", TaskStatus::Downloading);
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    let segments = db::ensure_task_segments(&pool, &task)
        .await
        .expect("segments");
    let segment_id = segments[0].id.clone();

    db::update_segments_status_for_task(&pool, &task.id, SegmentStatus::Pending, None)
        .await
        .expect("reset segments");
    db::update_task_status(
        &pool,
        &task.id,
        TaskStatus::Paused,
        None,
        0,
        0,
        Some("Paused"),
        None,
    )
    .await
    .expect("pause");

    db::update_task_and_segment_progress(
        &pool,
        &task.id,
        &segment_id,
        65_536,
        2048,
        1,
        TaskStatus::Downloading,
    )
    .await
    .expect("late direct write");

    let record = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(record.status, TaskStatus::Paused);
    assert_eq!(record.downloaded_bytes, 65_536);
    let segment = db::get_first_segment_record(&pool, &task.id)
        .await
        .expect("segment query")
        .expect("segment");
    assert_eq!(segment.downloaded_until, 65_536);
    assert_ne!(segment.status, SegmentStatus::Downloading);

    pool.close().await;
}
