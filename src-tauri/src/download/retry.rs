//! Shared retry utility for download engines.
//!
//! Provides a configurable [`RetryPolicy`] with multiple backoff strategies,
//! used by HTTP, SFTP, and other engines for transient-error retries.
//! Segment-level retries in FTP and HLS remain in their respective modules
//! because they are tightly coupled to segment management and progress tracking.

use std::time::Duration;

/// Backoff delay strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backoff {
    /// Same delay between every attempt.
    Fixed,
    /// Delay increases linearly: `base * attempt_number` (1-indexed).
    #[allow(dead_code)]
    Linear,
    /// Delay doubles each attempt: `base * 2^(attempt_number - 1)`.
    Exponential,
}

/// Configurable retry policy.
#[derive(Debug, Clone)]
pub(crate) struct RetryPolicy {
    /// Total number of attempts (including the first). Minimum 1.
    pub max_attempts: u32,
    /// Base delay between retries.
    pub base_delay: Duration,
    /// Maximum delay cap (0 = unlimited).
    pub max_delay: Duration,
    /// Backoff strategy.
    pub backoff: Backoff,
}

impl RetryPolicy {
    /// HTTP request-level retry: 3 attempts, 25ms fixed delay.
    pub fn http_request() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(25),
            max_delay: Duration::from_secs(0),
            backoff: Backoff::Fixed,
        }
    }

    /// SFTP connection retry: 3 attempts, 500ms exponential backoff, 5s cap.
    pub fn sftp_connect() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            backoff: Backoff::Exponential,
        }
    }

    /// FTP segment worker retry: 3 attempts, 500ms exponential backoff, 5s cap.
    /// Aligns with SFTP connection retry for consistent transient-error handling.
    pub fn ftp_worker() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            backoff: Backoff::Exponential,
        }
    }

    /// HLS segment retry: 3 attempts, 500ms exponential backoff.
    /// Upgrades the previous 2-attempt/200ms-linear strategy for better
    /// transient-error recovery while keeping the attempt count conservative.
    pub fn hls_segment() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            backoff: Backoff::Exponential,
        }
    }

    /// Metalink per-mirror retry: 2 attempts, 1s fixed delay.
    /// A single mirror gets one retry before failover to the next priority mirror.
    pub fn metalink_mirror() -> Self {
        Self {
            max_attempts: 2,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(0),
            backoff: Backoff::Fixed,
        }
    }

    /// Compute the delay for a given 1-indexed attempt number.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let base_ms = self.base_delay.as_millis() as u64;
        let raw_ms = match self.backoff {
            Backoff::Fixed => base_ms,
            Backoff::Linear => base_ms.saturating_mul(attempt as u64),
            Backoff::Exponential => {
                let shift = (attempt - 1).min(63);
                base_ms.saturating_mul(1u64 << shift)
            }
        };
        let capped = if self.max_delay.is_zero() {
            raw_ms
        } else {
            raw_ms.min(self.max_delay.as_millis() as u64)
        };
        Duration::from_millis(capped)
    }
}

/// Execute an async operation with retries.
///
/// `operation` is a factory that creates a fresh future for each attempt.
/// It is called with a 0-based attempt index.
///
/// If the operation returns `Ok(value)`, it is returned immediately.
/// If the operation returns `Err(error)`, the policy decides whether to retry
/// after a backoff delay. The last error is returned when all attempts fail.
pub(crate) async fn with_retry<T, E, F>(
    policy: &RetryPolicy,
    mut operation: impl FnMut(u32) -> F,
) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    let attempts = policy.max_attempts.max(1);
    let mut last_error = None;
    for attempt in 0..attempts {
        match operation(attempt).await {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    let delay = policy.delay_for_attempt(attempt + 1);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
    }
    Err(last_error.expect("retry loop executed at least once"))
}

/// Like [`with_retry`], but only retries when `should_retry(&error)` returns `true`.
/// Use this to avoid retrying permanent errors (e.g. SFTP host-key mismatch).
pub(crate) async fn with_retry_if<T, E, F>(
    policy: &RetryPolicy,
    mut operation: impl FnMut(u32) -> F,
    should_retry: impl Fn(&E) -> bool,
) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    let attempts = policy.max_attempts.max(1);
    let mut last_error = None;
    for attempt in 0..attempts {
        match operation(attempt).await {
            Ok(value) => return Ok(value),
            Err(error) => {
                let retry = should_retry(&error);
                last_error = Some(error);
                if retry && attempt + 1 < attempts {
                    let delay = policy.delay_for_attempt(attempt + 1);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                } else {
                    break;
                }
            }
        }
    }
    Err(last_error.expect("retry loop executed at least once"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn fixed_delay() {
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::ZERO,
            backoff: Backoff::Fixed,
        };
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(100));
    }

    #[test]
    fn linear_delay() {
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::ZERO,
            backoff: Backoff::Linear,
        };
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(600));
    }

    #[test]
    fn exponential_delay() {
        let policy = RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::ZERO,
            backoff: Backoff::Exponential,
        };
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(800));
    }

    #[test]
    fn max_delay_cap() {
        let policy = RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(2),
            backoff: Backoff::Exponential,
        };
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(1000));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(2000));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(2000)); // capped
    }

    #[test]
    fn zero_attempt_returns_zero() {
        let policy = RetryPolicy::http_request();
        assert_eq!(policy.delay_for_attempt(0), Duration::ZERO);
    }

    #[tokio::test]
    async fn succeeds_first_try() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let result: Result<&str, &str> = with_retry(&RetryPolicy::http_request(), |_attempt| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("ok")
            }
        })
        .await;
        assert_eq!(result, Ok("ok"));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn succeeds_after_retries() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::ZERO,
            backoff: Backoff::Fixed,
        };
        let result: Result<u32, String> = with_retry(&policy, |_attempt| {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(format!("fail {n}"))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        assert_eq!(result, Ok(2));
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn exhausts_all_attempts() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::ZERO,
            backoff: Backoff::Fixed,
        };
        let result: Result<(), String> = with_retry(&policy, |_attempt| {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                Err(format!("fail {n}"))
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "fail 2");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn http_request_preset() {
        let p = RetryPolicy::http_request();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.base_delay, Duration::from_millis(25));
        assert_eq!(p.backoff, Backoff::Fixed);
    }

    #[test]
    fn sftp_connect_preset() {
        let p = RetryPolicy::sftp_connect();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.base_delay, Duration::from_millis(500));
        assert_eq!(p.max_delay, Duration::from_secs(5));
        assert_eq!(p.backoff, Backoff::Exponential);
    }

    #[test]
    fn ftp_worker_preset() {
        let p = RetryPolicy::ftp_worker();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.base_delay, Duration::from_millis(500));
        assert_eq!(p.max_delay, Duration::from_secs(5));
        assert_eq!(p.backoff, Backoff::Exponential);
    }

    #[test]
    fn hls_segment_preset() {
        let p = RetryPolicy::hls_segment();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.base_delay, Duration::from_millis(500));
        assert_eq!(p.max_delay, Duration::from_secs(5));
        assert_eq!(p.backoff, Backoff::Exponential);
    }

    #[test]
    fn metalink_mirror_preset() {
        let p = RetryPolicy::metalink_mirror();
        assert_eq!(p.max_attempts, 2);
        assert_eq!(p.base_delay, Duration::from_secs(1));
        assert_eq!(p.backoff, Backoff::Fixed);
    }
}
