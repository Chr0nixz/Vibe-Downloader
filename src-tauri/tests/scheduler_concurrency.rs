//! G-2: True-concurrency regression tests for the scheduler dispatch path.
//!
//! `scheduler_logic.rs` tests the conditional UPDATE invariants by manually
//! reordering DB calls ("simulated race"). These tests use `tokio::spawn` to
//! exercise **real concurrent access** to the same task row, closing the gap
//! between "simulated race" and "actual race".
//!
//! The conditional UPDATE (`UPDATE ... WHERE status = ?`) is the actual
//! serialization mechanism that prevents a worker's terminal write from
//! overwriting a user-initiated state change. These tests verify that it
//! holds under genuine contention.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    db,
    models::task::now_iso,
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
};

// --- Helpers ----------------------------------------------------------------

fn sample_task_record(id: &str, protocol: &str) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("{protocol}://example.com/{id}"),
        final_url: None,
        protocol: protocol.to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 0,
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("{protocol}://example.com/{id}"),
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

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-g2-concurrency-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

async fn insert_queued_task(pool: &sqlx::SqlitePool, id: &str) {
    let task = sample_task_record(id, "http");
    db::insert_task_record(pool, &task)
        .await
        .expect("insert task");
}

async fn insert_downloading_task(pool: &sqlx::SqlitePool, id: &str) {
    let mut task = sample_task_record(id, "http");
    task.status = TaskStatus::Downloading;
    task.total_size = 1024;
    task.downloaded_bytes = 512;
    task.connection_count = 2;
    task.speed_bps = 1024;
    db::insert_task_record(pool, &task)
        .await
        .expect("insert task");
}

async fn fetch_task_status(pool: &sqlx::SqlitePool, id: &str) -> TaskStatus {
    let record = db::get_task_record(pool, id)
        .await
        .expect("query task")
        .expect("task exists");
    record.status
}

// --- G-2: True concurrency tests --------------------------------------------

/// G-2: One thread tries to start the task (Queued → Downloading, simulating
/// dispatch's `start_task`), another tries to fail it (Queued → Failed,
/// simulating a user-initiated abort or external error). Only one conditional
/// UPDATE must succeed; the other must be a no-op (return None).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g2_concurrent_dispatch_and_cancel_only_one_wins() {
    let pool = test_pool("g2-dispatch-cancel").await;
    let task_id = "g2-dispatch-cancel-task";
    insert_queued_task(&pool, task_id).await;

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let id_a = task_id.to_string();
    let id_b = task_id.to_string();

    let start_handle = tokio::spawn(async move {
        db::update_task_status(
            &pool_a,
            &id_a,
            TaskStatus::Downloading,
            Some(TaskStatus::Queued),
            0,
            1,
            Some("Downloading"),
            None,
        )
        .await
        .expect("start call")
    });

    let fail_handle = tokio::spawn(async move {
        db::update_task_status(
            &pool_b,
            &id_b,
            TaskStatus::Failed,
            Some(TaskStatus::Queued),
            0,
            0,
            Some("Aborted"),
            Some("User aborted"),
        )
        .await
        .expect("fail call")
    });

    let start_result = start_handle.await.expect("start join");
    let fail_result = fail_handle.await.expect("fail join");

    let start_succeeded = start_result.is_some();
    let fail_succeeded = fail_result.is_some();
    assert!(
        start_succeeded ^ fail_succeeded,
        "exactly one of dispatch/fail must succeed, got start={start_succeeded}, fail={fail_succeeded}"
    );

    let final_status = fetch_task_status(&pool, task_id).await;
    if start_succeeded {
        assert_eq!(
            final_status,
            TaskStatus::Downloading,
            "dispatch won, status must be Downloading"
        );
    } else {
        assert_eq!(
            final_status,
            TaskStatus::Failed,
            "fail won, status must be Failed"
        );
    }

    pool.close().await;
}

