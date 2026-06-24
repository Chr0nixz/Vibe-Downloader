use std::{
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Returns the current time as milliseconds since the Unix epoch.
/// Used for atomic timestamp storage without requiring a Mutex.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
    /// Last refill timestamp in milliseconds since Unix epoch.
    last_refill_millis: AtomicU64,
    parent: Option<Arc<GlobalSpeedLimiter>>,
}

impl GlobalSpeedLimiter {
    pub fn new(limit_bps: Option<i64>) -> Self {
        let limit = limit_bps.unwrap_or(0).max(0);
        Self {
            limit_bps: AtomicI64::new(limit),
            tokens_milli: AtomicI64::new(limit * 1000),
            last_refill_millis: AtomicU64::new(now_millis()),
            parent: None,
        }
    }

    pub fn disabled() -> Arc<Self> {
        Arc::new(Self::new(None))
    }

    pub fn with_parent(parent: Arc<Self>, limit_bps: Option<i64>) -> Arc<Self> {
        match limit_bps {
            Some(limit) if limit > 0 => Arc::new(Self {
                limit_bps: AtomicI64::new(limit),
                tokens_milli: AtomicI64::new(limit * 1000),
                last_refill_millis: AtomicU64::new(now_millis()),
                parent: Some(parent),
            }),
            _ => parent,
        }
    }

    pub async fn set_limit(&self, limit_bps: Option<i64>) {
        let limit = limit_bps.unwrap_or(0).max(0);
        self.limit_bps.store(limit, Ordering::Relaxed);
        self.tokens_milli.store(limit * 1000, Ordering::Relaxed);
        self.last_refill_millis.store(now_millis(), Ordering::Relaxed);
    }

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

    pub async fn throttle(&self, bytes: usize) {
        if let Some(parent) = &self.parent {
            self.throttle_self(bytes).await;
            parent.throttle_self(bytes).await;
        } else {
            self.throttle_self(bytes).await;
        }
    }

    async fn throttle_self(&self, bytes: usize) {
        let mut remaining = bytes as i64;
        let mut spin_count = 0u32;

        while remaining > 0 {
            let limit = self.limit_bps.load(Ordering::Relaxed);
            if limit <= 0 {
                return;
            }

            // Refill: only one thread succeeds the CAS and performs the refill.
            let now = now_millis();
            let last = self.last_refill_millis.load(Ordering::Relaxed);
            if now > last {
                if self
                    .last_refill_millis
                    .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    let elapsed_millis = (now - last) as i64;
                    // Refill in milli-bytes: limit (bytes/sec) × elapsed_ms = milli-bytes
                    let refill_milli = elapsed_millis.saturating_mul(limit);
                    let current = self.tokens_milli.load(Ordering::Relaxed);
                    let max_milli = limit * 1000;
                    let new_tokens = (current + refill_milli).min(max_milli);
                    self.tokens_milli.store(new_tokens, Ordering::Relaxed);
                }
            }

            // Consume: try to take tokens via CAS.
            let request_bytes = remaining.min(limit);
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
                    continue;
                }
                // CAS failed — another thread raced us, retry.
            } else {
                // Not enough tokens — sleep until we have enough.
                let deficit_milli = request_milli - current;
                // wait_ms = deficit_milli / limit (milli-bytes / bytes-per-sec = ms)
                let wait_ms = ((deficit_milli as f64) / (limit as f64) as u64)
                    .max(1)
                    .min(250);
                tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                spin_count = 0;
                continue;
            }

            // CAS contention backoff: spin a few times, then yield.
            spin_count += 1;
            if spin_count >= 4 {
                tokio::time::sleep(Duration::from_millis(1)).await;
                spin_count = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_throttle_no_limit() {
        let limiter = GlobalSpeedLimiter::new(None);
        // With no limit (0), throttle should return immediately.
        limiter.throttle(1_000_000).await;
    }

    #[tokio::test]
    async fn test_throttle_with_limit() {
        let limiter = GlobalSpeedLimiter::new(Some(1_000_000)); // 1 MB/s
        // Should be able to consume up to the limit immediately (tokens pre-filled).
        limiter.throttle(500_000).await;
        // Remaining tokens should be ~500_000.
        let tokens = limiter.tokens_milli.load(Ordering::Relaxed);
        assert!(tokens < 1_000_000_000, "tokens should have been consumed");
    }

    #[tokio::test]
    async fn test_set_limit_resets_tokens() {
        let limiter = GlobalSpeedLimiter::new(Some(1_000_000));
        // Consume some tokens.
        limiter.throttle(500_000).await;
        // Change limit — should reset tokens to the new full bucket.
        limiter.set_limit(Some(2_000_000)).await;
        let tokens = limiter.tokens_milli.load(Ordering::Relaxed);
        assert_eq!(tokens, 2_000_000_000, "tokens should be reset to new limit");
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
}
