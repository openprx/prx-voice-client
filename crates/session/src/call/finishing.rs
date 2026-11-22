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

/// Whether a translating direction still has speech pending or playing: open speech, a clause not
/// yet ended, or audio still queued in the sink. This is the same test the graceful stop uses to
/// decide that the last sentences were played, so the window's "sending" indicator and the stop
/// agree. `pipeline` is the direction's pipeline snapshot (`None` when it does not translate) and
/// `sink_queued` the audio queued in its sink (`None` while the path does not run).
pub fn direction_in_flight(
    pipeline: Option<&DirectionSnapshot>,
    sink_queued: Option<Duration>,
) -> bool {
    pipeline.is_some_and(|s| !is_idle(s)) || sink_queued.is_some_and(|q| q >= SINK_EMPTY)
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
            let queued = l.stats.as_ref().map(|s| s.queued);
            r.drain_flushed() && !direction_in_flight(r.snapshot().as_deref(), queued)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::direction_state::{DirectionState, Reasons};
    use crate::output::OutputSource;
    use crate::pipeline::{ClauseSnapshot, Counters, Ledger, PartialSnapshot};
    use crate::segment::ClauseState;
    use prxvc_media::{ClauseId, Direction, SegmentId};

    fn snapshot(activity: Activity) -> DirectionSnapshot {
        DirectionSnapshot {
            version: 1,
            direction: Direction::Outgoing,
            state: DirectionState::Off,
            reasons: Reasons::default(),
            output: OutputSource::Translation,
            activity,
            partial: None,
            clauses: Vec::new(),
            counters: Counters::default(),
            backlog_ms: 0,
            recent_skips: Vec::new(),
            last_error: None,
            ledger: Ledger::default(),
        }
    }

    fn clause(state: ClauseState) -> ClauseSnapshot {
        let seg = SegmentId::new(Direction::Outgoing, 1).expect("seq >= 1");
        ClauseSnapshot {
            clause: ClauseId::new(seg, 0),
            state,
            source_text: String::new(),
            translation: String::new(),
            skipped: None,
            flags: Vec::new(),
            retried: false,
        }
    }

    const EMPTY: Option<Duration> = Some(Duration::ZERO);

    #[test]
    fn a_direction_that_does_not_translate_or_has_drained_is_not_in_flight() {
        assert!(!direction_in_flight(None, None));
        assert!(!direction_in_flight(None, EMPTY));
        let mut s = snapshot(Activity::Listening);
        s.clauses = vec![clause(ClauseState::Played), clause(ClauseState::Skipped)];
        assert!(!direction_in_flight(Some(&s), EMPTY));
    }

    #[test]
    fn open_speech_is_in_flight_before_any_clause_is_queued() {
        let mut s = snapshot(Activity::Recognizing);
        assert!(direction_in_flight(Some(&s), EMPTY));
        s.activity = Activity::Listening;
        s.partial = Some(PartialSnapshot {
            segment: SegmentId::new(Direction::Outgoing, 1).expect("seq >= 1"),
            revision: 1,
            text: "hello".into(),
            stable_prefix_len: 0,
        });
        assert!(direction_in_flight(Some(&s), EMPTY));
    }

    #[test]
    fn a_clause_that_has_not_ended_is_in_flight_whatever_stage_it_is_in() {
        for state in [
            ClauseState::Committed,
            ClauseState::Translating,
            ClauseState::Synthesizing,
            ClauseState::Playing,
        ] {
            let mut s = snapshot(Activity::Listening);
            s.clauses = vec![clause(ClauseState::Played), clause(state.clone())];
            assert!(direction_in_flight(Some(&s), EMPTY), "{state:?}");
        }
    }

    #[test]
    fn audio_still_queued_in_the_sink_is_in_flight_until_it_is_played() {
        let s = snapshot(Activity::Listening);
        assert!(direction_in_flight(
            Some(&s),
            Some(Duration::from_millis(400))
        ));
        assert!(direction_in_flight(Some(&s), Some(SINK_EMPTY)));
        assert!(!direction_in_flight(
            Some(&s),
            Some(Duration::from_millis(10))
        ));
    }
}
