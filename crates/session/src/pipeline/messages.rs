//! The items that cross the queues between the stage threads and the control thread, all wrapped in envelopes.

use std::sync::Arc;

use prxvc_media::{ClauseId, Envelope, MonoTime, PcmChunk, SampleRate, SegmentId};
use prxvc_ports::{CancellationToken, EndpointError, VoiceSelection};

use crate::fault::{FaultKind, Stage};

/// Audio of one clause, kept for the refine pass.
#[derive(Debug)]
pub(crate) struct ClauseSamples {
    pub(crate) samples: Vec<f32>,
    pub(crate) rate: SampleRate,
}

/// A failure of a provider call as the control thread needs it: classified, and without call content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageFailure {
    pub(crate) fault: FaultKind,
    pub(crate) detail: String,
}

impl StageFailure {
    pub(crate) fn new(fault: FaultKind, detail: impl Into<String>) -> Self {
        Self {
            fault,
            detail: detail.into(),
        }
    }
}

/// Why the recogniser committed a clause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitReason {
    /// The segment ended.
    SegmentEnd,
    /// A pause inside the segment.
    ClausePause,
    /// The uncommitted audio reached its maximum.
    LengthCut,
    /// The recogniser ended the segment itself at its maximum length.
    Forced,
    /// Stable text reached a sentence-final or clause-level mark (early commit).
    Punctuation,
    /// Stable text reached the chunk size (early commit).
    Chunk,
    /// The maximum wait passed and a smaller chunk was taken (early commit).
    MaxWait,
    /// The re-translation rule: the translation of the prefix was confirmed.
    Retranslation,
}

impl CommitReason {
    /// Whether the clause was cut from the stable text of a hypothesis while the speaker went on.
    pub(crate) const fn is_early(self) -> bool {
        matches!(
            self,
            Self::Punctuation | Self::Chunk | Self::MaxWait | Self::Retranslation
        )
    }

    /// Whether the commit ends its segment.
    pub(crate) const fn ends_segment(self) -> bool {
        matches!(self, Self::SegmentEnd | Self::Forced)
    }

    /// The name used in logs and reports.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::SegmentEnd => "segment_end",
            Self::ClausePause => "clause_pause",
            Self::LengthCut => "length_cut",
            Self::Forced => "forced",
            Self::Punctuation => "punctuation",
            Self::Chunk => "chunk",
            Self::MaxWait => "max_wait",
            Self::Retranslation => "retranslation",
        }
    }
}

/// A clause the recogniser commits.
#[derive(Debug)]
pub(crate) struct Commit {
    pub(crate) text: String,
    pub(crate) source_start: MonoTime,
    pub(crate) source_end: MonoTime,
    /// The audio of the clause, only when a refine pass will use it.
    pub(crate) audio: Option<Arc<ClauseSamples>>,
    /// Why it was committed.
    pub(crate) reason: CommitReason,
    /// The unit does not end its source sentence: what follows in the segment goes on with it, so
    /// the translation stage translates from the start of the sentence.
    pub(crate) continues: bool,
}

/// Segment-level events of the recogniser (envelope: segment, hypothesis revision, speech onset).
#[derive(Debug)]
pub(crate) enum RecognitionEvent {
    SpeechStarted,
    Hypothesis {
        text: String,
        stable_prefix_len: usize,
    },
    Commit(Commit),
    /// What the draft worker did for the segment, sent just before `EndSegment` (only with
    /// re-translation on).
    DraftTally(DraftTally),
    EndSegment,
}

/// The re-translation figures of one segment, counted by the recognition stage.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DraftTally {
    /// Engine calls made for drafts.
    pub(crate) drafts: u32,
    /// Draft texts served from the cache.
    pub(crate) hits: u32,
    /// Replies discarded as stale.
    pub(crate) stale: u32,
    /// Partials or finals that contradicted text committed by the rule.
    pub(crate) src_rewrites: u32,
    /// Elapsed time of each engine call, in milliseconds.
    pub(crate) engine_ms: Vec<u64>,
}

