//! Digital-zero watchdog of the process tap: decides when "the application is playing but the tap delivers only exact zeros" means consent is missing or the tap is stale.
//!
//! Pure and clock-injected, like [`crate::tap_watchdog`]: the supervisor passes
//! the current time and the observed facts. The decision uses "output running"
//! plus exact zeros, never a level: real call audio is practically never exactly
//! zero while the application's output runs, and a silent application (output
//! not running) or a pause between utterances never fires it.
//!
//! Sequence: zeros with the output running for the threshold fire
//! [`TapEvent::SilentWhilePlaying`] and ask for one rebuild; zeros for another
//! threshold after the rebuild latch [`TapEvent::NoSignal`] (a typed error
//! state). Real audio at any point returns to watching and, if something was
//! reported, emits [`TapEvent::SignalRestored`].

use std::time::{Duration, Instant};

use crate::tap_types::{TapError, TapEvent};

/// Time of exact zeros with the output running after which the watchdog acts.
pub const DEFAULT_ZERO_TIMEOUT: Duration = Duration::from_secs(3);

/// Facts observed at one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroObservation {
    /// The target's Core Audio process object reports output running.
    pub output_running: bool,
    /// IO callbacks received so far (a counter of the current tap).
    pub callbacks: u64,
    /// Callbacks so far that held at least one sample that is not exactly zero.
    pub nonzero_callbacks: u64,
}

/// What the supervisor must do after a poll.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ZeroStep {
    /// Event to report, if any.
    pub emit: Option<TapEvent>,
    /// Tear down and re-create the tap (the supervisor then calls
    /// [`ZeroWatchdog::restart_window`]).
    pub rebuild: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No silence reported.
    Watching,
    /// Silence reported and the tap rebuilt once; waiting for signal or a second window.
    Rebuilt,
    /// The error state was reported; stays until signal returns.
    Failed,
}

/// The digital-zero watchdog.
#[derive(Debug, Clone)]
pub struct ZeroWatchdog {
    timeout: Duration,
    target: String,
    phase: Phase,
    window_start: Instant,
    window_callbacks: u64,
    last_nonzero: u64,
}

impl ZeroWatchdog {
    /// Start watching `target` (named in the error) at `now`, with the counters as given.
    pub fn new(timeout: Duration, target: impl Into<String>, now: Instant, obs: ZeroObservation) -> Self {
        Self {
            timeout,
            target: target.into(),
            phase: Phase::Watching,
            window_start: now,
            window_callbacks: obs.callbacks,
            last_nonzero: obs.nonzero_callbacks,
        }
    }

    /// Whether the error state has been reported and not cleared.
    pub fn is_failed(&self) -> bool {
        self.phase == Phase::Failed
    }

    /// A new native tap exists (its counters start again): restart the window
    /// without changing what has been reported.
    pub fn restart_window(&mut self, now: Instant, obs: ZeroObservation) {
        self.window_start = now;
        self.window_callbacks = obs.callbacks;
        self.last_nonzero = obs.nonzero_callbacks;
    }

    /// Feed one poll.
    pub fn step(&mut self, now: Instant, obs: ZeroObservation) -> ZeroStep {
        if obs.nonzero_callbacks != self.last_nonzero {
            let reported = self.phase != Phase::Watching;
            self.phase = Phase::Watching;
            self.restart_window(now, obs);
            return ZeroStep { emit: reported.then_some(TapEvent::SignalRestored), rebuild: false };
        }
        if !obs.output_running {
            // Silence is expected: the window must be continuous output.
            self.restart_window(now, obs);
            return ZeroStep::default();
        }
        // No callback in this window means no evidence of delivered zeros; a
        // stalled tap is the stall watchdog's case.
        let silent_for = now.saturating_duration_since(self.window_start);
        if obs.callbacks == self.window_callbacks || silent_for < self.timeout {
            return ZeroStep::default();
        }
        match self.phase {
            Phase::Watching => {
                self.phase = Phase::Rebuilt;
                let silent_ms = u64::try_from(silent_for.as_millis()).unwrap_or(u64::MAX);
                ZeroStep { emit: Some(TapEvent::SilentWhilePlaying { silent_ms }), rebuild: true }
            }
            Phase::Rebuilt => {
                self.phase = Phase::Failed;
                let error = TapError::NoSignal { target: self.target.clone() };
                ZeroStep { emit: Some(TapEvent::NoSignal { error }), rebuild: false }
            }
            Phase::Failed => ZeroStep::default(),
        }
    }
}

#[cfg(test)]
mod tests;
