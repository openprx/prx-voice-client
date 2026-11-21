//! The process tap's health decisions as one pure state: stall, target gone, digital zeros, and the bookkeeping of re-creations and the capture interruption they cause.
//!
//! The supervisor thread ([`crate::tap_supervisor`]) gathers the facts of each poll,
//! hands them to [`TapHealth::poll`] with the current time and carries out the
//! returned [`Actions`]; nothing here touches Core Audio or reads a clock, so the
//! whole policy is tested with an injected clock and a fake tap.
//!
//! The rule it enforces: silence alone never rebuilds a tap that has worked. Stall
//! (no IO callbacks) and a vanished target are failures of the endpoint; exact
//! zeros are only suspicious while no signal was ever proven (see
//! [`crate::tap_zero_watchdog`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tap_incidents::{RebuildReason, TapIncidents};
use crate::tap_types::{TapEvent, TapStats};
use crate::tap_watchdog::{DEFAULT_STALL_TIMEOUT, Observation, Phase, StallWatchdog, TapPolicy};
use crate::tap_zero_watchdog::{DEFAULT_ZERO_TIMEOUT, ZeroObservation, ZeroWatchdog};

/// Facts the supervisor observed at one poll.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Facts {
    /// The target has a Core Audio process object.
    pub target_running: bool,
    /// The tap restores its target by itself (macOS 26 bundle-ID form).
    pub native_restore: bool,
    /// The target's process object reports output running.
    pub output_running: bool,
    /// The tap's counters.
    pub stats: TapStats,
}

/// What the supervisor must do after a poll.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Actions {
    /// Events to report, in order.
    pub events: Vec<TapEvent>,
    /// Destroy the current native tap.
    pub teardown: bool,
    /// Try to create a native tap now.
    pub create: bool,
}

/// Why and since when capture is interrupted by a re-creation; booked when the new tap runs.
#[derive(Debug, Default)]
struct RebuildClock {
    open: Option<(RebuildReason, Instant)>,
}

/// The health state of one supervised tap.
#[derive(Debug)]
pub(crate) struct TapHealth {
    stall: StallWatchdog,
    policy: TapPolicy,
    zero: ZeroWatchdog,
    clock: RebuildClock,
    incidents: Arc<TapIncidents>,
}

impl TapHealth {
    /// Health of a tap that was just started at `now`. `signal_proven` is true when an
    /// earlier tap of this run delivered signal.
    pub(crate) fn new(
        now: Instant,
        target: String,
        stats: &TapStats,
        signal_proven: bool,
        incidents: Arc<TapIncidents>,
    ) -> Self {
        Self::with_timeouts(now, target, stats, signal_proven, incidents, DEFAULT_STALL_TIMEOUT, DEFAULT_ZERO_TIMEOUT)
    }

    pub(crate) fn with_timeouts(
        now: Instant,
        target: String,
        stats: &TapStats,
        signal_proven: bool,
        incidents: Arc<TapIncidents>,
        stall_timeout: Duration,
        zero_timeout: Duration,
    ) -> Self {
        Self {
            stall: StallWatchdog::new(stall_timeout, now, stats.callbacks),
            policy: TapPolicy::running(),
            zero: ZeroWatchdog::new(zero_timeout, target, now, zero_observation(false, stats), signal_proven),
            clock: RebuildClock::default(),
            incidents,
        }
    }

    /// Whether signal has been seen in this run (the supervisor publishes this as the session proof).
    pub(crate) fn signal_proven(&self) -> bool {
        self.zero.is_proven()
    }

    /// Decide what to do for one poll.
    pub(crate) fn poll(&mut self, now: Instant, facts: &Facts) -> Actions {
        let observation = Observation {
            target_running: facts.target_running,
            native_restore: facts.native_restore,
            verdict: self.stall.observe(now, facts.stats.callbacks),
        };
        let mut step = self.policy.step(observation);
        let mut actions = Actions::default();
        let mut reason = step.teardown.then_some(RebuildReason::Stall).filter(|_| step.create);
        if self.policy.phase() == Phase::Running && !step.teardown {
            // Only a running tap with a running target has zeros worth judging.
            let zero_step = self.zero.step(now, zero_observation(facts.output_running, &facts.stats));
            actions.events.extend(zero_step.emit);
            if zero_step.rebuild {
                step = self.policy.rebuild();
                reason = Some(RebuildReason::Silence);
            }
        }
        actions.events.extend(step.emit.clone());
        actions.teardown = step.teardown;
        actions.create = step.create;
        self.book_start(now, facts.target_running, &step, reason);
        actions
    }

    fn book_start(
        &mut self,
        now: Instant,
        target_running: bool,
        step: &crate::tap_watchdog::Step,
        reason: Option<RebuildReason>,
    ) {
        if !target_running {
            // The application quit: its absence is not an interruption of ours.
            self.clock.open = None;
        } else if step.teardown {
            self.clock.open = reason.map(|r| (r, now));
        } else if step.create && self.clock.open.is_none() {
            // The target is back: the interruption is the time the creation takes.
            self.clock.open = Some((RebuildReason::TargetReturned, now));
        }
    }

    /// A native tap was created and started at `now`; `stats` are the counters then.
    pub(crate) fn created(&mut self, now: Instant, stats: &TapStats) {
        self.policy.created();
        self.stall.reset(now, stats.callbacks);
        self.zero.restart_window(now, zero_observation(false, stats));
        if let Some((reason, since)) = self.clock.open.take() {
            self.incidents.record(reason, now.saturating_duration_since(since));
        }
    }
}

fn zero_observation(output_running: bool, stats: &TapStats) -> ZeroObservation {
    ZeroObservation { output_running, callbacks: stats.callbacks, nonzero_callbacks: stats.nonzero_callbacks }
}

#[cfg(test)]
mod tests;
