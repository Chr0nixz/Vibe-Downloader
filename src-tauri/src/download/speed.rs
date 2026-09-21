use std::{
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Process-monotonic millisecond clock (PERF-08).
/// Anchored once so wall-clock jumps cannot stall token refill.
fn mono_anchor() -> Instant {
    static ANCHOR: OnceLock<Instant> = OnceLock::new();
    *ANCHOR.get_or_init(Instant::now)
}

fn now_millis() -> u64 {
    mono_anchor().elapsed().as_millis() as u64
}

fn bucket_capacity_milli(limit_bps: i64) -> i64 {
    limit_bps.saturating_mul(1000)
}

fn refill_tokens(tokens: &AtomicI64, refill_milli: i64, max_milli: i64) {
    // Relaxed ordering is sufficient because the balance does not publish any
    // other memory; fetch_update is required only to preserve numeric atomicity.
    let _ = tokens.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(refill_milli).min(max_milli))
    });
}

/// PERF-08: centralized refill cadence shared by all waiters on this limiter.
const TICK_INTERVAL_MS: u64 = 25;

/// Lock-free token bucket speed limiter.
///
/// Uses atomic operations instead of a Mutex to avoid lock contention
/// when many concurrent downloads throttle simultaneously. Token precision
/// is in milli-bytes (bytes × 1000) to avoid f64 atomics.
#[derive(Debug)]
pub struct GlobalSpeedLimiter {
    limit_bps: AtomicI64,
    /// Token balance in milli-bytes (bytes × 1000) for sub-integer precision.
    tokens_milli: AtomicI64,
    /// Last refill timestamp in process-monotonic milliseconds.
    last_refill_millis: AtomicU64,
    parent: Option<Arc<GlobalSpeedLimiter>>,
    /// Shared wake signal when the ticker refills tokens.
    notify: Notify,
    /// Holds the JoinHandle so Drop / set_limit(0) can abort the ticker.
    ticker_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    #[cfg(debug_assertions)]
    ticker_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl GlobalSpeedLimiter {
    pub fn new(limit_bps: Option<i64>) -> Self {
        let limit = limit_bps.unwrap_or(0).max(0);
        Self {
            limit_bps: AtomicI64::new(limit),
            tokens_milli: AtomicI64::new(bucket_capacity_milli(limit)),
            last_refill_millis: AtomicU64::new(now_millis()),
            parent: None,
            notify: Notify::new(),
            ticker_handle: Mutex::new(None),
            #[cfg(debug_assertions)]
            ticker_count: Arc::default(),
        }
    }

    pub fn disabled() -> Arc<Self> {
        Arc::new(Self::new(None))
    }

    pub fn with_parent(parent: Arc<Self>, limit_bps: Option<i64>) -> Arc<Self> {
        match limit_bps {
            Some(limit) if limit > 0 => Arc::new(Self {
                limit_bps: AtomicI64::new(limit),
                tokens_milli: AtomicI64::new(bucket_capacity_milli(limit)),
                last_refill_millis: AtomicU64::new(now_millis()),
                parent: Some(parent),
                notify: Notify::new(),
                ticker_handle: Mutex::new(None),
                #[cfg(debug_assertions)]
                ticker_count: Arc::default(),
            }),
            _ => parent,
        }
    }

    pub async fn set_limit(&self, limit_bps: Option<i64>) {
        // Serialize disable/re-enable with ticker installation so an old stop
        // cannot abort a new ticker or leave a detached handle behind.
        let mut ticker = self.ticker_handle.lock().unwrap_or_else(|e| e.into_inner());
        let limit = limit_bps.unwrap_or(0).max(0);
        self.limit_bps.store(limit, Ordering::Relaxed);
        self.tokens_milli
            .store(bucket_capacity_milli(limit), Ordering::Relaxed);
        self.last_refill_millis
            .store(now_millis(), Ordering::Relaxed);
        if limit <= 0 {
            if let Some(handle) = ticker.take() {
                handle.abort();
            }
        }
        self.notify.notify_waiters();
    }

    fn stop_ticker(&self) {
        let mut guard = self.ticker_handle.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }

    /// Lazy-start a shared ticker so waiters wake together on refill (PERF-08).
    fn ensure_ticker(self: &Arc<Self>) {
        let mut guard = self.ticker_handle.lock().unwrap_or_else(|e| e.into_inner());
        if self.limit_bps.load(Ordering::Relaxed) <= 0
            || guard.as_ref().is_some_and(|handle| !handle.is_finished())
        {
            return;
        }
        // PERF-17: the ticker must not keep its owner alive while sleeping.
        // Drop aborts it when the last engine/waiter releases the limiter.
        let owner = Arc::downgrade(self);
        #[cfg(debug_assertions)]
        let ticker_count = TickerCount::new(self.ticker_count.clone());
        let handle = tokio::spawn(async move {
            #[cfg(debug_assertions)]
            let _ticker_count = ticker_count;
            let mut interval = tokio::time::interval(Duration::from_millis(TICK_INTERVAL_MS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(this) = owner.upgrade() else {
                    break;
                };
                if this.limit_bps.load(Ordering::Relaxed) <= 0 {
                    break;
                }
                this.refill_now();
                this.notify.notify_waiters();
            }
        });
        *guard = Some(handle);
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn ticker_count_for_test(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        self.ticker_count.clone()
    }

    fn refill_now(&self) {
        let limit = self.limit_bps.load(Ordering::Relaxed);
        if limit <= 0 {
            return;
        }
        let now = now_millis();
        let last = self.last_refill_millis.load(Ordering::Relaxed);
        if now > last
            && self
                .last_refill_millis
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let elapsed_millis = (now - last) as i64;
            let refill_milli = elapsed_millis.saturating_mul(limit);
            refill_tokens(
                &self.tokens_milli,
                refill_milli,
                bucket_capacity_milli(limit),
            );
        }
    }

    /// Hierarchical limiting: each byte is charged to BOTH the per-task child and the global
    /// parent. The effective limit is the minimum of the two (stricter wins). This lets the
    /// global limiter cap aggregate throughput while per-task limiters enforce individual caps.
    pub fn current_limit_bps(&self) -> Option<i64> {
        let own = match self.limit_bps.load(Ordering::SeqCst) {
            value if value > 0 => Some(value),
            _ => None,
        };
        match (
            own,
            self.parent
                .as_ref()
                .and_then(|parent| parent.current_limit_bps()),
        ) {
            (Some(own), Some(parent)) => Some(own.min(parent)),
            (Some(own), None) => Some(own),
            (None, Some(parent)) => Some(parent),
            (None, None) => None,
        }
    }

    /// Acquire `bytes` permits from the token bucket.
    ///
    /// ARC-04: waits are cancellable so pause/delete/exit can converge under
    /// very low limits (e.g. 1 B/s) instead of sleeping for hours.
    pub async fn throttle(
        self: &Arc<Self>,
        bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<(), ()> {
        if let Some(parent) = &self.parent {
            self.throttle_self(bytes, cancel).await?;
            parent.throttle_self(bytes, cancel).await?;
        } else {
            self.throttle_self(bytes, cancel).await?;
        }
        Ok(())
    }

    async fn throttle_self(
        self: &Arc<Self>,
        bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<(), ()> {
        let mut remaining = bytes as i64;
        let mut spin_count = 0u32;

        while remaining > 0 {
            if cancel.is_cancelled() {
                return Err(());
            }

            let limit = self.limit_bps.load(Ordering::Relaxed);
            if limit <= 0 {
                return Ok(());
            }

            self.ensure_ticker();
            self.refill_now();

            // Cap each CAS take to ~one tick of bandwidth so concurrent waiters
            // interleave instead of one connection draining the whole bucket.
            let fair_quantum =
                ((limit as u64).saturating_mul(TICK_INTERVAL_MS) / 1000).max(1) as i64;
            let request_bytes = remaining.min(limit).min(fair_quantum);
            let request_milli = request_bytes * 1000;
            let current = self.tokens_milli.load(Ordering::Relaxed);

            if current >= request_milli {
                if self
                    .tokens_milli
                    .compare_exchange(
                        current,
                        current - request_milli,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    remaining -= request_bytes;
                    spin_count = 0;
                    // Always yield after a take so sibling waiters can interleave
                    // even when each throttle() call fits in one quantum.
                    self.notify.notify_waiters();
                    tokio::task::yield_now().await;
                    continue;
                }
                // CAS failed — another thread raced us, retry.
            } else {
                // PERF-08: wait for shared ticker notify instead of a private long sleep.
                let notified = self.notify.notified();
                tokio::pin!(notified);
                // Register before checking cancel/tokens again to avoid missed wakeups.
                notified.as_mut().enable();
                // Disabling the limiter may abort the only ticker between the
                // first balance check and waiter registration. Recheck after
                // registering so that transition cannot leave a waiter asleep.
                if self.limit_bps.load(Ordering::Relaxed) <= 0
                    || self.tokens_milli.load(Ordering::Relaxed) >= request_milli
                {
                    continue;
                }
                tokio::select! {
                    _ = cancel.cancelled() => return Err(()),
                    _ = notified => {}
                }
                spin_count = 0;
                continue;
            }

            spin_count += 1;
            if spin_count >= 4 {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(()),
                    _ = tokio::task::yield_now() => {}
                }
                spin_count = 0;
            }
        }
        Ok(())
    }
}

impl Drop for GlobalSpeedLimiter {
    fn drop(&mut self) {
        self.stop_ticker();
    }
}

#[cfg(debug_assertions)]
struct TickerCount(Arc<std::sync::atomic::AtomicUsize>);

#[cfg(debug_assertions)]
impl TickerCount {
    fn new(count: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count)
    }
}

#[cfg(debug_assertions)]
impl Drop for TickerCount {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_throttle_no_limit() {
        let limiter = Arc::new(GlobalSpeedLimiter::new(None));
        let cancel = CancellationToken::new();
        // With no limit (0), throttle should return immediately.
        limiter
            .throttle(1_000_000, &cancel)
            .await
            .expect("unlimited throttle");
    }

    #[tokio::test]
    async fn test_throttle_with_limit() {
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(1_000_000))); // 1 MB/s
        let cancel = CancellationToken::new();
        // Should be able to consume up to the limit immediately (tokens pre-filled).
        limiter
            .throttle(500_000, &cancel)
            .await
            .expect("limited throttle");
        // Remaining tokens should be ~500_000.
        let tokens = limiter.tokens_milli.load(Ordering::Relaxed);
        assert!(tokens < 1_000_000_000, "tokens should have been consumed");
    }

