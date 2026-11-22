//! Recognition stage: voice activity, segment boundaries and streaming recognition, driven frame by frame, committing clauses by the commit policy.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{AudioFrame, ClauseId, Envelope, MonoTime, SampleRate, SegmentId};
use prxvc_ports::{
    AsrError, AsrHypothesis, AsrStream, CancellationToken, SegmentEnd, TraceStage, VadEngine,
};

use super::align::{Alignment, align_after_committed};
use super::faults::{asr_failure, vad_failure};
use super::messages::{Commit, RecognitionEvent, StageFailure};
use super::params::PipelineParams;
use super::segmenter::Segmenter;
use super::trace::Tracer;
use super::tree::TokenTree;
use crate::commit_policy::{
    ClauseReason, CommitDecision, CutPoint, RecognitionInput, decide_recognition_commit,
    is_clause_sized,
};
use crate::fault::Stage;
use crate::filler::is_pure_filler;
use crate::pipeline::snapshot::Stats;

mod clips;
mod early;

use clips::{advance_past, clip_audio, trim_clips};
use early::{floor_boundary, stable_cut};

/// Stands in for the unknown text of a clause when the recogniser gives finals only: it makes the pause rule fire once speech has been heard.
const UNKNOWN_TEXT: &str = "x x";

/// A fault of the recognition stage: which stage and what went wrong.
#[derive(Debug)]
pub(crate) struct RecognitionFault {
    pub(crate) stage: Stage,
    pub(crate) failure: StageFailure,
}

type Out = Vec<Envelope<RecognitionEvent>>;

/// A VAD frame cut from the incoming stream.
struct VadFrame {
    ts: MonoTime,
    samples: Vec<f32>,
}

/// Cuts the incoming frames (any length) into the fixed frames the VAD engine scores.
struct Reframer {
    len: usize,
    buf: Vec<f32>,
    ts: MonoTime,
    rate: SampleRate,
}

impl Reframer {
    fn push(&mut self, frame: &AudioFrame, out: &mut Vec<VadFrame>) {
        if self.buf.is_empty() {
            self.ts = frame.ts();
        }
        self.buf.extend_from_slice(frame.samples());
        while self.buf.len() >= self.len {
            let samples: Vec<f32> = self.buf.drain(..self.len).collect();
            let ts = self.ts;
            self.ts = ts.saturating_add(self.rate.duration_of(self.len));
            out.push(VadFrame { ts, samples });
        }
    }

    fn clear(&mut self) {
        self.buf.clear();
    }
}

/// The recogniser's latest reading of the open clause, relative to its round.
#[derive(Clone)]
struct Hyp {
    text: String,
    stable_prefix_len: usize,
    audio_end: MonoTime,
}

struct OpenSegment {
    id: SegmentId,
    token: CancellationToken,
    onset: MonoTime,
    /// Increases with every hypothesis and is the revision of every event of the segment.
    revision: u32,
    clause_index: u16,
    last_commit_ts: MonoTime,
    /// Bytes of the current round's text that are already committed.
    committed_bytes: usize,
    /// The committed text, to check that the recogniser keeps its promise.
    committed_prefix: String,
    /// Text of a clause that was too short to commit, joining the next one.
    carry: String,
    /// A unit of this round was committed from the stable prefix: the audio positions of the cuts
    /// are not known, so what follows in the round is not given to the refine pass.
    early_round: bool,
    hyp: Option<Hyp>,
    partial_seen: bool,
    tail: String,
    first_partial_traced: bool,
    pause_traced: bool,
    clips: VecDeque<(MonoTime, Vec<f32>)>,
}

impl OpenSegment {
    fn clause(&self) -> ClauseId {
        ClauseId::new(self.id, self.clause_index)
    }

    fn reset_round(&mut self) {
        self.early_round = false;
        self.committed_bytes = 0;
        self.committed_prefix.clear();
        self.hyp = None;
        self.partial_seen = false;
        self.tail.clear();
    }
}

