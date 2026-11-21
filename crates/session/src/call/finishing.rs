//! The graceful stop: what the speakers said last is still translated and heard before call mode ends.
//!
//! A first Stop while translated directions run does not end them. Each pipeline stops accepting
//! audio and ends the segment that was open, so that the last words are committed; clauses already
//! committed are translated, synthesised and played to their end, to the listener's headphones and
//! to the virtual microphone, up to `call.stop_grace_ms` (6 s by default). The call machine stays
//! in its state and the snapshot says it is *finishing*. When both directions have nothing in
//! flight and the sinks have played out, or when the limit is reached, the stop proceeds as it
//! always did (flush, restore the default input, close the endpoints). A second Stop ends it at
//! once. With the limit at zero, or nothing translating, Stop acts at once as before.

use std::time::Duration;

use prxvc_media::MonoTime;

use super::coordinator::{BOTH, Coordinator, slot};
use crate::call_state::{CallEvent, CallState};
use crate::pipeline::DirectionSnapshot;
use crate::segment_book::Activity;

/// How long both directions must have nothing in flight before the stop proceeds: it covers the
/// last samples still in the sinks and the last events still on their way.
const SETTLE: Duration = Duration::from_millis(250);

/// The sink must hold less than this for the last sample to count as played.
const SINK_EMPTY: Duration = Duration::from_millis(60);

/// A graceful stop in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Finishing {
    deadline: MonoTime,
    idle_since: Option<MonoTime>,
}

/// Whether a direction's pipeline has nothing left in flight: no open speech, every clause ended.
fn is_idle(s: &DirectionSnapshot) -> bool {
    s.activity == Activity::Listening
        && s.partial.is_none()
        && s.clauses.iter().all(|c| c.state.is_terminal())
}

impl Coordinator {
    /// Whether a graceful stop applies now: a translation runs and the limit is not zero.
    fn can_finish_gracefully(&self) -> bool {
        !self.settings.stop_grace.is_zero()
            && matches!(
                self.state,
                CallState::Translating
                    | CallState::Degraded { .. }
                    | CallState::Paused { .. }
                    | CallState::Recovering { .. }
            )
            && self.lanes.iter().any(|l| l.running.is_some())
    }

    /// The first Stop: stop listening and let what is committed play. `false` when the stop is
    /// to proceed at once.
    pub(super) fn begin_finishing(&mut self) -> bool {
        if !self.can_finish_gracefully() {
            return false;
        }
        for d in BOTH {
            if let Some(r) = self.lanes[slot(d)].running.as_ref() {
                r.drain();
            }
        }
        let deadline = self.now().saturating_add(self.settings.stop_grace);
        self.finishing = Some(Finishing {
            deadline,
            idle_since: None,
        });
        tracing::info!(
            limit_ms = self.settings.stop_grace.as_millis() as u64,
            "stop: no new speech is accepted; the last sentences are played first"
        );
        true
    }

    /// The periodic check of a graceful stop; it proceeds with the stop when it is time.
    pub(super) fn poll_finishing(&mut self) {
        let Some(mut f) = self.finishing else {
            return;
        };
        let now = self.now();
        if now >= f.deadline {
            tracing::info!("stop: the limit for the last sentences was reached");
            self.finish_now();
            return;
        }
        if self.everything_played() {
            let since = *f.idle_since.get_or_insert(now);
            if now.saturating_since(since) >= SETTLE {
                tracing::info!("stop: the last sentences were played");
                self.finish_now();
                return;
            }
        } else {
            f.idle_since = None;
        }
        self.finishing = Some(f);
    }

    /// Nothing is in flight in either pipeline and the sinks have played out.
    fn everything_played(&self) -> bool {
        self.lanes.iter().all(|l| {
            let Some(r) = l.running.as_ref() else {
                return true;
            };
            let sink_empty = l.stats.as_ref().is_none_or(|s| s.queued < SINK_EMPTY);
            r.drain_flushed() && r.snapshot().is_none_or(|s| is_idle(&s)) && sink_empty
        })
    }

    /// End the graceful stop and let the stop proceed.
    pub(super) fn finish_now(&mut self) {
        self.finishing = None;
        self.feed(CallEvent::StopRequested);
        self.release_claim_when_idle();
    }

    /// Seconds left until the limit, rounded up, while a graceful stop runs (for the snapshot).
    pub(super) fn finishing_left_s(&self) -> Option<u32> {
        let f = self.finishing?;
        let left = f.deadline.saturating_since(self.now());
        Some(u32::try_from(left.as_millis().div_ceil(1000)).unwrap_or(u32::MAX))
    }
}
