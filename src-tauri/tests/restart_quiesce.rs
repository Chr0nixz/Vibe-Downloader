//! ARC-45: the restart path must quiesce the worker before deleting temp
//! files. A bare abort takes effect at the worker's next await, so a worker
//! that is mid-write keeps the temp handle open — on Windows the deletion
//! becomes delete-pending and the new worker's create() fails with
//! ACCESS_DENIED, turning "restart download" into a near-certain
//! Access-is-denied failure seconds later.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri_app_lib::{cancel_and_drain_control, DownloadControl};
use tokio_util::sync::CancellationToken;

/// A worker that holds the temp file open and writes until cancelled — the
/// shape of a real download worker during its file I/O.
async fn slow_writer(path: std::path::PathBuf, cancel: CancellationToken, exited: Arc<AtomicBool>) {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path);
    let mut file = match file {
        Ok(file) => file,
        Err(_) => {
            exited.store(true, Ordering::SeqCst);
            return;
        }
    };
    while !cancel.is_cancelled() {
        let _ = file.write_all(&[0u8]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The handle must be closed BEFORE the exited flag flips: quiesce
    // returning with the flag set proves the file is no longer held.
    drop(file);
    exited.store(true, Ordering::SeqCst);
}

fn control_for(cancel: &CancellationToken, handle: tokio::task::JoinHandle<()>) -> DownloadControl {
    DownloadControl {
        cancel_token: cancel.clone(),
        finish: Arc::new(AtomicBool::new(false)),
        handle: Some(handle),
        source_key: "quiesce-host".to_string(),
        connection_slots: 1,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc45_quiesce_joins_worker_before_temp_removal() {
    let dir = std::env::temp_dir().join(format!("vibe-arc45-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("task.tmp");

    let cancel = CancellationToken::new();
    let exited = Arc::new(AtomicBool::new(false));
    let worker = {
        let cancel = cancel.clone();
        let exited = exited.clone();
        let path = path.clone();
        tokio::spawn(slow_writer(path, cancel, exited))
    };

    // Give the writer a moment to open the file.
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(path.exists(), "writer should have created the temp file");

    cancel_and_drain_control(control_for(&cancel, worker), Duration::from_secs(5)).await;

    assert!(
        exited.load(Ordering::SeqCst),
        "the worker must have fully exited before quiesce returns"
    );
    std::fs::remove_file(&path).expect("temp removal after drain");
    // The ARC-45 symptom check: the new worker's create() must succeed.
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .expect("new worker must be able to create the temp file");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc45_quiesce_aborts_stubborn_worker_within_bounds() {
    // A worker that never observes cancellation still holds the temp file
    // while it runs. Phase 2 (abort + join) must bound the drain AND close
    // the handle so the new worker can create the same path.
    let dir = std::env::temp_dir().join(format!("vibe-arc45-stubborn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("stubborn.tmp");

    let cancel = CancellationToken::new();
    let worker = {
        let path = path.clone();
        tokio::spawn(async move {
            let _file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path);
            // Ignores cancellation entirely — simulates work that cannot be
            // interrupted cooperatively (e.g. a spawn_blocking section).
            tokio::time::sleep(Duration::from_secs(30)).await;
        })
    };

    let started = Instant::now();
    cancel_and_drain_control(control_for(&cancel, worker), Duration::from_millis(200)).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "abort phase must bound the drain instead of hanging on the worker"
    );
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .expect("new worker must be able to create the temp file after abort");

    std::fs::remove_dir_all(&dir).ok();
}
