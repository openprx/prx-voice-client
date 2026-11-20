//! Synthesis stage: one streamed request per translated piece, in clause order, with the direction's voice.

use std::sync::Arc;

use prxvc_media::{ClauseId, Envelope, SegmentId};
use prxvc_ports::{
    CancellationToken, SpeechSynth, TraceStage, TtsError, TtsItem as PortItem, TtsRequest,
    TtsStream,
};

use super::faults::tts_failure;
use super::messages::{PieceJob, StageEvent, StageFailure, TtsItem, WorkerEvent};
use super::outbox::{Meta, Outbox};
use super::params::PipelineParams;
use super::queue::{BoundedQueue, Popped, Wait};
use super::trace::Tracer;
use crate::fault::{FaultKind, Stage};
use crate::pipeline::snapshot::Stats;

/// Everything the synthesis thread uses.
pub(crate) struct TtsStage {
    pub(crate) tts: Arc<dyn SpeechSynth>,
    pub(crate) p: PipelineParams,
    pub(crate) outbox: Outbox,
    pub(crate) input: Arc<BoundedQueue<TtsItem>>,
    pub(crate) call: CancellationToken,
    pub(crate) tracer: Tracer,
    pub(crate) stats: Arc<Stats>,
}

/// What the thread remembers between pieces: the clause whose first piece it already announced.
#[derive(Default)]
struct Progress {
    announced: Option<ClauseId>,
    first_chunk_seen: Option<ClauseId>,
}

impl TtsStage {
    /// The thread body: synthesise pieces until the queue closes.
    pub(crate) fn run(self) {
        let mut progress = Progress::default();
        let mut seen = 0;
        loop {
            match self.input.pop(Wait::Forever, &mut seen) {
                Popped::Item(TtsItem::Piece(job)) => self.piece(job, &mut progress),
                Popped::Item(TtsItem::End(job)) => self.end(job),
                Popped::Item(TtsItem::Probe) => self.probe(),
                Popped::Woken | Popped::Timeout => {}
                Popped::Closed => break,
            }
        }
    }

    fn end(&self, job: Envelope<PieceJob>) {
        let meta = Meta {
            revision: job.revision,
            source_ts: job.source_ts,
        };
        let PieceJob { clause, token, .. } = job.payload;
        if token.is_cancelled() {
            Stats::add(&self.stats.late_dropped, 1);
            return;
        }
        self.tracer.mark(clause, TraceStage::TtsDone);
        self.outbox
            .stage(clause, meta, &token, StageEvent::TtsDone { clause });
    }

    fn piece(&self, job: Envelope<PieceJob>, progress: &mut Progress) {
        let meta = Meta {
            revision: job.revision,
            source_ts: job.source_ts,
        };
        let PieceJob {
            clause,
            text,
            token,
        } = job.payload;
        if token.is_cancelled() {
            Stats::add(&self.stats.late_dropped, 1);
            return;
        }
        if progress.announced != Some(clause) {
            self.tracer.mark(clause, TraceStage::TtsRequestSent);
        }
        let request = TtsRequest {
            clause,
            text,
            lang: self.p.target,
            voice: self.p.voice.clone(),
        };
        let stream = match self.tts.synthesize(request) {
            Ok(s) => s,
            Err(e) => {
                self.fail(clause, meta, &token, &e, false);
                return;
            }
        };
        if progress.announced != Some(clause) {
            progress.announced = Some(clause);
            let voice = stream.voice_used().clone();
            if !self.outbox.stage(
                clause,
                meta,
                &token,
                StageEvent::TtsStarted { clause, voice },
            ) {
                return;
            }
        }
        self.stream(clause, meta, &token, stream, progress);
    }

    /// Forward the chunks of one request until it ends.
    fn stream(
        &self,
        clause: ClauseId,
        meta: Meta,
        token: &CancellationToken,
        mut stream: Box<dyn TtsStream>,
        progress: &mut Progress,
    ) {
        loop {
            match stream.next(token) {
                Some(Ok(PortItem::Chunk(chunk))) => {
                    if progress.first_chunk_seen != Some(clause) {
                        progress.first_chunk_seen = Some(clause);
                        self.tracer.mark(clause, TraceStage::TtsFirstChunk);
                    }
                    if token.is_cancelled() {
                        Stats::add(&self.stats.late_dropped, 1);
                        return;
                    }
                    if !self.outbox.stage(
                        clause,
                        meta,
                        token,
                        StageEvent::TtsChunk { clause, chunk },
                    ) {
                        return;
                    }
                }
                Some(Ok(PortItem::Done { .. })) => return,
                Some(Err(e)) if e.is_cancelled() || token.is_cancelled() => return,
                Some(Err(e)) => {
                    self.fail(clause, meta, token, &e, true);
                    return;
                }
                None => {
                    if !token.is_cancelled() {
                        let failure =
                            StageFailure::new(FaultKind::Crashed, "synthesis ended early");
                        self.send_failure(clause, meta, token, failure, false);
                    }
                    return;
                }
            }
        }
    }

    fn fail(
        &self,
        clause: ClauseId,
        meta: Meta,
        token: &CancellationToken,
        e: &TtsError,
        in_use: bool,
    ) {
        if e.is_cancelled() || token.is_cancelled() {
            return;
        }
        let runaway = matches!(e, TtsError::Runaway { .. });
        self.send_failure(clause, meta, token, tts_failure(e, in_use), runaway);
    }

    fn send_failure(
        &self,
        clause: ClauseId,
        meta: Meta,
        token: &CancellationToken,
        failure: StageFailure,
        runaway: bool,
    ) {
        self.tracer.mark(clause, TraceStage::Failed);
        self.outbox.stage(
            clause,
            meta,
            token,
            StageEvent::TtsFailed {
                clause,
                failure,
                runaway,
            },
        );
    }

    /// Synthesise the probe text to see whether the provider works again.
    fn probe(&self) {
        let clause = ClauseId::new(SegmentId::first(self.p.direction), u16::MAX);
        let request = TtsRequest {
            clause,
            text: self.p.probe_text.clone(),
            lang: self.p.target,
            voice: self.p.voice.clone(),
        };
        let result = self
            .tts
            .synthesize(request)
            .map_err(|e| tts_failure(&e, false))
            .and_then(|mut s| {
                loop {
                    match s.next(&self.call) {
                        Some(Ok(PortItem::Done { .. })) => return Ok(()),
                        Some(Ok(PortItem::Chunk(_))) => {}
                        Some(Err(e)) => return Err(tts_failure(&e, true)),
                        None => {
                            return Err(StageFailure::new(FaultKind::Crashed, "probe ended early"));
                        }
                    }
                }
            });
        self.outbox.worker(WorkerEvent::ProbeResult {
            stage: Stage::Tts,
            result,
        });
    }
}
