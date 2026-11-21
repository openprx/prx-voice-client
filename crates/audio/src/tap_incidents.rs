//! Counters of the process tap's re-creations and of how long capture was interrupted by them, for the call summary.
//!
//! One shared handle: the tap's supervisor thread writes it (relaxed atomics, never
//! on a real-time thread) and the call host reads it with each stats poll. The
//! counts are per reason so that a summary shows what rebuilt the tap, and the
//! interruption is the time during which no native tap was delivering audio.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Why the tap was torn down and created again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildReason {
    /// Exact zeros while the target's output ran and no signal was ever proven (possible missing consent).
    Silence,
    /// No IO callback arrived for the stall timeout.
    Stall,
    /// The target application quit and came back.
    TargetReturned,
}

/// A reading of [`TapIncidents`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapIncidentCounts {
    /// Re-creations after silence without any proven signal.
    pub silence: u64,
    /// Re-creations after a stall.
    pub stall: u64,
    /// Re-creations after the target came back.
    pub target_returned: u64,
    /// The longest time capture was interrupted by a re-creation, in milliseconds.
    pub longest_interruption_ms: u64,
}

/// Shared counters; see the module docs.
#[derive(Debug, Default)]
pub struct TapIncidents {
    silence: AtomicU64,
    stall: AtomicU64,
    target_returned: AtomicU64,
    longest_interruption_ms: AtomicU64,
}

impl TapIncidents {
    /// A rebuild for `reason` finished after `interrupted` without capture.
    pub fn record(&self, reason: RebuildReason, interrupted: Duration) {
        let counter = match reason {
            RebuildReason::Silence => &self.silence,
            RebuildReason::Stall => &self.stall,
            RebuildReason::TargetReturned => &self.target_returned,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        let ms = u64::try_from(interrupted.as_millis()).unwrap_or(u64::MAX);
        self.longest_interruption_ms.fetch_max(ms, Ordering::Relaxed);
    }

    /// The counters now.
    pub fn snapshot(&self) -> TapIncidentCounts {
        TapIncidentCounts {
            silence: self.silence.load(Ordering::Relaxed),
            stall: self.stall.load(Ordering::Relaxed),
            target_returned: self.target_returned.load(Ordering::Relaxed),
            longest_interruption_ms: self.longest_interruption_ms.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_reason_and_keeps_the_longest_interruption() {
        let c = TapIncidents::default();
        c.record(RebuildReason::Silence, Duration::from_millis(300));
        c.record(RebuildReason::Stall, Duration::from_millis(1_200));
        c.record(RebuildReason::Stall, Duration::from_millis(400));
        c.record(RebuildReason::TargetReturned, Duration::from_millis(50));
        assert_eq!(
            c.snapshot(),
            TapIncidentCounts { silence: 1, stall: 2, target_returned: 1, longest_interruption_ms: 1_200 }
        );
    }
}
