//! Executes the actions of the segment book: clause bookkeeping, cancellation of a scope, ordered release.

use super::{ClauseInfo, Core, Stats};
use crate::interrupt::CancelAction;
use crate::pipeline::events::PipelineEvent;
use crate::pipeline::messages::{MtItem, TtsItem};
use crate::pipeline::queue::Pushed;
use crate::pipeline::snapshot::{ClauseSnapshot, SkipReason};
use crate::segment::{ClauseEventKind, ClauseState};
use crate::segment_book::{SegmentAction, SegmentEvent};
use prxvc_media::ClauseId;
use prxvc_media::Envelope;

use crate::pipeline::messages::RefineJob;

impl Core {
    /// Feed `event` to the segment book and execute its actions. Returns
    /// `false` when the book refused the event or dropped it as late.
    pub(super) fn apply_segment(&mut self, event: SegmentEvent) -> bool {
        match self.book.on(event) {
            Err(rejection) => {
                Stats::add(&self.w.stats.rejected_events, 1);
                tracing::debug!(%rejection, "segment book refused an event");
                false
            }
            Ok(actions) => {
                let mut accepted = true;
                for action in actions {
                    accepted &= !matches!(action, SegmentAction::DropLate { .. });
                    self.run_segment_action(action);
                }
                accepted
            }
        }
    }

    fn run_segment_action(&mut self, action: SegmentAction) {
        self.dirty = true;
        match action {
            SegmentAction::Opened(_)
            | SegmentAction::HypothesisAccepted { .. }
            | SegmentAction::SegmentClosed(_)
            | SegmentAction::DropLate { .. } => {}
            SegmentAction::ClauseCommitted(clause) => self.on_clause_committed(clause),
            SegmentAction::ClauseAdvanced { clause, state } => {
                self.on_clause_advanced(clause, state);
            }
            SegmentAction::ReleaseForPlayback(clause) => self.release(clause),
            SegmentAction::SegmentDone(segment) => self.w.tree.forget_segment(segment),
            SegmentAction::Cancel(action) => self.execute_cancel(action),
        }
    }

    /// A clause was committed: register it and send it on to refinement or translation.
    fn on_clause_committed(&mut self, clause: ClauseId) {
        let Some(commit) = self.commit_stash.take() else {
            return;
        };
        let (revision, source_ts) = (
            self.book.clause_revision(clause).unwrap_or_default(),
            self.book
                .segment_onset(clause.segment())
                .unwrap_or(commit.source_start),
        );
        let token = self.w.tree.clause(clause);
        Stats::add(&self.w.stats.clauses_committed, 1);
        self.w
            .tracer
            .mark_text(clause, prxvc_ports::TraceStage::ClauseCommitted, || {
                commit.text.clone()
            });
        self.clauses.insert(
            clause,
            ClauseInfo {
                token: token.clone(),
                revision,
                source_ts,
                source_text: commit.text.clone(),
                source_end: commit.source_end,
                target_text: String::new(),
                synthesised: std::time::Duration::ZERO,
                chunks: 0,
                tts_done: false,
                voice: None,
                held: std::collections::VecDeque::new(),
                held_ms: 0,
                released: false,
            },
        );
        self.captions.push_back(ClauseSnapshot {
            clause,
            state: ClauseState::Committed,
            source_text: commit.text.clone(),
            translation: String::new(),
            skipped: None,
        });
        while self.captions.len() > self.p.snapshot_clauses {
            self.captions.pop_front();
        }
        match commit.audio {
            Some(audio) if self.p.refines() => {
                let job = Envelope::new(
                    clause.segment(),
                    revision,
                    source_ts,
                    RefineJob {
                        clause,
                        stream_text: commit.text,
                        audio,
                        token,
                    },
                );
                self.queue_refine(job);
            }
            _ => self.recognized(clause, commit.text),
        }
    }

    fn queue_refine(&mut self, job: Envelope<RefineJob>) {
        if let Pushed::Evicted(old) = self.w.refine_in.push(job, None) {
            for job in old {
                // The refine queue is full: the oldest clause keeps its streaming text.
                Stats::add(&self.w.stats.refine_fallbacks, 1);
                self.recognized(job.payload.clause, job.payload.stream_text);
            }
        }
    }

