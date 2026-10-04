//! ARC-62: a canceled child and its submitted blocking write retain ownership until I/O ends.

use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    sync::{atomic::AtomicBool, mpsc, Arc},
    time::Duration,
};

use tauri_app_lib::{
    download::{
        lifecycle::{self, JoinSet},
        owned_fs,
    },
    remove_and_drain_control, DownloadControl,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn control(cancel: CancellationToken, handle: tokio::task::JoinHandle<()>) -> DownloadControl {
    DownloadControl {
        cancel_token: cancel,
        finish: Arc::new(AtomicBool::new(false)),
        finish_notify: Arc::new(tokio::sync::Notify::new()),
        speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
        handle: Some(handle),
        source_key: "lifecycle-host".into(),
        connection_slots: 1,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_abort_does_not_release_owner_before_blocking_file_write() {
    let root = std::env::temp_dir().join(format!("arc62-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("late-write.bin");
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let write_path = path.clone();
    let worker = tokio::spawn(lifecycle::run_owned(async move {
        let mut children = JoinSet::new();
        children.spawn(async move {
            let _ = lifecycle::blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&write_path)
                    .unwrap()
                    .write_all(b"finished")
                    .unwrap();
            })
            .await;
        });
        tokio::select! {
            _ = worker_cancel.cancelled() => children.abort_all(),
            _ = tokio::time::sleep(Duration::from_secs(30)) => {},
        }
        drop(children);
    }));
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
        .await
        .unwrap();

    let downloads = Mutex::new(HashMap::from([("task".into(), control(cancel, worker))]));
    let headers = Mutex::new(HashMap::new());
    let error = remove_and_drain_control(&downloads, &headers, "task", Duration::from_millis(100))
        .await
        .unwrap_err();
    assert!(error.contains("task_stop_pending"));
    assert!(
        !path.exists(),
        "the blocking operation has not run after timeout"
    );
    assert!(downloads.lock().await.contains_key("task"));

    release_tx.send(()).unwrap();
    remove_and_drain_control(&downloads, &headers, "task", Duration::from_secs(3))
        .await
        .expect("retrying stop joins after the blocking write completes");
    assert!(!downloads.lock().await.contains_key("task"));
    assert_eq!(std::fs::read(&path).unwrap(), b"finished");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_owned_file_waits_for_its_submitted_flush() {
    use tokio::io::AsyncWriteExt;

    let root = std::env::temp_dir().join(format!("arc62-file-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("owned.bin");
    let write_path = path.clone();
    let worker = tokio::spawn(lifecycle::run_owned(async move {
        let mut file = owned_fs::File::create(&write_path).await.unwrap();
        file.write_all(b"owned").await.unwrap();
        drop(file);
    }));
    tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"owned");
    std::fs::remove_dir_all(root).unwrap();
}
