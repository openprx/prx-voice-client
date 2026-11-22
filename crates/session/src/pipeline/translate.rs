//! Translation stage: one streamed request per committed clause, with bounded context, feeding synthesis.

use std::collections::VecDeque;
use std::sync::Arc;

use prxvc_media::{ClauseId, Envelope, SegmentId, has_speakable_content, trim_stray_start};
use prxvc_ports::{
    CancellationToken, MtDelta, MtError, MtFlag, MtRequest, MtStream, TraceStage, Translator,
};

use super::continuation::SentenceMemory;
use super::draft_cache::{Cached, CachedStream, DraftCache};
use super::faults::mt_failure;
use super::messages::{
    ClauseJob, ContinuationOutcome, MtItem, PieceJob, StageEvent, StageFailure, TtsItem,
    WorkerEvent,
};
use super::outbox::{Meta, Outbox};
use super::params::PipelineParams;
use super::pieces::PieceSplitter;
use super::queue::{BoundedQueue, Popped, Pushed, Wait};
use super::trace::Tracer;
use crate::commit_policy::{approx_prefix_end, continuation_text, prefix_end};
use crate::fault::{FaultKind, Stage};
use crate::pipeline::snapshot::Stats;

/// Everything the translation thread uses.
pub(crate) struct MtStage {
    pub(crate) mt: Arc<dyn Translator>,
    pub(crate) p: PipelineParams,
    pub(crate) outbox: Outbox,
    pub(crate) input: Arc<BoundedQueue<MtItem>>,
    pub(crate) tts_in: Arc<BoundedQueue<TtsItem>>,
    pub(crate) call: CancellationToken,
    pub(crate) tracer: Tracer,
    pub(crate) stats: Arc<Stats>,
    /// Translations by exact source text, only when clause translations continue the sentence.
    pub(crate) cache: Option<Arc<DraftCache>>,
}

/// How a request ended.
enum Ended {
    Done,
    Cancelled,
    Failed,
    /// The translation started in the wrong language, or opened like an answer or a refusal, and
    /// nothing of it was spoken yet: it can be asked for once more with the strict instruction.
    Retry,
    /// The translation from the start of the sentence no longer began with the part already
    /// spoken, and nothing of it was spoken: the unit is translated on its own instead.
    Diverged,
}

impl MtStage {
    /// The thread body: translate clauses until the queue closes.
    pub(crate) fn run(self) {
        let mut history: VecDeque<(String, String)> = VecDeque::new();
        let mut memory: Option<SentenceMemory> = None;
        let mut seen = 0;
        loop {
            match self.input.pop(Wait::Forever, &mut seen) {
                Popped::Item(MtItem::Clause(job)) => {
                    self.translate_clause(job, &mut history, &mut memory);
                }
                Popped::Item(MtItem::Probe) => self.probe(),
                Popped::Woken | Popped::Timeout => {}
                Popped::Closed => break,
            }
        }
    }

