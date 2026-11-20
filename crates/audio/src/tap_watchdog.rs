//! Tap health policy: stall detection from the callback counter and the decision of when to report, tear down and re-create the tap.
//!
//! Pure and clock-injected: the supervisor thread passes the current time and
//! the observed facts; nothing here touches Core Audio or reads a clock.

use std::time::{Duration, Instant};

use crate::tap_types::TapEvent;

/// Time without a new IO callback after which a running tap counts as stalled.
pub const DEFAULT_STALL_TIMEOUT: Duration = Duration::from_secs(2);

/// What the stall watchdog concluded from one observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Callbacks are arriving.
    Healthy,
    /// No callback for at least the timeout.
    Stalled {
        /// How long since the last new callback.
        idle: Duration,
    },
}

/// Detects that IO callbacks stopped arriving.
#[derive(Debug, Clone)]
pub struct StallWatchdog {
    timeout: Duration,
    last_count: u64,
    last_change: Instant,
}

impl StallWatchdog {
    /// Start watching at `now` with the counter at `count`.
    pub fn new(timeout: Duration, now: Instant, count: u64) -> Self {
        Self { timeout, last_count: count, last_change: now }
    }

    /// Restart the watch for a new tap (counter and clock).
    pub fn reset(&mut self, now: Instant, count: u64) {
        self.last_count = count;
        self.last_change = now;
    }

    /// Feed the current time and callback counter.
    pub fn observe(&mut self, now: Instant, count: u64) -> Verdict {
        if count != self.last_count {
            self.last_count = count;
            self.last_change = now;
            return Verdict::Healthy;
        }
        let idle = now.saturating_duration_since(self.last_change);
        if idle >= self.timeout { Verdict::Stalled { idle } } else { Verdict::Healthy }
    }
}

/// Whether a tap currently exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// A native tap is running.
    Running,
    /// No tap; waiting for the target to be running so one can be created.
    Waiting,
}

/// Facts observed by the supervisor at one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// The target application is running (has a Core Audio process object).
    pub target_running: bool,
    /// The tap restores its target by itself (macOS 26 bundle-ID form).
    pub native_restore: bool,
    /// The stall watchdog's verdict (only meaningful while running).
    pub verdict: Verdict,
}

/// What the supervisor must do after a poll.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Step {
    /// Event to report, if any.
    pub emit: Option<TapEvent>,
    /// Destroy the current native tap.
    pub teardown: bool,
    /// Try to create a native tap now.
    pub create: bool,
}

/// The supervisor's decision state.
#[derive(Debug, Clone)]
pub struct TapPolicy {
    phase: Phase,
    gone_reported: bool,
}

