//! ARC-06: High-frequency checkpoint + control-plane transitions must not fail
//! with SQLITE_BUSY_SNAPSHOT under concurrency.
//!
//! `transition_task` now uses `BEGIN IMMEDIATE` with bounded BUSY retries.
//! These tests hammer checkpoints against pause/fail/retry transitions and
//! assert zero Database errors plus a coherent terminal control-plane status.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    db,
    models::task::now_iso,
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
    state_machine::{self, TransitionError},
};

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
        downloaded_bytes: 512,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("https://example.com/{id}"),
        connection_count: 2,
        speed_bps: 1024,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("Downloading".to_string()),
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
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-arc06-stress-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

fn is_database_error(error: &TransitionError) -> bool {
    matches!(error, TransitionError::Database(_))
}

/// Concurrent checkpoints + pause/fail transitions: no BUSY_SNAPSHOT Database errors.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc06_checkpoint_and_control_plane_stress() {
    let pool = test_pool("checkpoint-control").await;
    let task_count = 4;
    let mut task_ids = Vec::new();
    for i in 0..task_count {
        let id = format!("arc06-stress-{i}");
        let task = sample_task_record(&id, TaskStatus::Downloading);
        db::insert_task_record(&pool, &task)
            .await
            .expect("insert task");
        task_ids.push(id);
    }

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut checkpoint_handles = Vec::new();
    for id in &task_ids {
        let pool = pool.clone();
        let id = id.clone();
        let stop = stop.clone();
        checkpoint_handles.push(tokio::spawn(async move {
            let mut tick = 0u64;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                tick += 1;
                let result = db::checkpoint_task_progress(
                    &pool,
                    db::TaskProgressCheckpoint {
                        task_id: &id,
                        downloaded_bytes: 512 + tick as i64,
                        speed_bps: 2048,
                        connection_count: 2,
                        status: "downloading",
                        update_files: false,
                    },
                    &[],
                )
                .await;
                if let Err(error) = result {
                    // Checkpoint may race a completed transition; soft-fail only on
                    // non-busy errors would hide ARC-06. Count busy separately below.
                    let msg = error.to_string().to_ascii_lowercase();
                    if msg.contains("busy") || msg.contains("database is locked") {
                        return Err(format!("checkpoint busy error: {error}"));
                    }
                }
                tokio::task::yield_now().await;
            }
            Ok::<(), String>(())
        }));
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut transition_attempts = 0u32;

    while Instant::now() < deadline {
        for (index, id) in task_ids.iter().enumerate() {
            transition_attempts += 1;
            let app: Option<tauri::AppHandle> = None;
            let result = if index % 2 == 0 {
                state_machine::transition_task(
                    &app,
                    &pool,
                    id,
                    TaskStatus::Paused,
                    0,
                    0,
                    Some("Paused"),
                    Some("paused"),
                )
                .await
            } else {
                state_machine::transition_task(
                    &app,
                    &pool,
                    id,
                    TaskStatus::Failed,
                    0,
                    0,
                    Some("Failed"),
                    Some("failed"),
                )
                .await
            };

            match result {
                Ok(_) => {
                    // Re-queue / re-start so the next loop iteration can transition again.
                    let _ = db::update_task_status(
                        &pool,
                        id,
                        TaskStatus::Downloading,
                        None,
                        512,
                        2,
                        Some("Downloading"),
                        None,
                    )
                    .await;
                }
                Err(TransitionError::Conflict { .. }) | Err(TransitionError::Illegal { .. }) => {
                    // Expected under contention / illegal after concurrent change.
                    let _ = db::update_task_status(
                        &pool,
                        id,
                        TaskStatus::Downloading,
                        None,
                        512,
                        2,
                        Some("Downloading"),
                        None,
                    )
                    .await;
                }
                Err(error) if is_database_error(&error) => {
                    panic!(
                        "ARC-06: unexpected Database transition error after {transition_attempts} attempts: {error}"
                    );
                }
                Err(error) => {
                    panic!("unexpected transition error: {error}");
                }
            }
        }
        tokio::task::yield_now().await;
    }

    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    for handle in checkpoint_handles {
        handle
            .await
            .expect("checkpoint join")
            .expect("checkpoint loop must not hit SQLITE_BUSY");
    }

    assert!(
        transition_attempts > 20,
        "stress test should exercise many transitions, got {transition_attempts}"
    );

    // Final control-plane pause must stick (not overwritten by a failed transition).
    let final_id = &task_ids[0];
    let app: Option<tauri::AppHandle> = None;
    let _ = db::update_task_status(
        &pool,
        final_id,
        TaskStatus::Downloading,
        None,
        512,
        2,
        Some("Downloading"),
        None,
    )
    .await;
    state_machine::transition_task(
        &app,
        &pool,
        final_id,
        TaskStatus::Paused,
        0,
        0,
        Some("Paused"),
        Some("paused"),
    )
    .await
    .expect("final pause must succeed");

    let final_status = db::get_task_record(&pool, final_id)
        .await
        .expect("query")
        .expect("exists")
        .status;
    assert_eq!(
        final_status,
        TaskStatus::Paused,
        "final control-plane status must not be overwritten"
    );

    pool.close().await;
}

/// BEGIN IMMEDIATE path still preserves R-1 conditional UPDATE semantics.
#[tokio::test]
async fn arc06_transition_conflict_still_surfaces() {
    let pool = test_pool("conflict-regression").await;
    let task = sample_task_record("arc06-conflict", TaskStatus::Queued);
    db::insert_task_record(&pool, &task).await.expect("insert");

    let app: Option<tauri::AppHandle> = None;
    state_machine::transition_task(
        &app,
        &pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        1,
        Some("Downloading"),
        Some("started"),
    )
    .await
    .expect("first transition");

    let err = state_machine::transition_task(
        &app,
        &pool,
        &task.id,
        TaskStatus::Downloading,
        0,
        1,
        Some("Downloading"),
        Some("started"),
    )
    .await
    .expect_err("second Queued→Downloading must fail");

    assert!(
        matches!(
            err,
            TransitionError::Illegal { .. } | TransitionError::Conflict { .. }
        ),
        "expected Conflict/Illegal, got {err:?}"
    );

    pool.close().await;
}
