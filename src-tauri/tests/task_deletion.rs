//! ARC-59: deletion owns each task until both its writer and persistent row are gone.

mod common;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri_app_lib::{
    commands::tasks::TaskDeletion,
    db,
    download::{lifecycle, EngineRegistry},
    remove_and_drain_control, DownloadControl, TaskRuntimeLocks,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

async fn drain_before_delete(bulk: bool) {
    let (_db, pool) = common::test_pool("arc59-delete").await;
    let mut settings = db::get_settings(&pool, String::new()).await.unwrap();
    settings.delete_to_trash = false;
    db::upsert_settings(&pool, &settings).await.unwrap();
    let paths = common::TestPaths::new("arc59-delete");
    let task = common::download_task(
        "deleting",
        "https://example.com/file".into(),
        "https",
        "file.bin",
        1024,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let temp = paths.temp.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&temp).await.unwrap();
        file.write_all(b"partial").await.unwrap();
        ready_tx.send(()).unwrap();
        worker_cancel.cancelled().await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        file.flush().await.unwrap();
        assert!(temp.exists(), "cleanup must wait for the final flush");
        drop(file);
        worker_finished.store(true, Ordering::SeqCst);
    });
    ready_rx.await.unwrap();
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
    let headers = Mutex::new(HashMap::from([(
        task.id.clone(),
        vec![("Cookie".into(), "stale=1".into())],
    )]));
    let locks = TaskRuntimeLocks::default();
    let engines = EngineRegistry::new().unwrap();
    let deletion = TaskDeletion {
        pool: &pool,
        downloads: &downloads,
        request_headers: &headers,
        runtime_locks: &locks,
        engines: &engines,
        drain_grace: tauri_app_lib::USER_ACTION_DRAIN_GRACE,
    };
    let deleting = async {
        if bulk {
            assert_eq!(
                deletion
                    .delete_many(&[task.id.clone(), task.id.clone()], true)
                    .await
                    .unwrap(),
                1
            );
        } else {
            deletion.delete(&task.id, true).await.unwrap();
        }
    };
    let competing_start = async {
        cancel.cancelled().await;
        let _guard = locks.lock(&task.id).await;
        assert!(
            db::get_task_record(&pool, &task.id)
                .await
                .unwrap()
                .is_none(),
            "a racing start must not see a row after cleanup released ownership"
        );
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(deleting, competing_start);
    })
    .await
    .unwrap();
    assert!(
        finished.load(Ordering::SeqCst),
        "the writer exited cooperatively before deletion"
    );
    assert!(!paths.temp.exists());
    assert!(downloads.lock().await.is_empty());
    assert!(headers.lock().await.is_empty());
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_delete_drains_and_holds_the_lock_until_the_row_is_gone() {
    drain_before_delete(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_delete_drains_and_holds_the_lock_until_each_row_is_gone() {
    drain_before_delete(true).await;
}

#[tokio::test]
async fn deletion_refreshes_published_paths_after_the_writer_exits() {
    let (_db, pool) = common::test_pool("arc59-final-rename").await;
    let mut settings = db::get_settings(&pool, String::new()).await.unwrap();
    settings.delete_to_trash = false;
    db::upsert_settings(&pool, &settings).await.unwrap();
    let paths = common::TestPaths::new("arc59-final-rename");
    let task = common::download_task(
        "renamed",
        "https://example.com/file".into(),
        "https",
        "file.bin",
        8,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    std::fs::write(&paths.final_path, b"unrelated collision").unwrap();
    std::fs::write(&paths.temp, b"download").unwrap();
    let published_path = paths.final_path.with_file_name("file (1).bin");
    let worker_path = published_path.clone();
    let temp = paths.temp.clone();
    let worker_pool = pool.clone();
    let task_id = task.id.clone();
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(async move {
        worker_cancel.cancelled().await;
        tokio::fs::rename(temp, &worker_path).await.unwrap();
        db::update_task_final_path(
            &worker_pool,
            &task_id,
            "file (1).bin",
            &worker_path.to_string_lossy(),
        )
        .await
        .unwrap();
    });
    let downloads = Mutex::new(HashMap::from([(
        task.id.clone(),
        DownloadControl {
            cancel_token: cancel,
            finish: Arc::new(AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
            handle: Some(worker),
            source_key: task.source_key.clone(),
            connection_slots: 1,
        },
    )]));
    let headers = Mutex::new(HashMap::new());
    let locks = TaskRuntimeLocks::default();
    let engines = EngineRegistry::new().unwrap();
    TaskDeletion {
        pool: &pool,
        downloads: &downloads,
        request_headers: &headers,
        runtime_locks: &locks,
        engines: &engines,
        drain_grace: tauri_app_lib::USER_ACTION_DRAIN_GRACE,
    }
    .delete(&task.id, true)
    .await
    .unwrap();
    assert!(!published_path.exists());
    assert!(!paths.temp.exists());
    assert_eq!(
        std::fs::read(&paths.final_path).unwrap(),
        b"unrelated collision"
    );
    assert!(db::get_task_record(&pool, &task.id)
        .await
        .unwrap()
        .is_none());
    pool.close().await;
}

#[cfg(windows)]
#[tokio::test]
async fn failed_file_cleanup_retains_a_stopped_task_and_bulk_successes_stay_deleted() {
    use std::os::windows::fs::OpenOptionsExt;
    use tauri_app_lib::models::{AppErrorPayload, TaskStatus};
    let (_db, pool) = common::test_pool("arc59-locked-file").await;
    let mut settings = db::get_settings(&pool, String::new()).await.unwrap();
    settings.delete_to_trash = false;
    db::upsert_settings(&pool, &settings).await.unwrap();
    let blocked = common::TestPaths::new("arc59-blocked");
    let good = common::TestPaths::new("arc59-good");
    let mut task = common::download_task(
        "blocked",
        "https://example.com/blocked".into(),
        "https",
        "file.bin",
        1024,
        &blocked,
        true,
    );
    task.status = TaskStatus::Queued;
    db::insert_task_record(&pool, &task).await.unwrap();
    let good_task = common::download_task(
        "good",
        "https://example.com/good".into(),
        "https",
        "file.bin",
        1024,
        &good,
        true,
    );
    db::insert_task_record(&pool, &good_task).await.unwrap();
    std::fs::write(&blocked.final_path, b"locked").unwrap();
    std::fs::write(&good.temp, b"removable").unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&blocked.final_path)
        .unwrap();
    let downloads = Mutex::new(HashMap::new());
    let headers = Mutex::new(HashMap::new());
    let locks = TaskRuntimeLocks::default();
    let engines = EngineRegistry::new().unwrap();
    let deletion = TaskDeletion {
        pool: &pool,
        downloads: &downloads,
        request_headers: &headers,
        runtime_locks: &locks,
        engines: &engines,
        drain_grace: tauri_app_lib::USER_ACTION_DRAIN_GRACE,
    };
    let error = deletion
        .delete_many(&[task.id.clone(), good_task.id.clone()], true)
        .await
        .unwrap_err();
    let payload: AppErrorPayload = serde_json::from_str(&error).unwrap();
    assert_eq!(payload.code, "storage_cleanup_failed");
    assert_eq!(
        db::get_task_record(&pool, &task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Paused
    );
    assert!(blocked.final_path.exists());
    assert!(db::get_task_record(&pool, &good_task.id)
        .await
        .unwrap()
        .is_none());
    assert!(!good.temp.exists());
    drop(lock);
    deletion.delete(&task.id, true).await.unwrap();
    assert!(db::get_task_record(&pool, &task.id)
        .await
        .unwrap()
        .is_none());
    assert!(!blocked.final_path.exists());
    pool.close().await;
}

#[tokio::test]
async fn metadata_only_delete_preserves_the_downloaded_file() {
    let (_db, pool) = common::test_pool("arc59-metadata").await;
    let paths = common::TestPaths::new("arc59-metadata");
    let task = common::download_task(
        "keep",
        "https://example.com/keep".into(),
        "https",
        "file.bin",
        8,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    std::fs::write(&paths.final_path, b"preserve").unwrap();
    let downloads = Mutex::new(HashMap::new());
    let headers = Mutex::new(HashMap::new());
    let locks = TaskRuntimeLocks::default();
    let engines = EngineRegistry::new().unwrap();
    TaskDeletion {
        pool: &pool,
        downloads: &downloads,
        request_headers: &headers,
        runtime_locks: &locks,
        engines: &engines,
        drain_grace: tauri_app_lib::USER_ACTION_DRAIN_GRACE,
    }
    .delete(&task.id, false)
    .await
    .unwrap();
    assert_eq!(std::fs::read(&paths.final_path).unwrap(), b"preserve");
    assert!(db::get_task_record(&pool, &task.id)
        .await
        .unwrap()
        .is_none());
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_refuses_to_remove_a_file_while_blocking_writer_is_unresolved() {
    use std::{fs::OpenOptions, io::Write, sync::mpsc, time::Duration};
    use tauri_app_lib::download::lifecycle::JoinSet;
    use tokio_util::sync::CancellationToken;

    let (_db, pool) = common::test_pool("arc62-delete-pending").await;
    let mut settings = db::get_settings(&pool, String::new()).await.unwrap();
    settings.delete_to_trash = false;
    db::upsert_settings(&pool, &settings).await.unwrap();
    let paths = common::TestPaths::new("arc62-delete-pending");
    let task = common::download_task(
        "pending-writer",
        "https://example.com/file".into(),
        "https",
        "file.bin",
        1024,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task).await.unwrap();
    std::fs::write(&paths.temp, b"before").unwrap();

    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let write_path = paths.temp.clone();
    let worker = tokio::spawn(lifecycle::run_owned(async move {
        let mut children = JoinSet::new();
        children.spawn(async move {
            let _ = lifecycle::blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                OpenOptions::new()
                    .append(true)
                    .open(write_path)
                    .unwrap()
                    .write_all(b"-late")
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

    let downloads = Mutex::new(HashMap::from([(
        task.id.clone(),
        DownloadControl {
            cancel_token: cancel,
            finish: Arc::new(AtomicBool::new(false)),
            finish_notify: Arc::new(tokio::sync::Notify::new()),
            speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
            handle: Some(worker),
            source_key: task.source_key.clone(),
            connection_slots: 1,
        },
    )]));
    let headers = Mutex::new(HashMap::new());
    let locks = TaskRuntimeLocks::default();
    let engines = EngineRegistry::new().unwrap();
    let deletion = TaskDeletion {
        pool: &pool,
        downloads: &downloads,
        request_headers: &headers,
        runtime_locks: &locks,
        engines: &engines,
        drain_grace: Duration::from_millis(100),
    };
    let error = deletion.delete(&task.id, true).await.unwrap_err();
    assert!(error.contains("task_stop_pending"));
    assert_eq!(std::fs::read(&paths.temp).unwrap(), b"before");
    assert!(db::get_task_record(&pool, &task.id)
        .await
        .unwrap()
        .is_some());
    assert!(downloads.lock().await.contains_key(&task.id));

    release_tx.send(()).unwrap();
    remove_and_drain_control(&downloads, &headers, &task.id, Duration::from_secs(3))
        .await
        .expect("writer exits before retrying deletion");
    assert_eq!(std::fs::read(&paths.temp).unwrap(), b"before-late");
    deletion
        .delete(&task.id, true)
        .await
        .expect("retry delete drains and releases ownership");
    assert!(!paths.temp.exists());
    assert!(db::get_task_record(&pool, &task.id)
        .await
        .unwrap()
        .is_none());
    pool.close().await;
}
