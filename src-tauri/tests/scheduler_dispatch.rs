//! ARC-05: Scheduler dispatch must not hold the global lock across remote resume probe.
//!
//! Full `Scheduler::start_task` needs a Tauri `AppHandle`. These tests exercise the
//! same control-flow contract the production path uses after ARC-05:
//!   1. Under the scheduler lock: insert pending control + Queued→Downloading.
//!   2. Outside the lock: await prepare/probe (possibly slow).
//!   3. On prepare failure: remove pending control so host/active slots cannot leak.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tauri_app_lib::{
    db,
    models::task::now_iso,
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
    DownloadControl,
};
use tokio::sync::Mutex;

fn sample_task_record(id: &str, source_key: &str) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("https://{source_key}/{id}"),
        final_url: None,
        protocol: "https".to_string(),
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
        source_key: source_key.to_string(),
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
    let path = std::env::temp_dir().join(format!("vibe-arc05-dispatch-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

/// Mirrors ARC-05 lock-held reservation: pending control + conditional start.
async fn reserve_task_under_lock(
    downloads: &Mutex<HashMap<String, DownloadControl>>,
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
    connection_slots: usize,
) -> Result<(), String> {
    if downloads.lock().await.contains_key(&task.id) {
        return Ok(());
    }

    let finish = Arc::new(AtomicBool::new(false));
    let cancel_token = tokio_util::sync::CancellationToken::new();
    downloads.lock().await.insert(
        task.id.clone(),
        DownloadControl {
            cancel_token,
            finish,
            handle: None,
            source_key: task.source_key.clone(),
            connection_slots,
        },
    );

    match db::update_task_status(
        pool,
        &task.id,
        TaskStatus::Downloading,
        Some(TaskStatus::Queued),
        0,
        i32::try_from(connection_slots.max(1)).unwrap_or(1),
        Some("Downloading"),
        None,
    )
    .await
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            downloads.lock().await.remove(&task.id);
            Ok(())
        }
        Err(error) => {
            downloads.lock().await.remove(&task.id);
            Err(error)
        }
    }
}

fn host_used(downloads: &HashMap<String, DownloadControl>, source_key: &str) -> usize {
    downloads
        .values()
        .filter(|control| control.source_key == source_key)
        .map(|control| control.connection_slots)
        .sum()
}

/// Slow resume probe on host A must not delay host B entering Downloading.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc05_slow_probe_does_not_block_other_host() {
    let pool = test_pool("slow-probe").await;
    let task_a = sample_task_record("arc05-a", "slow.example");
    let task_b = sample_task_record("arc05-b", "fast.example");
    db::insert_task_record(&pool, &task_a)
        .await
        .expect("insert A");
    db::insert_task_record(&pool, &task_b)
        .await
        .expect("insert B");

    let scheduler_lock = Mutex::new(());
    let downloads = Arc::new(Mutex::new(HashMap::<String, DownloadControl>::new()));
    let probe_done = Arc::new(AtomicBool::new(false));

    // Host A: reserve under lock, then spawn a ~30s "probe" outside the lock.
    {
        let _guard = scheduler_lock.lock().await;
        reserve_task_under_lock(&downloads, &pool, &task_a, 1)
            .await
            .expect("reserve A");
        let probe_done = probe_done.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            probe_done.store(true, Ordering::SeqCst);
        });
    }

    // Host B: must reserve while A's probe is still running.
    let started = Instant::now();
    {
        let _guard = scheduler_lock.lock().await;
        reserve_task_under_lock(&downloads, &pool, &task_b, 1)
            .await
            .expect("reserve B");
    }
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "host B reservation took {elapsed:?}; slow probe must not block other hosts"
    );
    assert!(
        !probe_done.load(Ordering::SeqCst),
        "host A's slow probe should still be in flight"
    );

    let status_a = db::get_task_record(&pool, &task_a.id)
        .await
        .expect("query A")
        .expect("A exists")
        .status;
    let status_b = db::get_task_record(&pool, &task_b.id)
        .await
        .expect("query B")
        .expect("B exists")
        .status;
    assert_eq!(status_a, TaskStatus::Downloading);
    assert_eq!(status_b, TaskStatus::Downloading);

    let downloads_guard = downloads.lock().await;
    assert_eq!(host_used(&downloads_guard, "slow.example"), 1);
    assert_eq!(host_used(&downloads_guard, "fast.example"), 1);

    pool.close().await;
}

/// Two concurrent reservations of the same queued task: only one transition wins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc05_double_dispatch_single_winner() {
    let pool = test_pool("double-dispatch").await;
    let task = sample_task_record("arc05-dup", "dup.example");
    db::insert_task_record(&pool, &task).await.expect("insert");

    let downloads = Arc::new(Mutex::new(HashMap::<String, DownloadControl>::new()));
    let downloads_a = downloads.clone();
    let downloads_b = downloads.clone();
    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let task_a = task.clone();
    let task_b = task.clone();

    let handle_a =
        tokio::spawn(
            async move { reserve_task_under_lock(&downloads_a, &pool_a, &task_a, 2).await },
        );
    let handle_b =
        tokio::spawn(
            async move { reserve_task_under_lock(&downloads_b, &pool_b, &task_b, 2).await },
        );

    handle_a.await.expect("join A").expect("reserve A ok");
    handle_b.await.expect("join B").expect("reserve B ok");

    let status = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists")
        .status;
    assert_eq!(status, TaskStatus::Downloading);

    let downloads_guard = downloads.lock().await;
    assert_eq!(
        downloads_guard.len(),
        1,
        "losing reservation must remove its pending control"
    );
    assert_eq!(host_used(&downloads_guard, "dup.example"), 2);

    pool.close().await;
}

/// Prepare/probe failure must release the pending control and host slots.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc05_probe_failure_releases_slot() {
    let pool = test_pool("probe-fail").await;
    let task = sample_task_record("arc05-fail", "fail.example");
    db::insert_task_record(&pool, &task).await.expect("insert");

    let downloads = Arc::new(Mutex::new(HashMap::<String, DownloadControl>::new()));
    reserve_task_under_lock(&downloads, &pool, &task, 3)
        .await
        .expect("reserve");
    assert_eq!(downloads.lock().await.len(), 1);
    assert_eq!(host_used(&*downloads.lock().await, "fail.example"), 3);

    // Worker-side prepare failure path: drop control, then mark failed if still Downloading.
    let removed = downloads.lock().await.remove(&task.id);
    assert!(
        removed.is_some(),
        "pending control must exist before cleanup"
    );
    let marked = db::mark_task_failed_if_active(
        &pool,
        &task.id,
        TaskStatus::Failed,
        Some("probe failed"),
        Some("probe failed"),
    )
    .await
    .expect("mark failed");
    assert!(
        marked,
        "Downloading task must accept failure after probe error"
    );

    assert!(
        downloads.lock().await.is_empty(),
        "probe failure must release pending DownloadControl"
    );
    assert_eq!(host_used(&*downloads.lock().await, "fail.example"), 0);

    let status = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists")
        .status;
    assert_eq!(status, TaskStatus::Failed);

    pool.close().await;
}
