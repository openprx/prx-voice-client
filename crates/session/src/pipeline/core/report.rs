//! The snapshot of the direction and the backlog check that precedes every publication.

use std::time::Duration;

use prxvc_media::ClauseId;

use super::Core;
use crate::backlog::{BacklogClause, SkippingChange, decide_backlog};
use crate::direction_state::DirectionState;
use crate::fault::DegradeReason;
use crate::pipeline::snapshot::{DirectionSnapshot, SkipReason, SkippedClause};
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
            recent_skips: self.skips.iter().cloned().collect(),
            last_error: self.last_error.clone(),
            ledger: self.ledger.snapshot(),
        }
    }

    /// Apply the backlog rule (3.4.3): skip stale queued clauses, oldest first, while the audio
    /// queued for the listener is over the bound, and track the degraded condition around it.
    pub(super) fn check_backlog(&mut self) {
        if !matches!(
            self.state,
            DirectionState::Running | DirectionState::Degraded { .. }
        ) {
            self.backlog_due = None;
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
                source_end: v.source_end,
            })
            .collect();
        let now = self.w.tracer.now();
        let decision = decide_backlog(&self.p.backlog, now, &list, self.skipping);
        for action in decision.skip {
            self.cancel_with(action, SkipReason::Backlog);
        }
        let after: Duration = self
            .book
            .clause_views()
            .into_iter()
            .filter(|v| !v.state.is_terminal() && v.state != ClauseState::Playing)
            .map(|v| self.clause_audio_estimate(v.clause))
            .sum();
        let backlog_ms = u64::try_from(after.as_millis()).unwrap_or(u64::MAX);
        if backlog_ms != self.backlog_ms {
            self.backlog_ms = backlog_ms;
            self.dirty = true;
        }
        self.backlog_due = decision.recheck_at;
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

    /// Remember a skipped clause with its source text, so that the captions can show what was not spoken.
    pub(super) fn record_skip(&mut self, clause: ClauseId, reason: SkipReason) {
        let now = self.w.tracer.now();
        let Some(info) = self.clauses.get(&clause) else {
            return;
        };
        let lag = now.saturating_since(info.source_end);
        self.skips.push_back(SkippedClause {
            clause,
            source_text: info.source_text.clone(),
            reason,
            lag_ms: u64::try_from(lag.as_millis()).unwrap_or(u64::MAX),
        });
        while self.skips.len() > MAX_RECENT_SKIPS {
            self.skips.pop_front();
        }
        self.dirty = true;
    }
}

/// Skipped clauses kept in the snapshot.
const MAX_RECENT_SKIPS: usize = 16;
