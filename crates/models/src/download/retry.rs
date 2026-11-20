//! Retry and backoff policy with an injectable sleeper. Single
//! responsibility: decide how long to wait and how often to try (design 4.2
//! rule 7).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::control::JobControl;

/// Waits after the 1st, 2nd, ... consecutive failure; the last value repeats.
/// After `max_failures` consecutive failures the file fails.
#[derive(Clone, Debug)]
pub struct BackoffPolicy {
    pub delays: Vec<Duration>,
    pub max_failures: u32,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        BackoffPolicy {
            delays: [1, 2, 4, 8, 16, 30]
                .into_iter()
                .map(Duration::from_secs)
                .collect(),
            max_failures: 6,
        }
    }
}

impl BackoffPolicy {
    /// Wait before the next attempt after `failures` consecutive failures
    /// (1-based), or `None` when the budget is spent.
    pub fn delay_after(&self, failures: u32) -> Option<Duration> {
        if failures >= self.max_failures || self.delays.is_empty() {
            return None;
        }
        let idx = (failures.saturating_sub(1) as usize).min(self.delays.len() - 1);
        Some(self.delays[idx])
    }
}

/// Sleeps for a duration, returning early when the job is stopped.
pub type Sleeper = Arc<dyn Fn(Duration, &JobControl) + Send + Sync>;

pub fn real_sleeper() -> Sleeper {
    Arc::new(|d, ctl| {
        let end = Instant::now() + d;
        while !ctl.is_stopped() {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_is_1_2_4_8_16_then_gives_up() {
        let p = BackoffPolicy::default();
        let secs: Vec<_> = (1..=6)
            .map(|n| p.delay_after(n).map(|d| d.as_secs()))
            .collect();
        assert_eq!(
            secs,
            vec![Some(1), Some(2), Some(4), Some(8), Some(16), None]
        );
    }

    #[test]
    fn last_delay_repeats_with_a_bigger_budget() {
        let p = BackoffPolicy {
            max_failures: 9,
            ..Default::default()
        };
        assert_eq!(p.delay_after(8), Some(Duration::from_secs(30)));
        assert_eq!(p.delay_after(9), None);
    }
}