/// G-2: A worker calls `mark_task_failed_if_active` (simulating a download
/// failure) while a user simultaneously pauses (Downloading → Paused). Only
/// one must win; the other must be a no-op.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g2_concurrent_worker_failure_and_pause_only_one_wins() {
    let pool = test_pool("g2-fail-pause").await;
    let task_id = "g2-fail-pause-task";
    insert_downloading_task(&pool, task_id).await;

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let id_a = task_id.to_string();
    let id_b = task_id.to_string();

    let fail_handle = tokio::spawn(async move {
        db::mark_task_failed_if_active(
            &pool_a,
            &id_a,
            TaskStatus::Failed,
            Some("Worker failed"),
            Some("connection reset"),
        )
        .await
        .expect("mark failed call")
    });

    let pause_handle = tokio::spawn(async move {
        db::update_task_status(
            &pool_b,
            &id_b,
            TaskStatus::Paused,
            Some(TaskStatus::Downloading),
            0,
            0,
            Some("Paused"),
            None,
        )
        .await
        .expect("pause call")
    });

    let fail_succeeded = fail_handle.await.expect("fail join");
    let pause_result = pause_handle.await.expect("pause join");
    let pause_succeeded = pause_result.is_some();

    // mark_task_failed_if_active returns bool; pause returns Option.
    // Exactly one must have mutated the row.
    assert!(
        fail_succeeded ^ pause_succeeded,
        "exactly one of fail/pause must succeed, got fail={fail_succeeded}, pause={pause_succeeded}"
    );

    let final_status = fetch_task_status(&pool, task_id).await;
    if fail_succeeded {
        assert_eq!(
            final_status,
            TaskStatus::Failed,
            "worker failure won, status must be Failed"
        );
    } else {
        assert_eq!(
            final_status,
            TaskStatus::Paused,
            "pause won, status must be Paused"
        );
    }

    pool.close().await;
}

/// G-2: Multiple dispatch threads race to start the same Queued task. Only
/// one conditional UPDATE (Queued → Downloading) must succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g2_concurrent_multiple_dispatch_only_one_wins() {
    let pool = test_pool("g2-multi-dispatch").await;
    let task_id = "g2-multi-dispatch-task";
    insert_queued_task(&pool, task_id).await;

    let mut handles = Vec::new();
    for _ in 0..4 {
        let pool_clone = pool.clone();
        let id_clone = task_id.to_string();
        handles.push(tokio::spawn(async move {
            db::update_task_status(
                &pool_clone,
                &id_clone,
                TaskStatus::Downloading,
                Some(TaskStatus::Queued),
                0,
                1,
                Some("Downloading"),
                None,
            )
            .await
            .expect("dispatch call")
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.expect("dispatch join");
        if result.is_some() {
            success_count += 1;
        }
    }

    assert_eq!(
        success_count, 1,
        "exactly one dispatch must succeed, got {success_count}"
    );

    let final_status = fetch_task_status(&pool, task_id).await;
    assert_eq!(
        final_status,
        TaskStatus::Downloading,
        "status must be Downloading after one dispatch won"
    );

    pool.close().await;
}

/// G-2: Concurrent pause and fail on the same Downloading task. Only one
/// must succeed; the task cannot be both Paused and Failed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g2_concurrent_pause_and_fail_only_one_wins() {
    let pool = test_pool("g2-pause-fail").await;
    let task_id = "g2-pause-fail-task";
    insert_downloading_task(&pool, task_id).await;

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let id_a = task_id.to_string();
    let id_b = task_id.to_string();

    let pause_handle = tokio::spawn(async move {
        db::update_task_status(
            &pool_a,
            &id_a,
            TaskStatus::Paused,
            Some(TaskStatus::Downloading),
            0,
            0,
            Some("Paused"),
            None,
        )
        .await
        .expect("pause call")
    });

    let fail_handle = tokio::spawn(async move {
        db::update_task_status(
            &pool_b,
            &id_b,
            TaskStatus::Failed,
            Some(TaskStatus::Downloading),
            0,
            0,
            Some("Aborted"),
            Some("User aborted"),
        )
        .await
        .expect("fail call")
    });

    let pause_result = pause_handle.await.expect("pause join");
    let fail_result = fail_handle.await.expect("fail join");

    let pause_succeeded = pause_result.is_some();
    let fail_succeeded = fail_result.is_some();
    assert!(
        pause_succeeded ^ fail_succeeded,
        "exactly one of pause/fail must succeed, got pause={pause_succeeded}, fail={fail_succeeded}"
    );

    let final_status = fetch_task_status(&pool, task_id).await;
    if pause_succeeded {
        assert_eq!(
            final_status,
            TaskStatus::Paused,
            "pause won, status must be Paused"
        );
    } else {
        assert_eq!(
            final_status,
            TaskStatus::Failed,
            "fail won, status must be Failed"
        );
    }

    pool.close().await;
}