/// The recognition stage of one direction. Pure with respect to threads: the
/// caller feeds frames and sends the returned events on.
pub(crate) struct RecognitionStage {
    p: PipelineParams,
    tree: Arc<TokenTree>,
    tracer: Tracer,
    stats: Arc<Stats>,
    vad: Box<dyn VadEngine>,
    asr: Box<dyn AsrStream>,
    segmenter: Segmenter,
    reframer: Reframer,
    preroll: VecDeque<(MonoTime, Duration, Vec<f32>)>,
    preroll_span: Duration,
    open: Option<OpenSegment>,
    next_seq: u64,
    now: MonoTime,
}

impl RecognitionStage {
    pub(crate) fn new(
        p: &PipelineParams,
        tree: Arc<TokenTree>,
        tracer: Tracer,
        stats: Arc<Stats>,
        vad: Box<dyn VadEngine>,
        asr: Box<dyn AsrStream>,
    ) -> Self {
        let rate = vad.rate();
        let reframer = Reframer {
            len: vad.frame_len().max(1),
            buf: Vec::new(),
            ts: MonoTime::ZERO,
            rate,
        };
        let frame_span = rate.duration_of(vad.frame_len());
        Self {
            segmenter: Segmenter::new(
                p.vad_threshold,
                p.vad_release_threshold,
                p.min_speech,
                p.pad,
                p.min_speech_dbfs,
            ),
            preroll_span: p.min_speech + p.pad + frame_span * 2,
            p: p.clone(),
            tree,
            tracer,
            stats,
            vad,
            asr,
            reframer,
            preroll: VecDeque::new(),
            open: None,
            next_seq: 1,
            now: MonoTime::ZERO,
        }
    }

    /// Start over on the same engines (the direction resumed after a
    /// suspension). A segment that was open is not reported: the machine
    /// cancelled it.
    pub(crate) fn reset(&mut self) {
        self.asr.reset();
        self.vad.reset();
        self.segmenter.reset();
        self.reframer.clear();
        self.preroll.clear();
        self.open = None;
    }

    /// Process one frame; events for the control thread are appended to `out`.
    pub(crate) fn on_frame(
        &mut self,
        frame: &AudioFrame,
        out: &mut Out,
    ) -> Result<(), RecognitionFault> {
        if frame.rate() != self.vad.rate() {
            return Err(RecognitionFault {
                stage: Stage::Vad,
                failure: StageFailure::new(
                    crate::fault::FaultKind::Unavailable,
                    format!(
                        "frames are {}, the VAD expects {}",
                        frame.rate(),
                        self.vad.rate()
                    ),
                ),
            });
        }
        self.now = frame.end_ts();
        if frame.discontinuity() {
            self.reframer.clear();
            self.segmenter.discontinuity();
        }
        self.push_preroll(frame);
        let onset = self.score(frame)?;
        let mut just_opened = false;
        if let (Some(onset), true) = (onset, self.open.is_none()) {
            self.open_segment(onset, out)?;
            just_opened = true;
        }
        if self.open.as_ref().is_some_and(|o| o.token.is_cancelled()) {
            self.abandon();
            return Ok(());
        }
        if self.open.is_some() {
            if !just_opened {
                self.feed(frame.ts(), frame.samples(), out)?;
            }
            self.evaluate(out)?;
        }
        Ok(())
    }

    fn push_preroll(&mut self, frame: &AudioFrame) {
        self.preroll
            .push_back((frame.ts(), frame.duration(), frame.samples().to_vec()));
        let mut span: Duration = self.preroll.iter().map(|(_, d, _)| *d).sum();
        while span > self.preroll_span && self.preroll.len() > 1 {
            if let Some((_, d, _)) = self.preroll.pop_front() {
                span = span.saturating_sub(d);
            }
        }
    }