    fn translate_clause(
        &self,
        job: Envelope<ClauseJob>,
        history: &mut VecDeque<(String, String)>,
        memory: &mut Option<SentenceMemory>,
    ) {
        let meta = Meta {
            revision: job.revision,
            source_ts: job.source_ts,
        };
        let ClauseJob {
            clause,
            text,
            token,
            continues,
        } = job.payload;
        // The sentence memory is for this unit only when it is the next one of the chain. It is
        // taken, so a unit that fails, is skipped or is dropped leaves none for the one after it:
        // the chain breaks there and the next unit stands alone.
        let chain = self.p.retranslate.continuation;
        let mut prior = if chain {
            memory.take().filter(|m| m.continues_with(clause))
        } else {
            None
        };
        let mut diverged = false;
        if token.is_cancelled() {
            Stats::add(&self.stats.late_dropped, 1);
            return;
        }
        self.tracer.mark(clause, TraceStage::MtRequestSent);
        if !self
            .outbox
            .stage(clause, meta, &token, StageEvent::MtStarted { clause })
        {
            return;
        }
        // A reply in the wrong language, or one that opens like an answer or a refusal, is caught
        // before any of it is spoken and asked for once more with the strict instruction
        // (`pipeline.retry_wrong_language`). After the retry a reply in the wrong language fails
        // the clause; one that still reads like an answer is spoken and flagged, as before.
        let mut strict = false;
        loop {
            // From the start of the sentence when there is a spoken part to continue, else the
            // unit alone.
            let request_text = prior
                .as_ref()
                .map_or_else(|| text.clone(), |m| m.request_text(&text));
            let request = MtRequest {
                clause,
                source: self.p.source,
                target: self.p.target,
                text: request_text.clone(),
                context: history.iter().cloned().collect(),
                glossary: self.p.glossary.clone(),
                strict_language: strict,
            };
            let stream = match self.open_stream(request) {
                Ok(s) => s,
                Err(e) => {
                    self.fail_open(clause, meta, &token, &e);
                    return;
                }
            };
            let mut run = Run {
                stage: self,
                clause,
                meta,
                token: &token,
                splitter: PieceSplitter::new(self.p.commit, self.p.target),
                full: String::new(),
                spoken: false,
                source: &request_text,
                strict,
                flags: Vec::new(),
                prior_target: prior.as_ref().map(|m| m.target_full().to_owned()),
                buffered: chain && (prior.is_some() || continues),
                continues: chain && continues,
                report: (chain && prior.is_none() && !diverged)
                    .then_some(ContinuationOutcome::Standalone),
                own: String::new(),
            };
            match run.drive(stream) {
                (Ended::Done, spoken) => {
                    if let Some(cache) = &self.cache {
                        cache.put(
                            &request_text,
                            Cached {
                                text: run.full.clone(),
                                flags: run.flags.clone(),
                            },
                        );
                    }
                    history.push_back((text.clone(), spoken));
                    while history.len() > self.p.mt_context_clauses {
                        history.pop_front();
                    }
                    if chain && continues {
                        *memory = Some(SentenceMemory::after(
                            clause,
                            request_text.clone(),
                            run.full.clone(),
                        ));
                    }
                }
                (Ended::Diverged, _) => {
                    let spoken = prior
                        .as_ref()
                        .map(SentenceMemory::target_full)
                        .unwrap_or_default();
                    let now = run.full.as_str();
                    self.tracer
                        .mark_text(clause, TraceStage::ContinuationDiverged, || {
                            format!("{spoken} | {now}")
                        });
                    if !run.event(StageEvent::MtContinuation {
                        clause,
                        outcome: ContinuationOutcome::Diverged,
                    }) {
                        return;
                    }
                    // Nothing spoken is repeated or taken back: the new unit is translated alone,
                    // and the sentence memory starts again from it.
                    prior = None;
                    diverged = true;
                    continue;
                }
                (Ended::Retry, _) if !strict => {
                    Stats::add(&self.stats.mt_retries, 1);
                    if !run.event(StageEvent::MtRetry { clause }) {
                        return;
                    }
                    strict = true;
                    continue;
                }
                (Ended::Retry, _) => {
                    run.fail(
                        StageFailure::new(
                            FaultKind::Rejected,
                            "suspect translation: wrong language after a retry",
                        ),
                        true,
                    );
                }
                _ => {}
            }
            return;
        }
    }

    /// The stream of a request: the cached translation of exactly its text when there is one (never
    /// for the retry with the strict instruction, which must ask the engine again), else the engine's.
    fn open_stream(&self, request: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        if !request.strict_language
            && let Some(hit) = self.cache.as_ref().and_then(|c| c.get(&request.text))
        {
            return Ok(Box::new(CachedStream::new(hit)));
        }
        self.mt.translate(request)
    }

    fn fail_open(&self, clause: ClauseId, meta: Meta, token: &CancellationToken, e: &MtError) {
        if e.is_cancelled() || token.is_cancelled() {
            return;
        }
        let failure = mt_failure(e, false);
        self.outbox.stage(
            clause,
            meta,
            token,
            StageEvent::MtFailed {
                clause,
                failure,
                suspect: false,
            },
        );
    }

    /// Translate the probe text to see whether the provider works again.
    fn probe(&self) {
        let clause = ClauseId::new(SegmentId::first(self.p.direction), u16::MAX);
        let request = MtRequest {
            clause,
            source: self.p.source,
            target: self.p.target,
            text: self.p.probe_text.clone(),
            context: Vec::new(),
            glossary: Vec::new(),
            strict_language: false,
        };
        let result = self
            .mt
            .translate(request)
            .map_err(|e| mt_failure(&e, false))
            .and_then(|mut s| {
                loop {
                    match s.next(&self.call) {
                        Some(Ok(MtDelta::Done { .. })) if s.suspect().is_none() => return Ok(()),
                        Some(Ok(MtDelta::Done { .. })) => {
                            return Err(StageFailure::new(
                                FaultKind::Rejected,
                                "probe was flagged",
                            ));
                        }
                        Some(Ok(MtDelta::Text(_))) => {}
                        Some(Err(e)) => return Err(mt_failure(&e, true)),
                        None => {
                            return Err(StageFailure::new(FaultKind::Crashed, "probe ended early"));
                        }
                    }
                }
            });
        self.outbox.worker(WorkerEvent::ProbeResult {
            stage: Stage::Mt,
            result,
        });
    }
}

