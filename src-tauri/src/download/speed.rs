use std::{
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

#[derive(Debug)]
pub struct GlobalSpeedLimiter {
    limit_bps: AtomicI64,
    state: Mutex<SpeedLimitState>,
    parent: Option<Arc<GlobalSpeedLimiter>>,
}

#[derive(Debug)]
struct SpeedLimitState {
    tokens: f64,
    last_refill: Instant,
}

impl GlobalSpeedLimiter {
    pub fn new(limit_bps: Option<i64>) -> Self {
        let limit = limit_bps.unwrap_or(0).max(0);
        Self {
            limit_bps: AtomicI64::new(limit),
            state: Mutex::new(SpeedLimitState {
                tokens: limit as f64,
                last_refill: Instant::now(),
            }),
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
                state: Mutex::new(SpeedLimitState {
                    tokens: limit as f64,
                    last_refill: Instant::now(),
                }),
                parent: Some(parent),
            }),
            _ => parent,
        }
    }

    pub fn set_limit(&self, limit_bps: Option<i64>) {
        let limit = limit_bps.unwrap_or(0).max(0);
        self.limit_bps.store(limit, Ordering::SeqCst);
        if let Ok(mut state) = self.state.try_lock() {
            state.tokens = limit as f64;
            state.last_refill = Instant::now();
        }
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
            parent.throttle_self(bytes).await;
        }
        self.throttle_self(bytes).await;
    }

    async fn throttle_self(&self, bytes: usize) {
        let mut remaining = bytes;
        while remaining > 0 {
            let limit = self.limit_bps.load(Ordering::SeqCst);
            if limit <= 0 {
                return;
            }

            let request = remaining.min(limit as usize);
            let wait = {
                let mut state = self.state.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(state.last_refill).as_secs_f64();
                state.tokens = (state.tokens + elapsed * limit as f64).min(limit as f64);
                state.last_refill = now;

                if state.tokens >= request as f64 {
                    state.tokens -= request as f64;
                    remaining -= request;
                    None
                } else {
                    let deficit = request as f64 - state.tokens;
                    Some(Duration::from_secs_f64((deficit / limit as f64).max(0.001)))
                }
            };

            if let Some(wait) = wait {
                tokio::time::sleep(wait.min(Duration::from_millis(250))).await;
            }
        }
    }
}
