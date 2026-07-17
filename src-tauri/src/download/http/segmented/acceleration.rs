use std::collections::VecDeque;
use std::time::Instant;

use super::AUTO_ACCELERATION_STABILITY_WINDOW;

pub(super) struct AccelerationCheck {
    pub(super) before_connections: i32,
    pub(super) before_speed_bps: i64,
    pub(super) started_at: Instant,
}

#[derive(Default)]
pub(super) struct AccelerationRuntime {
    pub(super) disabled: bool,
    pub(super) split_count: usize,
    pub(super) failure_count: usize,
    pub(super) last_failure_at: Option<Instant>,
    pub(super) pending: Option<AccelerationCheck>,
}

impl AccelerationRuntime {
    pub(super) fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_at = Some(Instant::now());
        if self.failure_count >= 2 {
            self.disabled = true;
        }
    }
}

pub(super) fn speed_is_stable(speed_history: &VecDeque<(Instant, i64)>) -> bool {
    if speed_history.len() < AUTO_ACCELERATION_STABILITY_WINDOW {
        return false;
    }
    let speeds = speed_history
        .iter()
        .map(|(_, speed)| *speed)
        .filter(|speed| *speed > 0)
        .collect::<Vec<_>>();
    if speeds.len() < AUTO_ACCELERATION_STABILITY_WINDOW {
        return false;
    }

    let min = speeds.iter().copied().min().unwrap_or(0);
    let max = speeds.iter().copied().max().unwrap_or(0);
    let average = speeds.iter().sum::<i64>() as f64 / speeds.len() as f64;
    // Stable = max-to-min spread within 15% of the mean. Zero-speed samples are excluded
    // from the band check (they indicate a momentary stall, not a steady-state speed) but
    // still count against the minimum-sample requirement.
    average > 0.0 && (max - min) as f64 <= average * 0.15
}
