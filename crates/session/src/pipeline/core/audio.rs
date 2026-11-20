//! Holds synthesised audio until its clause is next in playback order, and tracks playback feedback.

use std::time::Duration;

use prxvc_media::{ClauseId, Envelope, MonoTime, PcmChunk};
use prxvc_ports::TraceStage;

use super::{ClauseInfo, Core, Held};
use crate::fault::{FaultKind, Stage};
use crate::pipeline::events::{ClauseAudio, PipelineEvent};
use crate::pipeline::messages::StageFailure;
use crate::pipeline::snapshot::Stats;
use crate::segment::{ClauseEventKind, ClauseState};

impl Core {
    /// A chunk of synthesised audio: forward it when its clause is playing, otherwise hold it.
    pub(super) fn on_chunk(&mut self, clause: ClauseId, chunk: PcmChunk) {
        if !self.clause_is_live_for_audio(clause) {
            // Through the book, so that a result for a finished clause is counted there.
            if self.book.clause_state(clause).is_some() {
                self.apply_clause(clause, ClauseEventKind::AudioReady);
            } else {
                Stats::add(&self.w.stats.late_dropped, 1);
            }
            return;
        }
        let Some(info) = self.clauses.get_mut(&clause) else {
            Stats::add(&self.w.stats.late_dropped, 1);
            return;
        };
        info.chunks += 1;
        info.synthesised += chunk.duration();
        let first = info.chunks == 1;
        let audio = ClauseAudio {
            clause,
            voice: info.voice.clone().unwrap_or_else(|| self.p.voice.clone()),
            chunk,
        };
        if info.released {
            self.send_audio(clause, audio);
        } else if !self.hold(clause, audio) {
            return;
        }
        if first {
            self.apply_clause(clause, ClauseEventKind::AudioReady);
        }
    }

    fn clause_is_live_for_audio(&self, clause: ClauseId) -> bool {
        matches!(
            self.book.clause_state(clause),
            Some(ClauseState::Synthesizing | ClauseState::Playing)
        )
    }

    /// Keep a chunk of a clause that is not next in order. A clause that
    /// would make more than the allowed audio wait is failed instead.
    fn hold(&mut self, clause: ClauseId, audio: ClauseAudio) -> bool {
        let cap = self.p.queues.hold.as_millis();
        let ms = audio.chunk.duration().as_millis();
        let total: u128 = self.clauses.values().map(|i| i.held_ms as u128).sum();
        if total + ms > cap {
            Stats::add(&self.w.stats.hold_overflows, 1);
            let failure =
                StageFailure::new(FaultKind::Unavailable, "too much audio waits for its turn");
            self.fail_clause(clause, Stage::Tts, &failure, false);
            return false;
        }
        if let Some(info) = self.clauses.get_mut(&clause) {
            info.held_ms += usize::try_from(ms).unwrap_or(usize::MAX);
            info.held.push_back(Held::Chunk(audio));
        }
        true
    }

    /// Synthesis of a clause is complete.
    pub(super) fn on_tts_done(&mut self, clause: ClauseId) {
        let Some(info) = self.clauses.get_mut(&clause) else {
            Stats::add(&self.w.stats.late_dropped, 1);
            return;
        };
        if !matches!(
            self.book.clause_state(clause),
            Some(ClauseState::Synthesizing | ClauseState::Playing)
        ) {
            Stats::add(&self.w.stats.late_dropped, 1);
            return;
        }
        info.tts_done = true;
        if info.chunks == 0 {
            let failure = StageFailure::new(FaultKind::Crashed, "synthesis produced no audio");
            self.fail_clause(clause, Stage::Tts, &failure, false);
        } else if info.released {
            self.send_audio_end(clause);
        } else {
            info.held.push_back(Held::End);
        }
    }

    /// The book named `clause` next in playback order: hand out its held audio.
    pub(super) fn release(&mut self, clause: ClauseId) {
        let Some(info) = self.clauses.get_mut(&clause) else {
            return;
        };
        info.released = true;
        info.held_ms = 0;
        let held: Vec<Held> = info.held.drain(..).collect();
        for h in held {
            match h {
                Held::Chunk(audio) => self.send_audio(clause, audio),
                Held::End => self.send_audio_end(clause),
            }
        }
    }

    fn envelope<T>(&self, info: &ClauseInfo, clause: ClauseId, payload: T) -> Envelope<T> {
        Envelope::new(clause.segment(), info.revision, info.source_ts, payload)
    }

    fn send_audio(&mut self, clause: ClauseId, audio: ClauseAudio) {
        let Some(info) = self.clauses.get(&clause) else {
            return;
        };
        let envelope = self.envelope(info, clause, audio);
        self.emit(PipelineEvent::Audio(envelope));
    }

    fn send_audio_end(&mut self, clause: ClauseId) {
        let Some(info) = self.clauses.get(&clause) else {
            return;
        };
        let envelope = self.envelope(info, clause, clause);
        self.emit(PipelineEvent::AudioEnd(envelope));
    }

    /// The playout side began to play the clause.
    pub(super) fn playback_started(&mut self, clause: ClauseId, at: MonoTime) {
        if self.apply_clause(clause, ClauseEventKind::PlaybackStarted) {
            self.w.tracer.mark_at(clause, TraceStage::PlayoutStart, at);
        }
    }

    /// The playout side finished the clause; the next one is released.
    pub(super) fn playback_finished(&mut self, clause: ClauseId, at: MonoTime) {
        if self.book.clause_state(clause) == Some(ClauseState::Synthesizing) {
            self.playback_started(clause, at);
        }
        if self.book.clause_state(clause) == Some(ClauseState::Playing) {
            self.w.tracer.mark_at(clause, TraceStage::PlayoutEnd, at);
        }
        self.apply_clause(clause, ClauseEventKind::PlaybackFinished);
    }

    /// Audio waiting for its turn plus unsynthesised clauses, for the backlog rule.
    pub(super) fn clause_audio_estimate(&self, clause: ClauseId) -> Duration {
        self.clauses.get(&clause).map_or(Duration::ZERO, |i| {
            if i.tts_done {
                i.synthesised
            } else {
                i.source_len.max(i.synthesised)
            }
        })
    }
}