    /// Score the frame's VAD frames; returns the onset when speech starts.
    fn score(&mut self, frame: &AudioFrame) -> Result<Option<MonoTime>, RecognitionFault> {
        let mut vad_frames = Vec::new();
        self.reframer.push(frame, &mut vad_frames);
        let dur = self.vad.rate().duration_of(self.reframer.len);
        let mut onset = None;
        for f in vad_frames {
            let p = self
                .vad
                .probability(&f.samples)
                .map_err(|e| RecognitionFault {
                    stage: Stage::Vad,
                    failure: vad_failure(&e, true),
                })?;
            let power =
                f.samples.iter().map(|s| s * s).sum::<f32>() / f.samples.len().max(1) as f32;
            if let Some(o) = self.segmenter.update(p, power, f.ts, dur) {
                onset = Some(o);
            }
        }
        Ok(onset)
    }

    fn open_segment(&mut self, onset: MonoTime, out: &mut Out) -> Result<(), RecognitionFault> {
        if !self.begin_segment(onset, VecDeque::new(), out) {
            return Ok(());
        }
        let backlog: Vec<(MonoTime, Duration, Vec<f32>)> = self
            .preroll
            .iter()
            .filter(|(ts, d, _)| ts.saturating_add(*d) > onset)
            .cloned()
            .collect();
        for (ts, _, samples) in backlog {
            self.feed(ts, &samples, out)?;
        }
        Ok(())
    }

    /// Open the next segment of the direction at `onset` and announce it. `clips` is the audio the
    /// refine pass may still need. Returns false when no segment id is left.
    fn begin_segment(
        &mut self,
        onset: MonoTime,
        clips: VecDeque<(MonoTime, Vec<f32>)>,
        out: &mut Out,
    ) -> bool {
        let Some(id) = SegmentId::new(self.p.direction, self.next_seq) else {
            return false;
        };
        self.next_seq += 1;
        let token = self.tree.open_segment(id);
        let open = OpenSegment {
            id,
            token,
            onset,
            revision: 0,
            clause_index: 0,
            last_commit_ts: onset,
            committed_bytes: 0,
            committed_prefix: String::new(),
            carry: String::new(),
            early_round: false,
            hyp: None,
            partial_seen: false,
            tail: String::new(),
            first_partial_traced: false,
            pause_traced: false,
            clips,
        };
        self.tracer.mark(open.clause(), TraceStage::SpeechStart);
        out.push(Envelope::new(id, 0, onset, RecognitionEvent::SpeechStarted));
        self.open = Some(open);
        true
    }

    /// Give audio to the recogniser and take the hypothesis it returns.
    fn feed(
        &mut self,
        ts: MonoTime,
        samples: &[f32],
        out: &mut Out,
    ) -> Result<(), RecognitionFault> {
        let hyp = self.asr.feed(samples, ts).map_err(|e| asr_fault(&e))?;
        let two_pass = self.p.refines();
        let keep = self.p.commit.max_clause + self.p.pad * 2 + Duration::from_secs(1);
        let Some(open) = self.open.as_mut() else {
            return Ok(());
        };
        if two_pass {
            open.clips.push_back((ts, samples.to_vec()));
            trim_clips(&mut open.clips, keep, self.reframer.rate);
        }
        if let Some(h) = hyp {
            if h.ended_by == Some(SegmentEnd::MaxLength) {
                self.on_forced_end(&h, out);
            } else {
                self.on_hypothesis(h.text, h.stable_prefix_len, h.audio_end, out);
            }
        }
        Ok(())
    }

