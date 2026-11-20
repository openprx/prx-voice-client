//! Trace port: where the pipeline reports stage boundaries for latency measurement and debugging.

use prxvc_media::{ClauseId, MonoTime};

/// A stage boundary of the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TraceStage {
    /// Speech began.
    SpeechStart,
    /// A partial recognition result appeared.
    AsrPartial,
    /// A clause was committed.
    ClauseCommitted,
    /// The refine pass finished.
    RefineDone,
    /// The first translated token arrived.
    MtFirstToken,
    /// Translation finished.
    MtDone,
    /// The first synthesised chunk arrived.
    TtsFirstChunk,
    /// Synthesis finished.
    TtsDone,
    /// Playback of the clause began.
    PlayoutStart,
    /// The clause was cancelled.
    Cancelled,
    /// The clause was skipped to keep up.
    Skipped,
    /// A stage failed.
    Failed,
}

/// One trace record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    /// The clause the event belongs to.
    pub clause: ClauseId,
    /// When it happened, on the pipeline clock.
    pub at: MonoTime,
    /// Which boundary.
    pub stage: TraceStage,
    /// The text at this boundary; only set when the sink asked for text.
    pub text: Option<String>,
}

/// A receiver of trace events.
pub trait TraceSink: Send + Sync {
    /// Whether the sink wants `text` filled in. `false` in the product's sink,
    /// so that call content never reaches logs.
    fn wants_text(&self) -> bool;

    /// Record one event.
    fn record(&self, ev: TraceEvent);
}
