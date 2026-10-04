//! ARC-63: app exit and restart wait for task owners without aborting them.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tauri_app_lib::{
    drain_download_owners, ActiveSupervisorGuard, AppLifecycle, AppLifecyclePhase, DownloadControl,
    ExitClaim,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_waits_for_owner_and_supervisor_tail() {
    let downloads = Arc::new(Mutex::new(HashMap::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let cancel = CancellationToken::new();
    let task_id = "cooperative".to_string();
    let worker_downloads = downloads.clone();
    let worker_active = active.clone();
    let worker_cancel = cancel.clone();
    let handle = tokio::spawn(async move {
        let _owner = ActiveSupervisorGuard::new(worker_active);
        worker_cancel.cancelled().await;
        worker_downloads.lock().await.remove(&task_id);
        tokio::time::sleep(Duration::from_millis(80)).await;
    });
    downloads
        .lock()
        .await
        .insert("cooperative".to_string(), control(cancel, Some(handle)));

    let started = Instant::now();
    drain_download_owners(&downloads, &active, Some(Duration::from_secs(2)))
        .await
        .expect("cooperative owner should drain");

    assert!(started.elapsed() >= Duration::from_millis(70));
    assert!(downloads.lock().await.is_empty());
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_keeps_stubborn_owner_registered_and_retry_can_finish() {
    let downloads = Arc::new(Mutex::new(HashMap::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let cancel = CancellationToken::new();
    let task_id = "stubborn".to_string();
    let worker_downloads = downloads.clone();
    let worker_active = active.clone();
    let worker_cancel = cancel.clone();
    let (release, wait_for_release) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        let _owner = ActiveSupervisorGuard::new(worker_active);
        worker_cancel.cancelled().await;
        let _ = wait_for_release.await;
        worker_downloads.lock().await.remove(&task_id);
    });
    downloads.lock().await.insert(
        "stubborn".to_string(),
        control(cancel.clone(), Some(handle)),
    );

    let error = drain_download_owners(&downloads, &active, Some(Duration::from_millis(80)))
        .await
        .expect_err("a writer beyond the budget must block restart and exit");
    assert!(error.contains("task_stop_pending"));
    assert!(downloads.lock().await.contains_key("stubborn"));
    assert!(cancel.is_cancelled());
    assert_eq!(active.load(Ordering::SeqCst), 1);

    release.send(()).expect("release owner");
    drain_download_owners(&downloads, &active, Some(Duration::from_secs(2)))
        .await
        .expect("retry after the owner exits should succeed");
    assert!(downloads.lock().await.is_empty());
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn an_exit_request_supersedes_a_prepared_restart_without_reopening_the_app() {
    let lifecycle = AppLifecycle::default();
    assert!(lifecycle.transition(
        AppLifecyclePhase::Running,
        AppLifecyclePhase::RestartDraining
    ));
    assert!(!lifecycle.transition(
        AppLifecyclePhase::Running,
        AppLifecyclePhase::RestartDraining
    ));
    assert_eq!(lifecycle.claim_exit(), ExitClaim::StartDrain);
    assert!(!lifecycle.transition(
        AppLifecyclePhase::RestartDraining,
        AppLifecyclePhase::RestartReady
    ));
    assert_eq!(lifecycle.claim_exit(), ExitClaim::AlreadyDraining);
    lifecycle.set_phase(AppLifecyclePhase::ExitReady);
    assert_eq!(lifecycle.claim_exit(), ExitClaim::ExitNow);
}

#[test]
fn concurrent_exit_requests_start_only_one_drain() {
    let lifecycle = Arc::new(AppLifecycle::default());
    let callers = (0..16)
        .map(|_| {
            let lifecycle = lifecycle.clone();
            std::thread::spawn(move || lifecycle.claim_exit())
        })
        .collect::<Vec<_>>();
    let claims = callers
        .into_iter()
        .map(|caller| caller.join().expect("exit claimant thread"))
        .collect::<Vec<_>>();

    assert_eq!(
        claims
            .iter()
            .filter(|claim| **claim == ExitClaim::StartDrain)
            .count(),
        1
    );
    assert_eq!(lifecycle.phase(), AppLifecyclePhase::ExitDraining);
}

fn control(
    cancel_token: CancellationToken,
    handle: Option<tokio::task::JoinHandle<()>>,
) -> DownloadControl {
    DownloadControl {
        cancel_token,
        finish: Arc::new(AtomicBool::new(false)),
        finish_notify: Arc::new(tokio::sync::Notify::new()),
        speed_limiter: tauri_app_lib::download::GlobalSpeedLimiter::disabled(),
        handle,
        source_key: "shutdown-test".to_string(),
        connection_slots: 1,
    }
}
