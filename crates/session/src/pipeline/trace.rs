//! Records the trace events of the stage boundaries (pipeline-design 5.4) on the pipeline clock.

use std::sync::Arc;

use parking_lot::Mutex;

use prxvc_media::{ClauseId, Clock, MonoTime, SegmentId};
use prxvc_ports::{TraceEvent, TraceSink, TraceStage};

use super::utterance::{OpenTimes, SegmentTimes};

/// Writes stage-boundary events to a [`TraceSink`], stamped by the shared clock.
///
/// Text is built and attached only when the sink asks for it
/// ([`TraceSink::wants_text`]): the product's sink records timings and
/// counts, never call content.
#[derive(Clone)]
pub(crate) struct Tracer {
    sink: Arc<dyn TraceSink>,
    clock: Arc<dyn Clock>,
    /// The times of the open segments, for the utterance log line (never any text).
    times: Arc<Mutex<OpenTimes>>,
}

impl Tracer {
    pub(crate) fn new(sink: Arc<dyn TraceSink>, clock: Arc<dyn Clock>) -> Self {
        Self {
            sink,
            clock,
            times: Arc::default(),
        }
    }

    /// The stage times of `segment` seen so far; they are forgotten.
    pub(crate) fn take_times(&self, segment: SegmentId) -> SegmentTimes {
        self.times.lock().take(segment)
    }

    /// The current time on the pipeline clock.
    pub(crate) fn now(&self) -> MonoTime {
        self.clock.now()
    }

    /// Record `stage` of `clause` now, without text.
    pub(crate) fn mark(&self, clause: ClauseId, stage: TraceStage) {
        self.record(clause, stage, self.clock.now(), None);
    }

    /// Record `stage` of `clause` at `at`, without text.
    pub(crate) fn mark_at(&self, clause: ClauseId, stage: TraceStage, at: MonoTime) {
        self.record(clause, stage, at, None);
    }

    /// Record `stage` of `clause` now with the text `text()` builds, if the sink wants text.
    pub(crate) fn mark_text(
        &self,
        clause: ClauseId,
        stage: TraceStage,
        text: impl FnOnce() -> String,
    ) {
        let text = self.sink.wants_text().then(text);
        self.record(clause, stage, self.clock.now(), text);
    }

    fn record(&self, clause: ClauseId, stage: TraceStage, at: MonoTime, text: Option<String>) {
        self.times.lock().note(clause.segment(), stage, at);
        self.sink.record(TraceEvent {
            clause,
            at,
            stage,
            text,
        });
    }
}
