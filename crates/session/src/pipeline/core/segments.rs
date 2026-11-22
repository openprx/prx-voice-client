//! Executes the actions of the segment book: clause bookkeeping, cancellation of a scope, ordered release.

use super::{ClauseInfo, Core, Stats};
use crate::interrupt::CancelAction;
use crate::pipeline::events::PipelineEvent;
use crate::pipeline::messages::{CommitReason, MtItem, TtsItem};
use crate::pipeline::queue::Pushed;
use crate::pipeline::snapshot::{ClauseSnapshot, SkipReason};
use crate::segment::{ClauseEventKind, ClauseState, SegmentState};
use crate::direction_state::DirectionState;
use crate::pipeline::utterance::{Ms, Utterance, notes, skip_name};
use crate::segment_book::{SegmentAction, SegmentEvent};
use crate::segment::ClauseView;
use prxvc_media::{ClauseId, Envelope, MonoTime, SegmentId};

use crate::pipeline::messages::RefineJob;

impl Core {
    /// The clauses the book knows, aged for the barge-in and backlog rules. A unit that does not
    /// end its source sentence was cut early, so its own speech ended earlier than the sentence it
    /// belongs to: it is aged from the end of the newest clause of its segment, and the middle of
    /// a long sentence is not dropped as stale while its end is still young
    /// (incremental-retranslation 4.7.7). The rules themselves are unchanged.
    pub(super) fn aged_clause_views(&self) -> Vec<ClauseView> {
        let mut views = self.book.clause_views();
        let mut newest: std::collections::HashMap<SegmentId, MonoTime> =
            std::collections::HashMap::new();
        for v in &views {
            let end = newest.entry(v.clause.segment()).or_insert(v.source_end);
            *end = (*end).max(v.source_end);
        }
        for v in &mut views {
            let continues = self.clauses.get(&v.clause).is_some_and(|i| i.continues);
            if let Some(end) = newest.get(&v.clause.segment()).filter(|_| continues) {
                v.source_end = *end;
            }
        }
        views
    }

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
            SegmentAction::SegmentDone(segment) => {
                self.w.tree.forget_segment(segment);
                self.finish_utterance(segment);
            }
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
        let tally = self.tallies.entry(clause.segment()).or_default();
        tally.clauses += 1;
        tally.first_cut.get_or_insert(commit.reason.name());
        if commit.reason == CommitReason::Retranslation {
            tally.retranslated += 1;
        }
        self.w
            .tracer
            .mark_text(clause, prxvc_ports::TraceStage::ClauseCommitted, || {
                commit.text.clone()
            });
        self.w
            .tracer
            .mark_text(clause, prxvc_ports::TraceStage::UnitCut, || {
                let sentence = if commit.continues {
                    "continues"
                } else {
                    "ends"
                };
                format!("{} {sentence}", commit.reason.name())
            });
        self.clauses.insert(
            clause,
            ClauseInfo {
                token: token.clone(),
                revision,
                source_ts,
                source_text: commit.text.clone(),
                source_end: commit.source_end,
                continues: commit.continues,
                first_chunk_at: None,
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
            flags: Vec::new(),
            retried: false,
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

    /// The segment is over: log its one line (figures and flags, never text) and keep its latency.
    fn finish_utterance(&mut self, segment: prxvc_media::SegmentId) {
        let tally = self.tallies.remove(&segment).unwrap_or_default();
        let times = self.w.tracer.take_times(segment);
        if tally.clauses == 0 {
            return;
        }
        let u = Utterance::new(segment, tally, &times);
        tracing::info!(
            direction = ?self.p.direction,
            segment = segment.seq(),
            clauses = u.tally.clauses,
            outcome = u.outcome(),
            speech_ms = %Ms(u.speech_ms),
            first_commit_ms = %Ms(u.first_commit_ms),
            first_cut = u.tally.first_cut.unwrap_or("-"),
            retranslated = u.tally.retranslated,
            drafts = u.tally.drafts,
            draft_hits = u.tally.draft_hits,
            drafts_stale = u.tally.drafts_stale,
            continued = u.tally.continued,
            diverged = u.tally.diverged,
            remainder_empty = u.tally.remainder_empty,
            src_rewrites = u.tally.src_rewrites,
            asr_final_ms = %Ms(u.asr_final_ms),
            mt_first_token_ms = %Ms(u.mt_first_token_ms),
            first_audio_ms = %Ms(u.first_audio_ms),
            playback_end_ms = %Ms(u.playback_end_ms),
            queue_wait_ms = %Ms(u.queue_wait_ms),
            notes = %notes(&u.tally),
            "utterance finished"
        );
        self.ledger.record(&u);
        self.dirty = true;
    }

    /// Start passing the original voice through while the open clause of `segment` is already in
    /// the language the listener hears, without waiting for the clause to commit.
    /// Whether `text` is what this direction's lane said to the other party a moment ago.
    pub(super) fn is_own_echo(&mut self, text: &str) -> bool {
        if self.p.direction != prxvc_media::Direction::Outgoing {
            return false;
        }
        let now = self.w.tracer.now();
        let live: Vec<&str> = self.clauses.values().map(|i| i.target_text.as_str()).collect();
        self.own.is_echo(now, text, &live)
    }

    pub(super) fn hypothesis_in_target(&mut self, segment: prxvc_media::SegmentId, text: &str) {
        if self.p.skip_source_in_target
            && !self.is_own_echo(text)
            && self.passthrough.is_none()
            && matches!(self.state, DirectionState::Running)
            && prxvc_media::script::is_written_in(text, self.p.target)
        {
            self.set_passthrough(Some(segment));
        }
    }

    /// Whether the clause of `segment` may be skipped as already in the listener's language: the
    /// speaker is still going (the original voice passes through now) or the original was passed
    /// through while the segment was open. A final that arrives after the segment ended with the
    /// original never passed would be lost entirely, so it is translated instead; and it never
    /// switches the direction to the original again, which nothing would switch back.
    fn can_pass_through(&self, segment: prxvc_media::SegmentId) -> bool {
        self.book.segment_state(segment) == Some(SegmentState::Open)
            || self.passed == Some(segment)
    }

    /// Record and announce that the original voice passes through (`Some`) or no longer does.
    pub(super) fn set_passthrough(&mut self, segment: Option<prxvc_media::SegmentId>) {
        let was = self.passthrough.is_some();
        self.passthrough = segment;
        if segment.is_some() {
            self.passed = segment;
        }
        if was != segment.is_some() {
            self.emit(PipelineEvent::Passthrough {
                on: segment.is_some(),
            });
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
        if self.is_own_echo(&text) {
            // Our own translated speech, heard again: never translated, never passed through.
            self.cancel_with(CancelAction::CancelClause(clause), SkipReason::SelfEcho);
            return;
        }
        if self.p.skip_source_in_target
            && prxvc_media::script::is_written_in(&text, self.p.target)
            && self.can_pass_through(clause.segment())
        {
            // Already in the language the listener hears: nothing to translate or speak; the
            // original voice is heard for as long as the speaker goes on.
            if self.book.segment_state(clause.segment()) == Some(SegmentState::Open) {
                self.set_passthrough(Some(clause.segment()));
            }
            self.cancel_with(
                CancelAction::CancelClause(clause),
                SkipReason::SourceInTarget,
            );
            return;
        }
        if self.passthrough == Some(clause.segment()) {
            self.set_passthrough(None);
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
                // A clause already in the target language is passed through, not lost: it is
                // counted on its own and does not appear among what was not said.
                *self
                    .tallies
                    .entry(clause.segment())
                    .or_default()
                    .skipped
                    .entry(skip_name(reason))
                    .or_default() += 1;
                if !matches!(reason, SkipReason::SourceInTarget | SkipReason::SelfEcho) {
                    Stats::add(&self.w.stats.skipped, 1);
                    self.record_skip(clause, reason);
                }
                match reason {
                    SkipReason::Backlog => Stats::add(&self.w.stats.backlog_skips, 1),
                    SkipReason::Stale => Stats::add(&self.w.stats.stale_skips, 1),
                    SkipReason::QueueFull => {}
                    SkipReason::SourceInTarget => Stats::add(&self.w.stats.in_target_passed, 1),
                    SkipReason::SelfEcho => Stats::add(&self.w.stats.self_echo_dropped, 1),
                }
                self.w.tracer.mark(clause, prxvc_ports::TraceStage::Skipped);
                self.emit(PipelineEvent::ClauseSkipped { clause, reason });
                self.drop_clause(clause);
            }
            ClauseState::Cancelled => {
                self.tallies.entry(clause.segment()).or_default().cancelled += 1;
                self.w
                    .tracer
                    .mark(clause, prxvc_ports::TraceStage::Cancelled);
                self.drop_clause(clause);
            }
            ClauseState::Failed(stage) => {
                self.tallies.entry(clause.segment()).or_default().failed += 1;
                Stats::add(&self.w.stats.clauses_failed, 1);
                self.w.tracer.mark(clause, prxvc_ports::TraceStage::Failed);
                self.emit(PipelineEvent::ClauseFailed { clause, stage });
                self.drop_clause(clause);
            }
            ClauseState::Played => {
                self.tallies.entry(clause.segment()).or_default().played += 1;
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
            if self.p.direction == prxvc_media::Direction::Outgoing {
                // What was said to the other party is remembered for a while: the microphone may
                // hear it again.
                let now = self.w.tracer.now();
                self.own.remember(now, &info.target_text);
            }
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