    #[tokio::test]
    async fn test_set_limit_resets_tokens() {
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(1_000_000)));
        let cancel = CancellationToken::new();
        // Consume some tokens.
        limiter
            .throttle(500_000, &cancel)
            .await
            .expect("consume tokens");
        // Change limit — should reset tokens to the new full bucket.
        limiter.set_limit(Some(2_000_000)).await;
        let tokens = limiter.tokens_milli.load(Ordering::Relaxed);
        assert_eq!(tokens, 2_000_000_000, "tokens should be reset to new limit");
    }

    #[tokio::test]
    async fn throttle_cancels_during_low_rate_wait() {
        // ARC-04: 1 B/s wait must exit promptly when cancelled.
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(1)));
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(async move { limiter.throttle(10_000, &cancel_clone).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("throttle cancel must converge quickly")
            .expect("join");
        assert!(result.is_err(), "throttle should report cancellation");
    }

    #[tokio::test]
    async fn test_current_limit_bps() {
        let limiter = GlobalSpeedLimiter::new(Some(1_000_000));
        assert_eq!(limiter.current_limit_bps(), Some(1_000_000));

        let unlimited = GlobalSpeedLimiter::new(None);
        assert_eq!(unlimited.current_limit_bps(), None);

        let parent = Arc::new(GlobalSpeedLimiter::new(Some(2_000_000)));
        let child = GlobalSpeedLimiter::with_parent(parent, Some(1_000_000));
        // Should return the minimum of parent and child.
        assert_eq!(child.current_limit_bps(), Some(1_000_000));
    }

    #[test]
    fn concurrent_refill_does_not_overwrite_token_consumption() {
        let tokens = Arc::new(AtomicI64::new(1_000_000));
        let mut workers = Vec::new();
        for _ in 0..4 {
            let tokens = tokens.clone();
            workers.push(std::thread::spawn(move || {
                for _ in 0..10_000 {
                    tokens.fetch_sub(1, Ordering::Relaxed);
                }
            }));
        }
        let refill_tokens_ref = tokens.clone();
        workers.push(std::thread::spawn(move || {
            for _ in 0..10_000 {
                refill_tokens(&refill_tokens_ref, 4, 2_000_000);
            }
        }));
        for worker in workers {
            worker.join().expect("speed limiter worker");
        }
        assert_eq!(tokens.load(Ordering::Relaxed), 1_000_000);
    }

    #[tokio::test]
    async fn refill_uses_monotonic_clock_not_wall_time() {
        // PERF-08: even if SystemTime were to jump backward, Instant-based
        // now_millis still advances and refill continues.
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(10_000)));
        let cancel = CancellationToken::new();
        limiter
            .throttle(10_000, &cancel)
            .await
            .expect("drain bucket");
        let before = limiter.tokens_milli.load(Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(80)).await;
        limiter.refill_now();
        let after = limiter.tokens_milli.load(Ordering::Relaxed);
        assert!(
            after > before,
            "monotonic refill should add tokens after sleep (before={before}, after={after})"
        );
    }

    #[tokio::test]
    async fn multi_waiter_throughput_stays_within_fairness_band() {
        // PERF-08: shared Notify should keep multi-waiter byte counts close.
        let limiter = Arc::new(GlobalSpeedLimiter::new(Some(50_000))); // 50 KB/s
        let cancel = CancellationToken::new();
        let mut handles = Vec::new();
        let counts = Arc::new(AtomicU64::new(0));
        let per_waiter = Arc::new([
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
        ]);

        for i in 0..4 {
            let limiter = Arc::clone(&limiter);
            let cancel = cancel.clone();
            let counts = Arc::clone(&counts);
            let per_waiter = Arc::clone(&per_waiter);
            handles.push(tokio::spawn(async move {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(400);
                while tokio::time::Instant::now() < deadline {
                    if limiter.throttle(1_000, &cancel).await.is_err() {
                        break;
                    }
                    per_waiter[i].fetch_add(1_000, Ordering::Relaxed);
                    counts.fetch_add(1_000, Ordering::Relaxed);
                }
            }));
        }
        for handle in handles {
            handle.await.expect("waiter join");
        }
        let mut values = [0u64; 4];
        for (i, slot) in per_waiter.iter().enumerate() {
            values[i] = slot.load(Ordering::Relaxed);
        }
        let min = *values.iter().min().unwrap();
        let max = *values.iter().max().unwrap();
        assert!(
            min > 0,
            "each waiter should acquire some tokens: {values:?}"
        );
        // Allow 2.5× spread under CAS contention; documents fairness tolerance.
        assert!(
            max <= ((min as f64) * 2.5) as u64 + 4_000,
            "waiter throughput variance too high: {values:?}"
        );
    }
}