    /// The final source text of the clause is known: translate it.
    pub(super) fn recognized(&mut self, clause: ClauseId, text: String) {
        if !self.apply_clause(clause, ClauseEventKind::Recognized) {
            return;
        }
        self.w
            .tracer
            .mark_text(clause, prxvc_ports::TraceStage::AsrFinalReady, || {
                text.clone()
            });
        if let Some(c) = self.captions.iter_mut().find(|c| c.clause == clause) {
            c.source_text.clone_from(&text);
        }
        let Some(job) = self.live_clause_job(clause, text) else {
            return;
        };
        if let Pushed::Evicted(old) = self.w.mt_in.push(MtItem::Clause(job), None) {
            for item in old {
                if let MtItem::Clause(j) = item {
                    self.cancel_with(
                        CancelAction::CancelClause(j.payload.clause),
                        SkipReason::QueueFull,
                    );
                }
            }
        }
    }

    /// A clause changed state in the book: caption, counters, trace and cleanup.
    fn on_clause_advanced(&mut self, clause: ClauseId, state: ClauseState) {
        if let Some(c) = self.captions.iter_mut().find(|c| c.clause == clause) {
            c.state = state;
        }
        match state {
            ClauseState::Skipped => {
                let reason = self.skip_reason.unwrap_or(SkipReason::Stale);
                if let Some(c) = self.captions.iter_mut().find(|c| c.clause == clause) {
                    c.skipped = Some(reason);
                }
                Stats::add(&self.w.stats.skipped, 1);
                self.record_skip(clause, reason);
                match reason {
                    SkipReason::Backlog => Stats::add(&self.w.stats.backlog_skips, 1),
                    SkipReason::Stale => Stats::add(&self.w.stats.stale_skips, 1),
                    SkipReason::QueueFull => {}
                }
                self.w.tracer.mark(clause, prxvc_ports::TraceStage::Skipped);
                self.emit(PipelineEvent::ClauseSkipped { clause, reason });
                self.drop_clause(clause);
            }
            ClauseState::Cancelled => {
                self.w
                    .tracer
                    .mark(clause, prxvc_ports::TraceStage::Cancelled);
                self.drop_clause(clause);
            }
            ClauseState::Failed(stage) => {
                Stats::add(&self.w.stats.clauses_failed, 1);
                self.w.tracer.mark(clause, prxvc_ports::TraceStage::Failed);
                self.emit(PipelineEvent::ClauseFailed { clause, stage });
                self.drop_clause(clause);
            }
            ClauseState::Played => {
                Stats::add(&self.w.stats.clauses_played, 1);
                self.drop_clause(clause);
            }
            _ => {}
        }
    }

    /// A clause is over: cancel what still works for it and forget its audio.
    fn drop_clause(&mut self, clause: ClauseId) {
        if let Some(info) = self.clauses.remove(&clause) {
            info.token.cancel();
        }
        self.purge_cancelled();
    }

    /// Cancel a scope: provider requests through their tokens, queued items
    /// of the scope, and what the caller already received (`Flush`).
    fn execute_cancel(&mut self, action: CancelAction) {
        match action {
            CancelAction::CancelClause(c) => self.w.tree.cancel_clause(c),
            CancelAction::CancelSegment(s) => self.w.tree.cancel_segment(s),
            CancelAction::CancelDirection(_) => {
                self.w.tree.cancel_direction();
                self.partial = None;
            }
        }
        self.purge_cancelled();
        self.emit(PipelineEvent::Flush(action));
    }

    /// Remove the queued items whose scope was cancelled and wake whoever waits on a queue.
    pub(super) fn purge_cancelled(&mut self) {
        self.w
            .mt_in
            .purge(|i| matches!(i, MtItem::Clause(j) if j.payload.token.is_cancelled()));
        self.w.tts_in.purge(|i| match i {
            TtsItem::Piece(j) | TtsItem::End(j) => j.payload.token.is_cancelled(),
            TtsItem::Probe => false,
        });
        self.w.refine_in.purge(|j| j.payload.token.is_cancelled());
        self.w.mt_in.wake();
        self.w.tts_in.wake();
        self.w.refine_in.wake();
    }
}