    /// The recogniser ended the segment itself at its maximum length: `h` is the final of the audio
    /// before the cut and the audio after it is already the start of the recogniser's next segment.
    /// The remainder of the cut part is committed as the last clause of the open segment, the
    /// segment ends, and the speech that goes on opens a new segment with a new id at the cut.
    fn on_forced_end(&mut self, h: &AsrHypothesis, out: &mut Out) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.revision += 1;
        let tail = committed_remainder(open, h, &self.stats, &self.tracer);
        let text = join_carry(&open.carry, tail.trim());
        let refinable = !open.early_round;
        // The recogniser's round continues with the audio after the cut: it needs no reset.
        open.reset_round();
        let end = h.audio_end.min(self.now);
        let clause = open.clause();
        if !text.trim().is_empty() {
            self.emit_commit(text, end, refinable, out);
        }
        Stats::add(&self.stats.forced_cuts, 1);
        self.tracer.mark(clause, TraceStage::SegmentCut);
        let Some(mut old) = self.open.take() else {
            return;
        };
        out.push(Envelope::new(
            old.id,
            old.revision,
            old.onset,
            RecognitionEvent::EndSegment,
        ));
        let clips = std::mem::take(&mut old.clips);
        self.begin_segment(end, clips, out);
    }

    fn on_hypothesis(&mut self, text: String, stable: usize, audio_end: MonoTime, out: &mut Out) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.revision += 1;
        let mut committed = open.committed_bytes.min(text.len());
        if open.committed_bytes > 0 && !text.starts_with(&open.committed_prefix) {
            Stats::add(&self.stats.stable_violations, 1);
            while !text.is_char_boundary(committed) {
                committed -= 1;
            }
            open.committed_bytes = committed;
            open.committed_prefix = text[..committed].to_owned();
        }
        let tail = text[committed..].to_owned();
        let stable_in_tail = floor_boundary(&tail, stable.saturating_sub(committed));
        open.tail.clone_from(&tail);
        open.partial_seen = true;
        open.hyp = Some(Hyp {
            text,
            stable_prefix_len: stable,
            audio_end,
        });
        if !open.first_partial_traced {
            open.first_partial_traced = true;
            self.tracer.mark(open.clause(), TraceStage::AsrPartial);
        }
        out.push(Envelope::new(
            open.id,
            open.revision,
            open.onset,
            RecognitionEvent::Hypothesis {
                text: tail,
                stable_prefix_len: stable_in_tail,
            },
        ));
    }

    /// Apply the commit rule to the state after this frame.
    fn evaluate(&mut self, out: &mut Out) -> Result<(), RecognitionFault> {
        let Some(open) = self.open.as_mut() else {
            return Ok(());
        };
        let silence = self.segmenter.silence();
        if silence.is_zero() {
            open.pause_traced = false;
        } else if silence >= self.p.commit.clause_pause && !open.pause_traced {
            open.pause_traced = true;
            self.tracer.mark(open.clause(), TraceStage::ClausePause);
        }
        // Text still waiting after a commit from the stable prefix is speech that has not been
        // committed, whatever the end of the last commit says.
        let heard_speech = self.segmenter.last_speech_end() > open.last_commit_ts
            || (open.committed_bytes > 0 && !open.tail.trim().is_empty());
        let joined;
        let text: &str = if !open.partial_seen {
            if heard_speech || !open.carry.is_empty() {
                UNKNOWN_TEXT
            } else {
                ""
            }
        } else if !heard_speech && open.carry.is_empty() {
            // No speech since the last commit: text that still shows up is a
            // late echo of what was committed, not a new clause.
            ""
        } else if open.carry.is_empty() {
            &open.tail
        } else {
            joined = format!("{} {}", open.carry, open.tail);
            &joined
        };
        let input = RecognitionInput {
            silence,
            uncommitted_text: text,
            uncommitted_audio: self.now.saturating_since(open.last_commit_ts),
            segment_audio: self.now.saturating_since(open.onset),
            has_stable_word_boundary: stable_cut(open).is_some(),
        };
        let waited = input.uncommitted_audio;
        match decide_recognition_commit(&self.p.commit, &input) {
            CommitDecision::Wait => {
                self.commit_early(waited, out);
                Ok(())
            }
            CommitDecision::CommitClause(reason) => self.commit_clause(reason, out),
            CommitDecision::EndSegment { .. } => self.end_segment(out),
        }
    }

    fn commit_clause(
        &mut self,
        reason: ClauseReason,
        out: &mut Out,
    ) -> Result<(), RecognitionFault> {
        let stable = matches!(reason, ClauseReason::LengthCut(CutPoint::LastStableWord));
        if stable && self.commit_stable(out) {
            return Ok(());
        }
        self.commit_final(false, out)
    }

    /// Commit the stable part of the hypothesis without ending the round.
    /// Returns whether a clause was committed.
    fn commit_stable(&mut self, out: &mut Out) -> bool {
        let Some(cut) = self.open.as_ref().and_then(stable_cut) else {
            return false;
        };
        self.commit_prefix(cut, false, out)
    }

    /// Commit the hypothesis text up to byte `cut` as a clause, without ending the round.
    /// Returns whether a clause was committed.
    fn commit_prefix(&mut self, cut: usize, early: bool, out: &mut Out) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let Some(hyp) = open.hyp.clone() else {
            return false;
        };
        if cut <= open.committed_bytes || cut > hyp.text.len() {
            return false;
        }
        let piece = hyp.text[open.committed_bytes..cut].trim().to_owned();
        let text = join_carry(&open.carry, &piece);
        if !is_clause_sized(&self.p.commit, &text) {
            return false;
        }
        open.committed_prefix = hyp.text[..cut].to_owned();
        open.committed_bytes = cut;
        open.tail = hyp.text[cut..].to_owned();
        let end = hyp.audio_end.min(self.now);
        let refinable = !open.early_round && !early;
        open.early_round |= early;
        self.emit_commit(text, end, refinable, out);
        true
    }

    /// Finalise the round, commit its remaining text as a clause and start the next round.
    fn commit_final(&mut self, ending: bool, out: &mut Out) -> Result<(), RecognitionFault> {
        let Some(open) = self.open.as_mut() else {
            return Ok(());
        };
        let fin = match self.asr.finalize(&open.token) {
            Ok(h) => h,
            Err(AsrError::Cancelled) => {
                self.abandon();
                return Ok(());
            }
            Err(e) => return Err(asr_fault(&e)),
        };
        let tail = committed_remainder(open, &fin, &self.stats, &self.tracer);
        let text = join_carry(&open.carry, tail.trim());
        let refinable = !open.early_round;
        self.asr.reset();
        open.reset_round();
        let end = self.segmenter.last_speech_end().min(self.now);
        if text.trim().is_empty() {
            open.carry.clear();
            if !ending {
                open.last_commit_ts = end.max(open.last_commit_ts);
            }
            return Ok(());
        }
        // A filler in the middle of a segment (more speech may follow in it) is not dropped: it
        // waits as a carry and joins the next unit, so that a word the recogniser left alone
        // between two pauses ("the", "和", "to") is not lost. Only a filler that is the whole
        // segment or its trailing fragment is dropped, when the segment ends (`emit_commit`). A
        // second filler while one already waits is dropped on the spot, so that echo noise does
        // not pile up in front of the next real unit; every dropped unit is counted once.
        let filler = self.p.drop_filler && is_pure_filler(self.p.source, &text);
        if !ending && (filler || !is_clause_sized(&self.p.commit, &text)) {
            let waiting_filler = self.p.drop_filler
                && !open.carry.is_empty()
                && is_pure_filler(self.p.source, &open.carry);
            if filler && waiting_filler && !tail.trim().is_empty() {
                Stats::add(&self.stats.filler_dropped, 1);
                self.tracer
                    .mark_text(open.clause(), TraceStage::FillerDropped, || tail.trim().to_owned());
                return Ok(());
            }
            open.carry = text;
            return Ok(());
        }
        self.emit_commit(text, end, refinable, out);
        Ok(())
    }

    /// Emit a committed clause. With `refinable` and a refine pass configured, its audio goes along.
    /// A unit that is only filler is not emitted: it is counted and traced, and the stage moves on
    /// as if it had been committed, so that it is not decoded again.
    fn emit_commit(&mut self, text: String, end: MonoTime, refinable: bool, out: &mut Out) {
        if self.p.drop_filler && is_pure_filler(self.p.source, &text) {
            self.drop_filler(&text, end);
            return;
        }
        let two_pass = self.p.refines() && refinable;
        let pad = self.p.pad;
        let rate = self.reframer.rate;
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let start = open.last_commit_ts;
        let end = end.max(start);
        let audio = two_pass.then(|| Arc::new(clip_audio(&open.clips, start, end, pad, rate)));
        out.push(Envelope::new(
            open.id,
            open.revision,
            open.onset,
            RecognitionEvent::Commit(Commit {
                text,
                source_start: start,
                source_end: end,
                audio,
            }),
        ));
        open.clause_index = open.clause_index.saturating_add(1);
        advance_past(open, end, pad);
    }

    /// Count and trace a unit that was only filler. The clause index stays: the next real unit
    /// takes the place the filler would have had.
    fn drop_filler(&mut self, text: &str, end: MonoTime) {
        let pad = self.p.pad;
        let Some(open) = self.open.as_mut() else {
            return;
        };
        Stats::add(&self.stats.filler_dropped, 1);
        self.tracer
            .mark_text(open.clause(), TraceStage::FillerDropped, || text.to_owned());
        advance_past(open, end.max(open.last_commit_ts), pad);
    }

    /// The segment-end pause or the length limit: commit what is left and close the segment.
    fn end_segment(&mut self, out: &mut Out) -> Result<(), RecognitionFault> {
        self.commit_final(true, out)?;
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        out.push(Envelope::new(
            open.id,
            open.revision,
            open.onset,
            RecognitionEvent::EndSegment,
        ));
        self.segmenter.end_segment();
        Ok(())
    }

    /// End the open segment now (a graceful stop): what is left is committed as the final clause.
    pub(crate) fn finish_open(&mut self, out: &mut Out) -> Result<(), RecognitionFault> {
        if self.open.is_some() {
            self.end_segment(out)?;
        }
        Ok(())
    }

    /// The segment was cancelled: drop the recogniser's state and ignore the
    /// rest of the utterance (the book already cancelled the segment).
    fn abandon(&mut self) {
        self.asr.reset();
        self.segmenter.abandon();
        self.open = None;
    }
}

