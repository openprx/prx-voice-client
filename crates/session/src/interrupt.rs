//! Interruption decision: which cancellations a trigger such as new speech, skip, pause or stop causes (pipeline-design 3.4, D-P5).

use std::time::Duration;

use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};

use crate::dir_set::DirSet;
use crate::segment::{ClauseState, ClauseView, PlaybackState};

/// The parameters of the decision, from `call.barge_in` and `call.barge_in_max_lag_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptParams {
    /// `call.barge_in`: whether new speech drops stale queued translation.
    pub barge_in: bool,
    /// `call.barge_in_max_lag_ms`: how long ago the source of a queued clause may have ended.
    pub max_lag: Duration,
}

impl Default for InterruptParams {
    /// Barge-in on with a 3000 ms limit (the configuration defaults, D-P5).
    fn default() -> Self {
        Self {
            barge_in: true,
            max_lag: Duration::from_millis(3_000),
        }
    }
}

/// What happened that may interrupt translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptTrigger {
    /// The same speaker began a new segment (`new_segment`, whose direction is the speaker's).
    SameSpeakerNewSegment {
        /// The segment that just opened.
        new_segment: SegmentId,
    },
    /// The other party began to speak: directions are independent.
    OtherPartySpeaks {
        /// The direction of the new speech.
        speaker: Direction,
    },
    /// The user pressed Skip on a lane or the interrupt shortcut.
    SkipRequested(Direction),
    /// The user paused these directions.
    Paused(DirSet),
    /// The system suspended these directions (sleep, lost endpoint).
    Suspended(DirSet),
    /// The call is stopping.
    Stop,
}

/// A cancellation, addressed by direction, segment or clause. The token tree
/// is call, direction, segment (CS-2.4.5): cancelling a scope cancels its
/// provider calls, drops its queued items and flushes its audio from the
/// playout queue; audio already played is never rolled back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CancelAction {
    /// Cancel one clause that has not started playing (shown as skipped).
    CancelClause(ClauseId),
    /// Cancel one segment, including a clause that is playing (the cut has a short fade).
    CancelSegment(SegmentId),
    /// Cancel every in-flight and queued segment of a direction; the other direction is untouched.
    CancelDirection(Direction),
}

/// Decide the cancellations for `trigger`.
///
/// `queued` is the clause list of the direction named by the trigger
/// (`SegmentBook::clause_views`); it is only read for
/// `SameSpeakerNewSegment`. The result is pure data: the owner feeds each
/// action to the segment book as `SegmentEvent::Cancel` and executes the
/// actions the book returns.
///
/// * Same speaker, new segment, `barge_in` on: skip clauses of older
///   segments that have not started playing and whose source ended more than
///   `max_lag` before `now`. The clause that is playing, younger clauses and
///   the other direction are kept (D-P5).
/// * Same speaker with `barge_in` off, and the other party speaking: nothing.
/// * Skip, pause, suspend: cancel the direction. Stop: cancel both.
pub fn decide_interrupt(
    trigger: InterruptTrigger,
    params: &InterruptParams,
    now: MonoTime,
    queued: &[ClauseView],
) -> Vec<CancelAction> {
    match trigger {
        InterruptTrigger::SameSpeakerNewSegment { new_segment } if params.barge_in => queued
            .iter()
            .filter(|c| is_stale(c, new_segment, params.max_lag, now))
            .map(|c| CancelAction::CancelClause(c.clause))
            .collect(),
        InterruptTrigger::SameSpeakerNewSegment { .. }
        | InterruptTrigger::OtherPartySpeaks { .. } => Vec::new(),
        InterruptTrigger::SkipRequested(d) => vec![CancelAction::CancelDirection(d)],
        InterruptTrigger::Paused(dirs) | InterruptTrigger::Suspended(dirs) => {
            dirs.iter().map(CancelAction::CancelDirection).collect()
        }
        InterruptTrigger::Stop => DirSet::BOTH
            .iter()
            .map(CancelAction::CancelDirection)
            .collect(),
    }
}

/// A clause of an older segment, still queued, whose source ended longer ago than the limit.
fn is_stale(c: &ClauseView, new_segment: SegmentId, max_lag: Duration, now: MonoTime) -> bool {
    c.clause.segment() < new_segment
        && c.clause.segment().direction() == new_segment.direction()
        && !c.state.is_terminal()
        && c.state != ClauseState::Playing
        && c.playback == PlaybackState::NotStarted
        && now.saturating_since(c.source_end) > max_lag
}

#[cfg(test)]
mod tests;
