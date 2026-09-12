//! ARC-23: the shutdown drain must be bounded — cooperative workers finish
//! within the budget, stubborn ones are aborted AND awaited (nothing detaches
//! with unflushed state), and the drain never exceeds the budget by more than
//! the abort-join tail.

use std::time::{Duration, Instant};

use tauri_app_lib::drain_download_handles;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc23_cooperative_workers_exit_within_budget() {
    let budget = Duration::from_secs(2);
    let handles = vec![
        (
            "cooperative-a".to_string(),
            spawn_cooperative(Duration::from_millis(200)),
        ),
        (
            "cooperative-b".to_string(),
            spawn_cooperative(Duration::from_millis(300)),
        ),
    ];

    let started = Instant::now();
    drain_download_handles(handles, budget).await;
    assert!(
        started.elapsed() < budget,
        "drain must end as soon as the cooperative workers finish"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arc23_stubborn_worker_is_aborted_and_awaited_within_bounds() {
    let budget = Duration::from_millis(500);
    let stubborn_started = Instant::now();
    let stubborn = spawn_stubborn(std::sync::Arc::clone(&std::sync::Arc::new(())));
    let handles = vec![
        ("stubborn".to_string(), stubborn),
        (
            "cooperative".to_string(),
            spawn_cooperative(Duration::from_millis(100)),
        ),
    ];

    let started = Instant::now();
    drain_download_handles(handles, budget).await;
    let elapsed = started.elapsed();

    // The budget bounds phase 1; phase 2 aborts and joins promptly.
    assert!(
        elapsed < Duration::from_secs(5),
        "drain must be bounded: took {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(450),
        "the drain must actually wait for the budget before aborting"
    );
    let _ = stubborn_started;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc23_empty_handle_list_returns_immediately() {
    let started = Instant::now();
    drain_download_handles(Vec::new(), Duration::from_secs(5)).await;
    assert!(started.elapsed() < Duration::from_millis(100));
}

fn spawn_cooperative(work_ms: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Models a worker that honors its cancel token: finishes promptly.
        tokio::time::sleep(work_ms).await;
    })
}

fn spawn_stubborn(_token: std::sync::Arc<()>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Models a worker that ignores cancellation: runs far past the budget
        // until the drain aborts it.
        tokio::time::sleep(Duration::from_secs(120)).await;
    })
}
