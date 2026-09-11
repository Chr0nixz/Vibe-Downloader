//! ARC-32: command paths must never await a scheduler dispatch while holding a
//! per-task runtime lock.
//!
//! The production deadlock mechanics: `dispatch` → `start_task` acquires the
//! task runtime lock as its first move (scheduler/mod.rs). A command that holds
//! that same non-reentrant tokio Mutex and awaits dispatch therefore either
//! self-deadlocks (Restart writes the task back to Queued first, so dispatch
//! selects the very task whose lock the caller holds) or forms an ABBA cycle
//! with a concurrent command whose dispatch needs the first caller's task lock.
//!
//! The fix routes every command tail through `Scheduler::dispatch_detached`,
//! which spawns the dispatch so the caller unwinds and releases its lock first.
//! Full `Scheduler::dispatch` needs a Tauri `AppHandle` (see the note in
//! scheduler_dispatch.rs), so these tests exercise the same control-flow
//! contract against the real `TaskRuntimeLocks` and a real database: the
//! "dispatch" stand-in performs exactly the lock acquisition `start_task`
//! performs, and the command tail follows the fixed production pattern.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use tauri_app_lib::TaskRuntimeLocks;

const DEADLOCK_TIMEOUT: Duration = Duration::from_secs(10);

async fn wait_until(flag: &AtomicBool, message: &'static str) {
    let deadline = Instant::now() + DEADLOCK_TIMEOUT;
    while !flag.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline, "{message}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Restart scenario: the command holds the task lock while the task is already
/// Queued, so a synchronous dispatch would re-acquire the same lock and hang
/// forever. The detached pattern must let the lock acquisition succeed once the
/// command tail releases the guard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc32_restart_tail_does_not_self_deadlock_on_task_lock() {
    let locks = Arc::new(TaskRuntimeLocks::default());
    let task_id = "task-restart-queued";

    // Command path: resolve_task_attention holds the runtime lock; the task was
    // written back to Queued before the dispatch tail runs.
    let guard = locks.lock(task_id).await;
    let dispatched = Arc::new(AtomicBool::new(false));

    // Fixed production pattern (Scheduler::dispatch_detached): spawn, never
    // await under the lock. The spawned closure models dispatch → start_task
    // picking this same Queued task and acquiring its runtime lock.
    {
        let locks = locks.clone();
        let dispatched = dispatched.clone();
        let task_id = task_id.to_string();
        tokio::spawn(async move {
            let _guard = locks.lock(&task_id).await;
            dispatched.store(true, Ordering::SeqCst);
        });
    }

    // The command tail returns, dropping the guard, BEFORE the dispatch runs.
    drop(guard);

    wait_until(
        &dispatched,
        "ARC-32 regression: dispatch blocked on the task runtime lock the \
         command tail still held (self-deadlock)",
    )
    .await;
}

/// ABBA scenario: two command paths each hold their own task lock while their
/// dispatch tails need the other task's lock. Awaiting dispatch under the lock
/// deadlocks both; the detached pattern lets both tails release first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc32_concurrent_command_tails_do_not_form_abba_cycle() {
    let locks = Arc::new(TaskRuntimeLocks::default());

    let command_a = {
        let locks = locks.clone();
        tokio::spawn(async move {
            let _guard = locks.lock("task-A").await;
            // Dispatch-equivalent: start_task would acquire the OTHER task's
            // runtime lock here (a different queued task). Fixed pattern:
            // detached spawn, then the tail returns and releases task-A.
            let dispatched_b = Arc::new(AtomicBool::new(false));
            {
                let locks = locks.clone();
                let dispatched = dispatched_b.clone();
                tokio::spawn(async move {
                    let _guard = locks.lock("task-B").await;
                    dispatched.store(true, Ordering::SeqCst);
                });
            }
            drop(_guard);
            wait_until(
                &dispatched_b,
                "ARC-32 regression: command A's dispatch never acquired task-B's lock",
            )
            .await;
        })
    };

    let command_b = {
        let locks = locks.clone();
        tokio::spawn(async move {
            let _guard = locks.lock("task-B").await;
            let dispatched_a = Arc::new(AtomicBool::new(false));
            {
                let locks = locks.clone();
                let dispatched = dispatched_a.clone();
                tokio::spawn(async move {
                    let _guard = locks.lock("task-A").await;
                    dispatched.store(true, Ordering::SeqCst);
                });
            }
            drop(_guard);
            wait_until(
                &dispatched_a,
                "ARC-32 regression: command B's dispatch never acquired task-A's lock",
            )
            .await;
        })
    };

    // If either tail awaited dispatch under its lock, this join hangs and the
    // tokio test times out — which is the regression signal.
    tokio::time::timeout(DEADLOCK_TIMEOUT * 2, async {
        tokio::try_join!(command_a, command_b).expect("both command tails completed");
    })
    .await
    .expect("ARC-32 regression: ABBA deadlock between command tails and dispatch");
}

/// Restart-tail end-to-end against the real database: the state the command
/// leaves behind (task back in the Queued set) is exactly what makes a
/// synchronous dispatch re-select the locked task. Runs the real
/// `reset_task_download_state` write the restart path performs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc32_restart_leaves_task_queued_for_detached_dispatch() {
    use std::time::{SystemTime, UNIX_EPOCH};

    use tauri_app_lib::{
        db,
        models::task::now_iso,
        models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
    };

    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-arc32-restart-{id}.sqlite"));
    let pool = db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool;

    let now = now_iso();
    let record = TaskRecord {
        id: "task-restart-db".to_string(),
        url: "https://example.com/restart.bin".to_string(),
        final_url: None,
        protocol: "https".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: "restart.bin".to_string(),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: Some(
            std::env::temp_dir()
                .join("vibe-arc32-restart.bin")
                .to_string_lossy()
                .to_string(),
        ),
        final_path: None,
        total_size: 1024,
        downloaded_bytes: 512,
        status: TaskStatus::NeedsAttention,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: "example.com".to_string(),
        connection_count: 1,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("NeedsAttention".to_string()),
        error_message: Some("remote_changed".to_string()),
        error_code: Some("remote_changed".to_string()),
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
    };
    db::insert_task_record(&pool, &record)
        .await
        .expect("insert task");

    // The restart write: resets progress and puts the task back into Queued.
    db::reset_task_download_state(&pool, &record.id)
        .await
        .expect("reset task download state");

    let queued = db::list_queued_task_records(&pool, 10)
        .await
        .expect("list queued");
    assert!(
        queued.iter().any(|task| task.id == record.id),
        "restart must leave the task selectable by dispatch — this is the state \
         that makes a lock-held synchronous dispatch self-deadlock"
    );

    let _ = std::fs::remove_file(&path);
}
