//! Backlog rule (pipeline-design 3.4.3): which queued clauses are skipped when a direction lags too far behind.

use std::time::Duration;

use prxvc_media::ClauseId;

use crate::interrupt::CancelAction;
use crate::segment::{ClauseState, PlaybackState};

/// One clause as the backlog rule sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BacklogClause {
    /// The clause.
    pub clause: ClauseId,
    /// Its lifecycle state.
    pub state: ClauseState,
    /// Its playback state.
    pub playback: PlaybackState,
    /// How much audio the clause will occupy once played: the synthesised
    /// length where synthesis finished, otherwise the length of its source
    /// speech as the estimate.
    pub audio: Duration,
}

impl BacklogClause {
    /// Whether the clause counts as queued, unplayed audio: not finished and
    /// not started playing (a playing clause is never skipped, D-P5).
    fn is_queued(&self) -> bool {
        !self.state.is_terminal()
            && self.state != ClauseState::Playing
            && self.playback == PlaybackState::NotStarted
    }
}

/// Whether the direction enters or leaves the "skipping to keep up" condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkippingChange {
    /// The bound was exceeded: the direction is Degraded with
    /// `DegradeReason::BacklogSkipping`.
    Enter,
    /// The backlog fell below half the bound: the reason is cleared.
    Leave,
    /// No change.
    Unchanged,
}

/// The decision of the backlog rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklogDecision {
    /// The clauses to skip, oldest first, as `CancelAction::CancelClause`.
    pub skip: Vec<CancelAction>,
    /// The change of the skipping condition.
    pub skipping: SkippingChange,
}

/// Decide what to skip.
///
/// `clauses` are the clauses of the direction in playback order. When the
/// queued, unplayed audio exceeds `max_backlog` the oldest queued clauses are
/// skipped one by one until it no longer does; the newest clause is always
/// kept. The condition ends (`Leave`) only when the audio left is below half
/// of the bound, so the direction does not flap at the limit (CS-4.3.2).
/// `skipping` tells whether the condition is on now.
pub fn decide_backlog(
    max_backlog: Duration,
    clauses: &[BacklogClause],
    skipping: bool,
) -> BacklogDecision {
    let queued: Vec<&BacklogClause> = clauses.iter().filter(|c| c.is_queued()).collect();
    let mut total: Duration = queued.iter().map(|c| c.audio).sum();
    let mut skip = Vec::new();
    if total > max_backlog {
        for c in queued.iter().take(queued.len().saturating_sub(1)) {
            if total <= max_backlog {
                break;
            }
            total = total.saturating_sub(c.audio);
            skip.push(CancelAction::CancelClause(c.clause));
        }
    }
    let change = if !skipping && !skip.is_empty() {
        SkippingChange::Enter
    } else if skipping && skip.is_empty() && total < max_backlog / 2 {
        SkippingChange::Leave
    } else {
        SkippingChange::Unchanged
    };
    BacklogDecision {
        skip,
        skipping: change,
    }
}

#[cfg(test)]
mod tests;
