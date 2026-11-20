//! The events a direction pipeline hands to its caller: audio, what the listener hears, failures and cancellations.

use prxvc_media::{ClauseId, Envelope, PcmChunk};
use prxvc_ports::{EndpointError, VoiceSelection};

use crate::direction_state::DirectionState;
use crate::fault::{DegradeReason, FaultKind, Stage};
use crate::interrupt::CancelAction;
use crate::output::{OutputCause, OutputSource};
use crate::pipeline::snapshot::SkipReason;

/// One chunk of translated speech of one clause.
#[derive(Clone, Debug, PartialEq)]
pub struct ClauseAudio {
    /// The clause the chunk belongs to.
    pub clause: ClauseId,
    /// The audio, at the synthesiser's rate.
    pub chunk: PcmChunk,
    /// The voice that actually spoke (it differs from the configured one when a fallback voice runs).
    pub voice: VoiceSelection,
}

/// An event of the pipeline, in the order the control thread produced it.
///
/// Audio of a clause arrives strictly in clause order: every chunk of a
/// clause, then its [`PipelineEvent::AudioEnd`], then the chunks of the next.
/// The caller reports playback back with `playback_started` and
/// `playback_finished`; the next clause is released when the previous one has
/// been reported finished.
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineEvent {
    /// The direction machine changed state.
    State(DirectionState),
    /// What the listener hears changed: the only authority on it (the direction machine's `ChangeOutput`).
    Output {
        /// What was heard.
        from: OutputSource,
        /// What is heard now.
        to: OutputSource,
        /// Why.
        cause: OutputCause,
    },
    /// A reduced mode began.
    Degraded(DegradeReason),
    /// A reduced mode ended.
    Recovered(DegradeReason),
    /// A stage failed and no fallback exists; the failure policy applies.
    Failed {
        /// The stage.
        stage: Stage,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// A chunk of translated speech to play, in clause order.
    Audio(Envelope<ClauseAudio>),
    /// The clause's audio is complete.
    AudioEnd(Envelope<ClauseId>),
    /// Discard audio of this scope that was handed out and is not played
    /// yet: provider requests were cancelled and queues purged; the playout
    /// side flushes its queue (with a short fade where something plays).
    Flush(CancelAction),
    /// A queued clause was skipped and will not be spoken.
    ClauseSkipped {
        /// The clause.
        clause: ClauseId,
        /// Why.
        reason: SkipReason,
    },
    /// A clause failed (the direction may carry on: a runaway synthesis, an overflow).
    ClauseFailed {
        /// The clause.
        clause: ClauseId,
        /// The stage that failed it.
        stage: Stage,
    },
    /// The attached audio source ended.
    CaptureEnded(Option<EndpointError>),
}
