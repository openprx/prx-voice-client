//! Digital-zero watchdog of the process tap: decides when "the application is playing but the tap has never delivered anything but exact zeros" means consent is missing.
//!
//! Pure and clock-injected, like [`crate::tap_watchdog`]: the supervisor passes
//! the current time and the observed facts. The decision uses "output running"
//! plus exact zeros, never a level.
//!
//! **Silence is not suspicious once signal has been proven.** A call application
//! keeps its output stream running while the other side is silent (muted peer,
//! voice-activity suppression), and that output is digital silence for as long as
//! the peer stays quiet. So the watchdog only suspects missing consent while no
//! non-zero sample has ever arrived (from this tap or, via `proven`, from any tap
//! of this run). After proof it never rebuilds and never reports.
//!
//! Sequence without proof: zeros with the output running for the window fire
//! [`TapEvent::SilentWhilePlaying`] and ask for one rebuild (the only one); zeros for
//! another window after it latch [`TapEvent::NoSignal`] (a typed error state) and
//! nothing rebuilds again. Real audio at any point proves the consent and, if
//! something was reported, emits [`TapEvent::SignalRestored`].

use std::time::{Duration, Instant};

use crate::tap_types::{TapError, TapEvent};

/// Time of exact zeros with the output running, with no signal ever proven, after which
/// the watchdog acts. Long on purpose: at the start of a call the other side may stay
/// silent for a while, and a rebuild or an error for that would be wrong.
pub const DEFAULT_ZERO_TIMEOUT: Duration = Duration::from_secs(15);

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
    /// A non-zero sample has been seen in this run: silence is normal from now on.
    proven: bool,
}

impl ZeroWatchdog {
    /// Start watching `target` (named in the error) at `now`, with the counters as given.
    /// `proven` is true when an earlier tap of this run already delivered signal.
    pub fn new(
        timeout: Duration,
        target: impl Into<String>,
        now: Instant,
        obs: ZeroObservation,
        proven: bool,
    ) -> Self {
        Self {
            timeout,
            target: target.into(),
            phase: Phase::Watching,
            window_start: now,
            window_callbacks: obs.callbacks,
            last_nonzero: obs.nonzero_callbacks,
            proven: proven || obs.nonzero_callbacks > 0,
        }
    }

    /// Whether signal has been seen in this run (the consent is proven).
    pub fn is_proven(&self) -> bool {
        self.proven
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
            self.proven = true;
            let reported = self.phase != Phase::Watching;
            self.phase = Phase::Watching;
            self.restart_window(now, obs);
            return ZeroStep { emit: reported.then_some(TapEvent::SignalRestored), rebuild: false };
        }
        if self.proven {
            // The consent works: zeros are the other side being quiet.
            self.restart_window(now, obs);
            return ZeroStep::default();
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
