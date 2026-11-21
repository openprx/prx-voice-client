//! Translation stage: one streamed request per committed clause, with bounded context, feeding synthesis.

use std::collections::VecDeque;
use std::sync::Arc;

use prxvc_media::{ClauseId, Envelope, SegmentId, has_speakable_content};
use prxvc_ports::{
    CancellationToken, MtDelta, MtError, MtRequest, MtStream, TraceStage, Translator,
};

use super::faults::mt_failure;
use super::messages::{
    ClauseJob, MtItem, PieceJob, StageEvent, StageFailure, TtsItem, WorkerEvent,
};
use super::outbox::{Meta, Outbox};
use super::params::PipelineParams;
use super::pieces::PieceSplitter;
use super::queue::{BoundedQueue, Popped, Pushed, Wait};
use super::trace::Tracer;
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
}

/// How a request ended.
enum Ended {
    Done,
    Cancelled,
    Failed,
    /// The translation started in the wrong language, or opened like an answer or a refusal, and
    /// nothing of it was spoken yet: it can be asked for once more with the strict instruction.
    Retry,
}

impl MtStage {
    /// The thread body: translate clauses until the queue closes.
    pub(crate) fn run(self) {
        let mut history: VecDeque<(String, String)> = VecDeque::new();
        let mut seen = 0;
        loop {
            match self.input.pop(Wait::Forever, &mut seen) {
                Popped::Item(MtItem::Clause(job)) => self.translate_clause(job, &mut history),
                Popped::Item(MtItem::Probe) => self.probe(),
                Popped::Woken | Popped::Timeout => {}
                Popped::Closed => break,
            }
        }
    }

    fn translate_clause(&self, job: Envelope<ClauseJob>, history: &mut VecDeque<(String, String)>) {
        let meta = Meta {
            revision: job.revision,
            source_ts: job.source_ts,
        };
        let ClauseJob {
            clause,
            text,
            token,
        } = job.payload;
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
            let request = MtRequest {
                clause,
                source: self.p.source,
                target: self.p.target,
                text: text.clone(),
                context: history.iter().cloned().collect(),
                glossary: self.p.glossary.clone(),
                strict_language: strict,
            };
            let stream = match self.mt.translate(request) {
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
                source: &text,
                strict,
            };
            match run.drive(stream) {
                (Ended::Done, translated) => {
                    history.push_back((text.clone(), translated));
                    while history.len() > self.p.mt_context_clauses {
                        history.pop_front();
                    }
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
                    let pieces = self.splitter.push(&delta);
                    if !self.event(StageEvent::MtText {
                        clause: self.clause,
                        delta,
                    }) {
                        return (Ended::Cancelled, String::new());
                    }
                    if !pieces.is_empty()
                        && (self.wrong_language_unspoken() || self.answer_unspoken())
                    {
                        return (Ended::Retry, String::new());
                    }
                    for p in pieces {
                        self.spoken = true;
                        if !self.speakable(&p) {
                            continue;
                        }
                        if !self.piece(p, false) {
                            return (Ended::Cancelled, String::new());
                        }
                    }
                }
                Some(Ok(MtDelta::Done { .. })) => {
                    let ended = self.finish();
                    if matches!(ended, Ended::Done) {
                        self.report_flags(stream.as_ref());
                    }
                    return (ended, self.full.clone());
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
    fn report_flags(&self, stream: &dyn MtStream) {
        let flags = stream.flags();
        if flags.is_empty() {
            return;
        }
        for flag in &flags {
            self.stage.stats.add_flag(*flag);
        }
        self.event(StageEvent::MtFlagged {
            clause: self.clause,
            flags,
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

    fn finish(&mut self) -> Ended {
        if self.full.trim().is_empty() {
            return self.suspect("empty");
        }
        if !has_speakable_content(&self.full) {
            return self.suspect("nothing to say aloud");
        }
        let last = self.splitter.finish();
        if self.wrong_language_unspoken() || self.answer_unspoken() {
            return Ended::Retry;
        }
        let full = &self.full;
        self.stage
            .tracer
            .mark_text(self.clause, TraceStage::MtDone, || full.clone());
        let sent = self.event(StageEvent::MtDone {
            clause: self.clause,
        });
        let ok = sent
            && last.is_none_or(|p| !self.speakable(&p) || self.piece(p, false))
            && self.piece(String::new(), true);
        if ok { Ended::Done } else { Ended::Cancelled }
    }
}
