//! ARC-45 / ARC-62: restart and deletion retain ownership until the writer's
//! file handle closes; a stop deadline does not authorize aborting ownership.

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
        finish_notify: Arc::new(tokio::sync::Notify::new()),
        speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
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

    cancel_and_drain_control(&mut control_for(&cancel, worker), Duration::from_secs(5))
        .await
        .expect("drained");

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
async fn arc62_quiesce_retains_stubborn_worker_on_timeout() {
    // ARC-62: an incomplete stop must remain owned, so the caller cannot
    // treat a deadline as permission to delete or reopen the file.
    let dir = std::env::temp_dir().join(format!("vibe-arc45-stubborn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("stubborn.tmp");

    let cancel = CancellationToken::new();
    let release = Arc::new(tokio::sync::Notify::new());
    let worker_release = release.clone();
    let worker = {
        let path = path.clone();
        tokio::spawn(async move {
            let _file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path);
            // Deterministic release models a writer that cannot stop immediately.
            worker_release.notified().await;
        })
    };

    let started = Instant::now();
    let mut control = control_for(&cancel, worker);
    let error = cancel_and_drain_control(&mut control, Duration::from_millis(200))
        .await
        .expect_err("not yet drained");
    assert!(error.contains("task_stop_pending"));
    assert!(!control.handle.as_ref().unwrap().is_finished());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the user action must remain bounded while preserving worker ownership"
    );
    release.notify_one();
    cancel_and_drain_control(&mut control, Duration::from_secs(2))
        .await
        .expect("second stop drains");
    assert!(control.handle.is_none());

    std::fs::remove_dir_all(&dir).ok();
}