/// The part of a final that is not committed yet. Committed text stays as committed and is never
/// taken from the final again: the remainder is found by content ([`align_after_committed`]). A
/// final that disagrees with the committed text is counted and traced, never patched into what was
/// committed and never spoken again.
fn committed_remainder(
    open: &OpenSegment,
    fin: &AsrHypothesis,
    stats: &Stats,
    tracer: &Tracer,
) -> String {
    if fin.contradicts_stable {
        Stats::add(&stats.finals_contradicting_stable, 1);
    }
    let aligned = align_after_committed(&open.committed_prefix, &fin.text);
    if aligned.alignment != Alignment::Agrees {
        Stats::add(&stats.final_rewrites, 1);
        if aligned.alignment == Alignment::Unaligned {
            Stats::add(&stats.final_unaligned, 1);
        }
        tracer.mark_text(open.clause(), TraceStage::FinalDisagrees, || {
            format!("{} | {}", open.committed_prefix, fin.text)
        });
    }
    fin.text[aligned.start..].to_owned()
}

fn asr_fault(e: &AsrError) -> RecognitionFault {
    RecognitionFault {
        stage: Stage::Asr,
        failure: asr_failure(e, true),
    }
}

fn join_carry(carry: &str, text: &str) -> String {
    if carry.is_empty() {
        text.to_owned()
    } else if text.is_empty() {
        carry.to_owned()
    } else {
        format!("{carry} {text}")
    }
}
