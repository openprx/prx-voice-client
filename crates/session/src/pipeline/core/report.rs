//! The snapshot of the direction and the backlog check that precedes every publication.

use std::time::Duration;

use super::Core;
use crate::backlog::{BacklogClause, SkippingChange, decide_backlog};
use crate::direction_state::DirectionState;
use crate::fault::DegradeReason;
use crate::pipeline::snapshot::{DirectionSnapshot, SkipReason};
use crate::segment::ClauseState;

impl Core {
    /// The snapshot of the direction as it is now.
    pub(super) fn snapshot(&self) -> DirectionSnapshot {
        DirectionSnapshot {
            version: 0,
            direction: self.p.direction,
            state: self.state.clone(),
            reasons: self.state.reasons(),
            output: self.state.output(&self.cfg.policy),
            activity: self.book.activity(),
            partial: self.partial.clone(),
            clauses: self.captions.iter().cloned().collect(),
            counters: self.w.stats.counters(self.book.late_dropped()),
            backlog_ms: self.backlog_ms,
            last_error: self.last_error.clone(),
        }
    }

    /// Apply the backlog rule (3.4.3): skip the oldest queued clauses while
    /// the direction lags more than `max_backlog`, and track the degraded
    /// condition around it.
    pub(super) fn check_backlog(&mut self) {
        if !matches!(
            self.state,
            DirectionState::Running | DirectionState::Degraded { .. }
        ) {
            return;
        }
        let list: Vec<BacklogClause> = self
            .book
            .clause_views()
            .into_iter()
            .map(|v| BacklogClause {
                clause: v.clause,
                state: v.state,
                playback: v.playback,
                audio: self.clause_audio_estimate(v.clause),
            })
            .collect();
        let queued: Duration = list
            .iter()
            .filter(|c| !c.state.is_terminal() && c.state != ClauseState::Playing)
            .map(|c| c.audio)
            .sum();
        let backlog_ms = u64::try_from(queued.as_millis()).unwrap_or(u64::MAX);
        if backlog_ms != self.backlog_ms {
            self.backlog_ms = backlog_ms;
            self.dirty = true;
        }
        let decision = decide_backlog(self.p.max_backlog, &list, self.skipping);
        for action in decision.skip {
            self.cancel_with(action, SkipReason::Backlog);
        }
        match decision.skipping {
            SkippingChange::Enter => {
                self.skipping = true;
                self.set_reason(DegradeReason::BacklogSkipping, true);
            }
            SkippingChange::Leave => {
                self.skipping = false;
                self.set_reason(DegradeReason::BacklogSkipping, false);
            }
            SkippingChange::Unchanged => {}
        }
    }
}
