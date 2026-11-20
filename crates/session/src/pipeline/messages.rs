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

/// A clause the recogniser commits.
#[derive(Debug)]
pub(crate) struct Commit {
    pub(crate) text: String,
    pub(crate) source_start: MonoTime,
    pub(crate) source_end: MonoTime,
    /// The audio of the clause, only when a refine pass will use it.
    pub(crate) audio: Option<Arc<ClauseSamples>>,
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
    EndSegment,
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