/// One request in flight.
struct Run<'a> {
    stage: &'a MtStage,
    clause: ClauseId,
    meta: Meta,
    token: &'a CancellationToken,
    splitter: PieceSplitter,
    full: String,
    /// A piece has been handed to synthesis: from then on the text is committed to being spoken.
    spoken: bool,
    /// The clause's source text.
    source: &'a str,
    /// This is the retry with the strict instruction.
    strict: bool,
    /// The quality flags of the finished translation.
    flags: Vec<MtFlag>,
    /// The translation of the part of the sentence already spoken, when this request is the
    /// sentence from its start.
    prior_target: Option<String>,
    /// The translation is read to its end before anything is spoken, because what is spoken is
    /// found only in the whole of it (the part after the spoken prefix, or the text with its
    /// closing mark turned into a comma). The system engine delivers a unit in one piece anyway.
    buffered: bool,
    /// The unit does not end its source sentence: its closing mark is spoken as a comma.
    continues: bool,
    /// How the unit related to its sentence, sent before `MtDone`.
    report: Option<ContinuationOutcome>,
    /// The unit's own text of a buffered translation (what was spoken); empty when unbuffered.
    own: String,
}

impl Run<'_> {
    fn event(&self, event: StageEvent) -> bool {
        self.stage
            .outbox
            .stage(self.clause, self.meta, self.token, event)
    }

    fn fail(&self, failure: StageFailure, suspect: bool) -> Ended {
        self.event(StageEvent::MtFailed {
            clause: self.clause,
            failure,
            suspect,
        });
        Ended::Failed
    }

    /// Whether `piece` has anything to say aloud. One that has not (a lone mark between two
    /// clauses) is counted and never sent to the synthesiser.
    fn speakable(&self, piece: &str) -> bool {
        let ok = has_speakable_content(piece);
        if !ok {
            Stats::add(&self.stage.stats.unspeakable_dropped, 1);
        }
        ok
    }

    /// Whether the translation so far is in the wrong language and nothing of it has been spoken, so
    /// that it can still be asked for again.
    fn wrong_language_unspoken(&self) -> bool {
        self.stage.p.retry_wrong_language
            && !self.spoken
            && prxvc_media::script::is_wrong_language_for(&self.full, self.stage.p.target)
    }

    /// Whether the translation so far opens like a chat answer or a refusal (the model replied to
    /// the message instead of translating it) and nothing of it has been spoken, so that it can
    /// still be asked for again. Only on the first attempt: the retry's answer is used.
    fn answer_unspoken(&self) -> bool {
        self.stage.p.retry_wrong_language
            && !self.strict
            && !self.spoken
            && (prxvc_media::answer::opens_like_answer(self.source, &self.full)
                || prxvc_media::answer::opens_like_refusal(self.source, &self.full))
    }

    fn piece(&self, text: String, end: bool) -> bool {
        let job = Envelope::new(
            self.clause.segment(),
            self.meta.revision,
            self.meta.source_ts,
            PieceJob {
                clause: self.clause,
                text,
                token: self.token.clone(),
            },
        );
        let item = if end {
            TtsItem::End(job)
        } else {
            TtsItem::Piece(job)
        };
        matches!(
            self.stage.tts_in.push(item, Some(self.token)),
            Pushed::Accepted | Pushed::Evicted(_)
        )
    }

    /// Read the stream to its end, handing pieces to synthesis as the clause rule allows.
    fn drive(&mut self, mut stream: Box<dyn MtStream>) -> (Ended, String) {
        let mut first = true;
        loop {
            let item = stream.next(self.token);
            if let Some(reason) = stream.suspect() {
                return (self.suspect(&reason), String::new());
            }
            match item {
                Some(Ok(MtDelta::Text(delta))) => {
                    if first {
                        first = false;
                        self.stage
                            .tracer
                            .mark(self.clause, TraceStage::MtFirstToken);
                    }
                    self.full.push_str(&delta);
                    if !self.buffered
                        && let Some(ended) = self.emit_text(delta)
                    {
                        return (ended, String::new());
                    }
                }
                Some(Ok(MtDelta::Done { .. })) => {
                    let ended = if self.buffered {
                        self.finish_buffered()
                    } else {
                        self.finish()
                    };
                    if matches!(ended, Ended::Done) {
                        self.flags = stream.flags();
                        self.report_flags();
                    }
                    let spoken = if self.buffered {
                        self.own.clone()
                    } else {
                        self.full.clone()
                    };
                    return (ended, spoken);
                }
                Some(Err(e)) if e.is_cancelled() || self.token.is_cancelled() => {
                    return (Ended::Cancelled, String::new());
                }
                Some(Err(e)) => return (self.fail(mt_failure(&e, true), false), String::new()),
                None => {
                    let ended = if self.token.is_cancelled() {
                        Ended::Cancelled
                    } else {
                        self.fail(
                            StageFailure::new(FaultKind::Crashed, "translation ended early"),
                            false,
                        )
                    };
                    return (ended, String::new());
                }
            }
        }
    }

    /// Report the quality flags of the finished translation (it was spoken regardless).
    fn report_flags(&self) {
        if self.flags.is_empty() {
            return;
        }
        for flag in &self.flags {
            self.stage.stats.add_flag(*flag);
        }
        self.event(StageEvent::MtFlagged {
            clause: self.clause,
            flags: self.flags.clone(),
        });
    }

    fn suspect(&self, reason: &str) -> Ended {
        self.fail(
            StageFailure::new(
                FaultKind::Rejected,
                format!("suspect translation: {reason}"),
            ),
            true,
        )
    }

    /// Hand a piece of the unit's own text on: to the core as caption text and, cut into pieces, to
    /// synthesis. Returns how the request ended when it cannot go on.
    fn emit_text(&mut self, delta: String) -> Option<Ended> {
        let pieces = self.splitter.push(&delta);
        if !self.event(StageEvent::MtText {
            clause: self.clause,
            delta,
        }) {
            return Some(Ended::Cancelled);
        }
        if !pieces.is_empty() && (self.wrong_language_unspoken() || self.answer_unspoken()) {
            return Some(Ended::Retry);
        }
        for p in pieces {
            self.spoken = true;
            if !self.speakable(&p) {
                continue;
            }
            if !self.piece(p, false) {
                return Some(Ended::Cancelled);
            }
        }
        None
    }

    /// A translation with nothing to say is a failure of the request.
    fn check_complete(&self) -> Option<Ended> {
        if self.full.trim().is_empty() {
            return Some(self.suspect("empty"));
        }
        if !has_speakable_content(&self.full) {
            return Some(self.suspect("nothing to say aloud"));
        }
        None
    }

    fn finish(&mut self) -> Ended {
        if let Some(ended) = self.check_complete() {
            return ended;
        }
        let last = self.splitter.finish();
        if self.wrong_language_unspoken() || self.answer_unspoken() {
            return Ended::Retry;
        }
        self.finish_tail(last)
    }

    /// The end of a buffered translation: the language checks on the whole text, then what is
    /// spoken. With a spoken part to continue, that is what follows it in the whole translation
    /// (the unit's own text must begin with nothing else): nothing speakable after it ends the unit
    /// with no text, and a translation that no longer begins with it is not spoken at all but
    /// reported as diverged unless it is close enough to it (`approx_prefix_end`). A unit that continues its sentence has its closing mark turned into a
    /// comma.
    fn finish_buffered(&mut self) -> Ended {
        if let Some(ended) = self.check_complete() {
            return ended;
        }
        if self.wrong_language_unspoken() || self.answer_unspoken() {
            return Ended::Retry;
        }
        let mut text = self.full.clone();
        if let Some(spoken) = &self.prior_target {
            let mut approx = false;
            let from = match prefix_end(spoken, &self.full) {
                Some(from) => from,
                None => match approx_prefix_end(
                    spoken,
                    &self.full,
                    self.stage.p.retranslate.align_max_ratio,
                ) {
                    Some(from) => {
                        approx = true;
                        from
                    }
                    None => return Ended::Diverged,
                },
            };
            text = trim_stray_start(&self.full[from..]).to_owned();
            if has_speakable_content(&text) {
                self.report = Some(if approx {
                    ContinuationOutcome::ContinuedApprox
                } else {
                    ContinuationOutcome::Continued
                });
                if approx {
                    let full = self.full.as_str();
                    self.stage.tracer.mark_text(
                        self.clause,
                        TraceStage::ContinuationApprox,
                        || format!("{spoken} | {full}"),
                    );
                }
            } else {
                self.report = Some(ContinuationOutcome::Empty);
                return self.finish_tail(None);
            }
        }
        if self.continues {
            text = continuation_text(self.stage.p.target, &text);
        }
        self.own.clone_from(&text);
        if let Some(ended) = self.emit_text(text) {
            return ended;
        }
        let last = self.splitter.finish();
        self.finish_tail(last)
    }

    /// Report the translation as done and send the rest of it, then the end of the clause, to synthesis.
    fn finish_tail(&mut self, last: Option<String>) -> Ended {
        let own = if self.buffered { &self.own } else { &self.full };
        self.stage
            .tracer
            .mark_text(self.clause, TraceStage::MtDone, || own.clone());
        let reported = self.report.is_none_or(|outcome| {
            self.event(StageEvent::MtContinuation {
                clause: self.clause,
                outcome,
            })
        });
        let sent = reported
            && self.event(StageEvent::MtDone {
                clause: self.clause,
            });
        let ok = sent
            && last.is_none_or(|p| !self.speakable(&p) || self.piece(p, false))
            && self.piece(String::new(), true);
        if ok { Ended::Done } else { Ended::Cancelled }
    }
}

#[cfg(test)]
mod tests;
