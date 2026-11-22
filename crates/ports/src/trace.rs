//! Trace port: where the pipeline reports stage boundaries for latency measurement and debugging.

use prxvc_media::{ClauseId, MonoTime};

/// A stage boundary of the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TraceStage {
    /// Speech began.
    SpeechStart,
    /// A partial recognition result appeared (the first one of a clause).
    AsrPartial,
    /// The silence that commits a clause was detected.
    ClausePause,
    /// The final source text of the clause is ready (streaming final or refine result).
    AsrFinalReady,
    /// A clause was committed.
    ClauseCommitted,
    /// The final text of a segment disagreed with text the pipeline had already committed: the
    /// committed text stayed and only the remainder was taken from the final. With text, the
    /// committed text and the final text.
    FinalDisagrees,
    /// The recogniser ended the segment itself at its maximum length; the speech goes on in a new
    /// segment. Recorded against the clause the cut committed (or the first clause of the segment
    /// that was cut when it committed none).
    SegmentCut,
    /// A recognised unit that was only filler ("嗯", "Mm.") was dropped: not translated, not
    /// spoken. Recorded against the clause the unit would have been; with text, the unit.
    FillerDropped,
    /// A unit was committed by the re-translation rule while the speaker went on. Recorded against
    /// the clause it committed.
    RetranslationCut,
    /// Why a unit was cut and whether its sentence goes on. With text, the reason's name and
    /// `continues` or `ends` (a name, never spoken text). Recorded against the unit's clause.
    UnitCut,
    /// A clause translated from the start of its sentence no longer began with the part already
    /// spoken, so the new part was translated on its own. With text, the spoken translation and
    /// the new one (for the development harness only: a trace sink that logs must not log it).
    ContinuationDiverged,
    /// A clause translated from the start of its sentence began with something close to, but not
    /// the same as, the part already spoken, and went on from there. Text as for
    /// `ContinuationDiverged` (development harness only).
    ContinuationApprox,
    /// The refine pass finished.
    RefineDone,
    /// The translation request was sent.
    MtRequestSent,
    /// The first translated token arrived.
    MtFirstToken,
    /// Translation finished.
    MtDone,
    /// The synthesis request was sent.
    TtsRequestSent,
    /// The first synthesised chunk arrived.
    TtsFirstChunk,
    /// Synthesis finished.
    TtsDone,
    /// Playback of the clause began.
    PlayoutStart,
    /// Playback of the clause ended.
    PlayoutEnd,
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
