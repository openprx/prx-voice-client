//! Refine stage: the second recognition pass over the audio of a committed clause (two-pass mode).

use std::sync::Arc;

use prxvc_media::Lang;
use prxvc_ports::{AsrContext, RefineAsr, TraceStage};

use super::faults::asr_failure;
use super::messages::{RefineJob, StageEvent, StageFailure};
use super::outbox::{Meta, Outbox};
use super::queue::{BoundedQueue, Popped, Wait};
use super::trace::Tracer;
use crate::fault::FaultKind;
use prxvc_media::Envelope;

/// Everything the refine thread uses.
pub(crate) struct RefineStage {
    pub(crate) refine: Arc<dyn RefineAsr>,
    pub(crate) lang: Lang,
    pub(crate) hotwords: Vec<String>,
    pub(crate) outbox: Outbox,
    pub(crate) input: Arc<BoundedQueue<Envelope<RefineJob>>>,
    pub(crate) tracer: Tracer,
}

impl RefineStage {
    /// The thread body: decode clauses until the queue closes. A previous
    /// clause's text is handed on as context.
    pub(crate) fn run(self) {
        let mut previous: Option<String> = None;
        let mut seen = 0;
        loop {
            match self.input.pop(Wait::Forever, &mut seen) {
                Popped::Item(job) => self.decode(job, &mut previous),
                Popped::Woken | Popped::Timeout => {}
                Popped::Closed => break,
            }
        }
    }

    fn decode(&self, job: Envelope<RefineJob>, previous: &mut Option<String>) {
        let meta = Meta {
            revision: job.revision,
            source_ts: job.source_ts,
        };
        let RefineJob {
            clause,
            stream_text: _,
            audio,
            token,
        } = job.payload;
        if token.is_cancelled() {
            return;
        }
        let ctx = AsrContext {
            previous_text: previous.clone(),
            hotwords: self.hotwords.clone(),
        };
        let result = self
            .refine
            .transcribe(&audio.samples, audio.rate, self.lang, &ctx, &token);
        let result = match result {
            Ok(h) if !h.text.trim().is_empty() => {
                self.tracer.mark(clause, TraceStage::RefineDone);
                *previous = Some(h.text.trim().to_owned());
                Ok(h.text.trim().to_owned())
            }
            Ok(_) => Err(StageFailure::new(
                FaultKind::Crashed,
                "refine returned no text",
            )),
            Err(e) if e.is_cancelled() || token.is_cancelled() => return,
            Err(e) => Err(asr_failure(&e, true)),
        };
        self.outbox
            .stage(clause, meta, &token, StageEvent::Refined { clause, result });
    }
}