impl TapPolicy {
    /// A policy for a tap that is running.
    pub fn running() -> Self {
        Self { phase: Phase::Running, gone_reported: false }
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Decide what to do for one poll.
    pub fn step(&mut self, obs: Observation) -> Step {
        match self.phase {
            Phase::Running => self.step_running(obs),
            Phase::Waiting => Step { create: obs.target_running, ..Step::default() },
        }
    }

    fn step_running(&mut self, obs: Observation) -> Step {
        if !obs.target_running {
            let emit = (!self.gone_reported).then_some(TapEvent::TargetGone);
            self.gone_reported = true;
            if obs.native_restore {
                // The tap saves the application by bundle ID and restores it.
                return Step { emit, ..Step::default() };
            }
            self.phase = Phase::Waiting;
            return Step { emit, teardown: true, create: false };
        }
        self.gone_reported = false;
        match obs.verdict {
            Verdict::Healthy => Step::default(),
            Verdict::Stalled { idle } => {
                self.phase = Phase::Waiting;
                let idle_ms = u64::try_from(idle.as_millis()).unwrap_or(u64::MAX);
                Step { emit: Some(TapEvent::Stalled { idle_ms }), teardown: true, create: true }
            }
        }
    }

    /// A native tap was created and started.
    pub fn created(&mut self) {
        self.phase = Phase::Running;
        self.gone_reported = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(2);

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn advancing_counter_is_healthy_forever() {
        let t0 = Instant::now();
        let mut w = StallWatchdog::new(T, t0, 0);
        for i in 1..50u64 {
            assert_eq!(w.observe(at(t0, i * 500), i), Verdict::Healthy);
        }
    }

    #[test]
    fn frozen_counter_stalls_exactly_at_the_timeout() {
        let t0 = Instant::now();
        let mut w = StallWatchdog::new(T, t0, 5);
        assert_eq!(w.observe(at(t0, 1_999), 5), Verdict::Healthy);
        assert_eq!(w.observe(at(t0, 2_000), 5), Verdict::Stalled { idle: T });
        assert!(matches!(w.observe(at(t0, 9_000), 5), Verdict::Stalled { .. }));
    }

    #[test]
    fn a_new_callback_clears_the_stall() {
        let t0 = Instant::now();
        let mut w = StallWatchdog::new(T, t0, 5);
        assert!(matches!(w.observe(at(t0, 3_000), 5), Verdict::Stalled { .. }));
        assert_eq!(w.observe(at(t0, 3_100), 6), Verdict::Healthy);
        assert_eq!(w.observe(at(t0, 4_000), 6), Verdict::Healthy);
    }

    #[test]
    fn reset_restarts_the_clock() {
        let t0 = Instant::now();
        let mut w = StallWatchdog::new(T, t0, 5);
        w.reset(at(t0, 10_000), 0);
        assert_eq!(w.observe(at(t0, 11_000), 0), Verdict::Healthy);
    }

    fn obs(target_running: bool, native_restore: bool, stalled: bool) -> Observation {
        Observation {
            target_running,
            native_restore,
            verdict: if stalled { Verdict::Stalled { idle: T } } else { Verdict::Healthy },
        }
    }

    #[test]
    fn healthy_running_tap_does_nothing() {
        let mut p = TapPolicy::running();
        assert_eq!(p.step(obs(true, false, false)), Step::default());
    }

    #[test]
    fn pid_tap_tears_down_when_target_goes_and_reports_once() {
        let mut p = TapPolicy::running();
        let s = p.step(obs(false, false, false));
        assert_eq!(s.emit, Some(TapEvent::TargetGone));
        assert!(s.teardown && !s.create);
        assert_eq!(p.phase(), Phase::Waiting);
        // While waiting and the target is absent: nothing, no repeated event.
        assert_eq!(p.step(obs(false, false, false)), Step::default());
    }

    #[test]
    fn waiting_creates_when_the_target_runs_again() {
        let mut p = TapPolicy::running();
        p.step(obs(false, false, false));
        let s = p.step(obs(true, false, false));
        assert!(s.create && !s.teardown);
        p.created();
        assert_eq!(p.phase(), Phase::Running);
        assert_eq!(p.step(obs(true, false, false)), Step::default());
    }

    #[test]
    fn failed_creation_keeps_waiting_and_retries() {
        let mut p = TapPolicy::running();
        p.step(obs(false, false, false));
        assert!(p.step(obs(true, false, false)).create);
        // The supervisor did not call created(): the next poll tries again.
        assert!(p.step(obs(true, false, false)).create);
    }

    #[test]
    fn native_restore_keeps_the_tap_and_reports_the_loss_once() {
        let mut p = TapPolicy::running();
        let s = p.step(obs(false, true, false));
        assert_eq!(s.emit, Some(TapEvent::TargetGone));
        assert!(!s.teardown && !s.create);
        assert_eq!(p.phase(), Phase::Running);
        assert_eq!(p.step(obs(false, true, true)), Step::default());
        // The target returns: back to normal, and a later loss is reported again.
        assert_eq!(p.step(obs(true, true, false)), Step::default());
        assert_eq!(p.step(obs(false, true, false)).emit, Some(TapEvent::TargetGone));
    }

    #[test]
    fn stall_with_running_target_recreates_the_tap() {
        let mut p = TapPolicy::running();
        let s = p.step(obs(true, false, true));
        assert_eq!(s.emit, Some(TapEvent::Stalled { idle_ms: 2_000 }));
        assert!(s.teardown && s.create);
    }

    #[test]
    fn stall_is_ignored_while_the_target_is_gone() {
        let mut p = TapPolicy::running();
        p.step(obs(false, true, false));
        assert_eq!(p.step(obs(false, true, true)), Step::default());
    }
}
