//! Handles the clause-level events of the refine, translation and synthesis stages.

use prxvc_media::{ClauseId, Envelope};
use prxvc_ports::VoiceSelection;

use super::Core;
use crate::direction_state::DirectionEvent;
use crate::fault::{DegradeReason, Stage};
use crate::filler::is_pure_filler;
use crate::pipeline::messages::{StageEvent, StageFailure};
use crate::pipeline::snapshot::{ErrorSnapshot, Stats};
use crate::segment::{ClauseEventKind, ClauseState};

impl Core {
    pub(super) fn on_stage(&mut self, e: Envelope<StageEvent>) {
        match e.payload {
            StageEvent::Refined { clause, result } => self.refined(clause, result),
            StageEvent::MtStarted { clause } => self.translation_started(clause),
            StageEvent::MtText { clause, delta } => self.translation_text(clause, &delta),
            StageEvent::MtDone { clause } => self.translation_done(clause),
            StageEvent::MtFailed {
                clause,
                failure,
                suspect,
            } => {
                if suspect {
                    Stats::add(&self.w.stats.mt_suspect, 1);
                }
                self.fail_clause(clause, Stage::Mt, &failure, true);
            }
            StageEvent::TtsStarted { clause, voice } => self.synthesis_started(clause, voice),
            StageEvent::TtsChunk { clause, chunk } => self.on_chunk(clause, chunk),
            StageEvent::TtsDone { clause } => self.on_tts_done(clause),
            StageEvent::TtsFailed {
                clause,
                failure,
                runaway,
            } => {
                if runaway {
                    Stats::add(&self.w.stats.runaway_synth, 1);
                }
                self.fail_clause(clause, Stage::Tts, &failure, !runaway);
            }
        }
        self.dirty = true;
    }

    /// Whether the clause is known and not finished; a result for any other clause is late.
    fn clause_is_live(&self, clause: ClauseId) -> bool {
        self.book
            .clause_state(clause)
            .is_some_and(|s| !s.is_terminal())
    }

    fn drop_late(&mut self) {
        Stats::add(&self.w.stats.late_dropped, 1);
    }

    fn refined(&mut self, clause: ClauseId, result: Result<String, StageFailure>) {
        if self.book.clause_state(clause) != Some(ClauseState::Committed) {
            self.drop_late();
            return;
        }
        let text = match result {
            // The second pass heard nothing, or only a hum where the streaming text had words: the
            // streaming text stands. That is not a failure of the pass (nothing broke, the clip
            // simply holds no speech, as with the pad-only clip of a trailing fragment), so it does
            // not count towards `RefineFailing`. A streaming text that is itself filler never gets
            // here when the filter is on: the recognition stage drops it.
            Ok(text)
                if text.trim().is_empty()
                    || (self.p.drop_filler
                        && is_pure_filler(self.p.source, &text)
                        && self
                            .clauses
                            .get(&clause)
                            .is_some_and(|i| !is_pure_filler(self.p.source, &i.source_text))) =>
            {
                Stats::add(&self.w.stats.refine_fallbacks, 1);
                self.clauses
                    .get(&clause)
                    .map(|i| i.source_text.clone())
                    .unwrap_or_default()
            }
            Ok(text) => {
                self.refine_failures = 0;
                self.set_reason(DegradeReason::RefineFailing, false);
                text
            }
            Err(_) => {
                self.refine_failures += 1;
                Stats::add(&self.w.stats.refine_fallbacks, 1);
                if self.refine_failures >= self.p.refine_failures_to_degrade {
                    self.set_reason(DegradeReason::RefineFailing, true);
                }
                self.clauses
                    .get(&clause)
                    .map(|i| i.source_text.clone())
                    .unwrap_or_default()
            }
        };
        self.recognized(clause, text);
    }

    fn translation_started(&mut self, clause: ClauseId) {
        if self.clause_is_live(clause) {
            self.apply_clause(clause, ClauseEventKind::TranslationStarted);
        } else {
            self.drop_late();
        }
    }

    fn translation_text(&mut self, clause: ClauseId, delta: &str) {
        if !self.clause_is_live(clause) {
            self.drop_late();
            return;
        }
        if let Some(c) = self.captions.iter_mut().find(|c| c.clause == clause) {
            c.translation.push_str(delta);
        }
        if let Some(info) = self.clauses.get_mut(&clause) {
            info.target_text.push_str(delta);
        }
    }

    fn translation_done(&mut self, clause: ClauseId) {
        match self.book.clause_state(clause) {
            Some(ClauseState::Translating) => {
                self.apply_clause(clause, ClauseEventKind::Translated);
            }
            // Early synthesis: the first target clause is already speaking.
            Some(ClauseState::Synthesizing | ClauseState::Playing) => {}
            _ => self.drop_late(),
        }
    }

    fn synthesis_started(&mut self, clause: ClauseId, voice: VoiceSelection) {
        if !self.clause_is_live(clause) {
            self.drop_late();
            return;
        }
        let fallback = voice != self.p.voice;
        if let Some(info) = self.clauses.get_mut(&clause) {
            info.voice = Some(voice);
        }
        if self.apply_clause(clause, ClauseEventKind::SynthesisStarted) {
            self.set_reason(DegradeReason::FallbackVoice, fallback);
        }
    }

    /// A clause failed. With `escalate` the failure is the provider's and the
    /// direction's failure policy applies; without it only the clause is lost.
    pub(super) fn fail_clause(
        &mut self,
        clause: ClauseId,
        stage: Stage,
        failure: &StageFailure,
        escalate: bool,
    ) {
        if !self.clause_is_live(clause) {
            self.drop_late();
            return;
        }
        self.apply_clause(clause, ClauseEventKind::Failed(stage));
        self.last_error = Some(ErrorSnapshot {
            stage,
            fault: failure.fault,
            at: self.w.tracer.now(),
            detail: failure.detail.clone(),
        });
        if escalate {
            self.direction_event(DirectionEvent::StageFailed {
                stage,
                fault: failure.fault,
            });
        }
    }
}