/// Clause-level events of the refine, translation and synthesis stages (envelope: the clause's segment, frozen revision and onset).
#[derive(Debug)]
pub(crate) enum StageEvent {
    Refined {
        clause: ClauseId,
        result: Result<String, StageFailure>,
    },
    MtStarted {
        clause: ClauseId,
    },
    MtText {
        clause: ClauseId,
        delta: String,
    },
    MtDone {
        clause: ClauseId,
    },
    /// How the clause's translation related to the sentence it belongs to, sent before `MtDone`
    /// by a stage that translates from the start of the sentence.
    MtContinuation {
        clause: ClauseId,
        outcome: ContinuationOutcome,
    },
    /// The translation came back in the wrong language before any of it was spoken and is being
    /// requested again: the text so far is void.
    MtRetry {
        clause: ClauseId,
    },
    /// The finished translation carries quality flags; it was spoken all the same.
    MtFlagged {
        clause: ClauseId,
        flags: Vec<prxvc_ports::MtFlag>,
    },
    /// Translation failed or was flagged; `suspect` marks the latter.
    MtFailed {
        clause: ClauseId,
        failure: StageFailure,
        suspect: bool,
    },
    TtsStarted {
        clause: ClauseId,
        voice: VoiceSelection,
    },
    TtsChunk {
        clause: ClauseId,
        chunk: PcmChunk,
    },
    TtsDone {
        clause: ClauseId,
    },
    TtsFailed {
        clause: ClauseId,
        failure: StageFailure,
        runaway: bool,
    },
}

/// How a clause translated from the start of its sentence turned out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContinuationOutcome {
    /// The translation began with the part already spoken: the clause speaks what follows it.
    Continued,
    /// It did not: the clause's own text was translated on its own.
    Diverged,
    /// It began with the part already spoken and nothing speakable follows it.
    Empty,
    /// There was nothing spoken to continue: the clause was translated on its own.
    Standalone,
}

/// Events of the workers that belong to no clause.
#[derive(Debug)]
pub(crate) enum WorkerEvent {
    RecognizerReady,
    RecognizerFailed {
        stage: Stage,
        failure: StageFailure,
    },
    Overrun,
    OverrunCleared,
    ProbeResult {
        stage: Stage,
        result: Result<(), StageFailure>,
    },
    CaptureEnded(Option<EndpointError>),
}

/// What the caller can ask of a running pipeline.
#[derive(Debug)]
pub(crate) enum Command {
    Start,
    Pause,
    Resume,
    Suspend,
    Skip,
    CancelSegment(SegmentId),
    Retry,
    PlaybackStarted {
        clause: ClauseId,
        at: MonoTime,
    },
    PlaybackFinished {
        clause: ClauseId,
        at: MonoTime,
    },
    /// Every worker thread has ended (or the wait for them timed out).
    Joined {
        clean: bool,
    },
    Stop,
}

/// What the control thread reads.
#[derive(Debug)]
pub(crate) enum Inbound {
    Recognition(Envelope<RecognitionEvent>),
    Stage(Envelope<StageEvent>),
    Worker(WorkerEvent),
    Command(Command),
}

/// A committed clause waiting for the refine pass (Q3).
#[derive(Debug)]
pub(crate) struct RefineJob {
    pub(crate) clause: ClauseId,
    pub(crate) stream_text: String,
    pub(crate) audio: Arc<ClauseSamples>,
    pub(crate) token: CancellationToken,
}

/// A recognised clause waiting for translation (Q4).
#[derive(Debug)]
pub(crate) struct ClauseJob {
    pub(crate) clause: ClauseId,
    pub(crate) text: String,
    pub(crate) token: CancellationToken,
    /// The clause does not end its source sentence (see [`Commit::continues`]).
    pub(crate) continues: bool,
}

/// What the translation stage reads.
#[derive(Debug)]
pub(crate) enum MtItem {
    Clause(Envelope<ClauseJob>),
    /// Translate the probe text to see whether the provider works again.
    Probe,
}

/// A translated piece waiting for synthesis (Q5).
#[derive(Debug)]
pub(crate) struct PieceJob {
    pub(crate) clause: ClauseId,
    pub(crate) text: String,
    pub(crate) token: CancellationToken,
}

/// What the synthesis stage reads.
#[derive(Debug)]
pub(crate) enum TtsItem {
    /// Speak this text as part of the clause.
    Piece(Envelope<PieceJob>),
    /// The translation of the clause is complete; no more pieces follow.
    End(Envelope<PieceJob>),
    /// Synthesise the probe text to see whether the provider works again.
    Probe,
}
