//! PERF-17: engine termination must release the per-task refill timer.
#![cfg(debug_assertions)]

use std::{sync::atomic::Ordering, time::Duration};

use tauri_app_lib::download::GlobalSpeedLimiter;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_enable_disable_does_not_strand_waiters_or_tickers() {
    let limiter = GlobalSpeedLimiter::with_parent(GlobalSpeedLimiter::disabled(), Some(1));
    let count = limiter.ticker_count_for_test();
    let weak = std::sync::Arc::downgrade(&limiter);
    let mut handles = Vec::new();
    for _ in 0..8 {
        let limiter = limiter.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..100 {
                limiter.set_limit(Some(1)).await;
                limiter
                    .throttle(1, &CancellationToken::new())
                    .await
                    .unwrap();
                limiter.set_limit(None).await;
                tokio::task::yield_now().await;
            }
        }));
    }
    tokio::time::timeout(Duration::from_secs(15), async {
        for handle in handles {
            handle.await.unwrap();
        }
    })
    .await
    .expect("enable/disable race must converge");
    drop(limiter);
    tokio::time::timeout(Duration::from_millis(100), async {
        while weak.upgrade().is_some() || count.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("ticker returns to baseline");
}

#[tokio::test]
async fn completed_paused_and_failed_runs_release_every_ticker() {
    for outcome in ["completed", "paused", "failed", "aborted"] {
        let parent = GlobalSpeedLimiter::disabled();
        for _ in 0..100 {
            let limiter = GlobalSpeedLimiter::with_parent(parent.clone(), Some(1_000));
            let weak = std::sync::Arc::downgrade(&limiter);
            let count = limiter.ticker_count_for_test();
            let cancel = CancellationToken::new();
            limiter.throttle(1, &cancel).await.expect("start ticker");
            assert_eq!(count.load(Ordering::SeqCst), 1);
            match outcome {
                "paused" => {
                    let waiter = tokio::spawn({
                        let limiter = limiter.clone();
                        let cancel = cancel.clone();
                        async move { limiter.throttle(1_000_000, &cancel).await }
                    });
                    cancel.cancel();
                    assert!(waiter.await.expect("waiter exits").is_err());
                }
                "failed" => {
                    let owner = limiter.clone();
                    let engine = tokio::spawn(async move {
                        let _owner = owner;
                        Err::<(), _>("transfer failed")
                    });
                    assert!(engine.await.expect("engine exits").is_err());
                }
                "aborted" => {
                    let owner = limiter.clone();
                    let engine = tokio::spawn(async move {
                        let _owner = owner;
                        std::future::pending::<()>().await;
                    });
                    engine.abort();
                    assert!(engine.await.expect_err("aborted").is_cancelled());
                }
                "completed" => {}
                _ => unreachable!(),
            }
            // The waiter path has joined, so releasing this final owner models
            // the scheduler's completed/paused/failed convergence boundary.
            drop(limiter);
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(weak.upgrade().is_none(), "{outcome} owner leaked");
            assert_eq!(count.load(Ordering::SeqCst), 0, "{outcome} ticker leaked");
        }
    }
}
