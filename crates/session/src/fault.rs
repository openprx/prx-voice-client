//! Typed fault vocabulary for the pipeline machines: the enums that name where something went wrong.
//!
//! Shared by the call, direction and segment machines; they branch on these values, never on text.

use prxvc_media::Direction;

/// A stage of one direction pipeline that can fail or be degraded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Voice activity detection.
    Vad,
    /// Streaming recognition.
    Asr,
    /// The refine recognition pass.
    Refine,
    /// Translation.
    Mt,
    /// Speech synthesis.
    Tts,
    /// Playout and mixing.
    Playout,
}

/// A provider role the call prepares before translation starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderRole {
    /// Voice activity detection.
    Vad,
    /// Streaming recognition.
    Asr,
    /// The refine recognition pass.
    AsrRefine,
    /// Translation.
    Mt,
    /// Speech synthesis.
    Tts,
}

/// The class of a failure, decided by the adapter that reports it.
///
/// The machines only branch on where a fault happened, never on its text;
/// the coordinator maps provider error values onto this classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaultKind {
    /// The provider or device cannot be reached or does not exist.
    Unavailable,
    /// The provider did not answer in time.
    Timeout,
    /// The provider or device stopped working while in use.
    Crashed,
    /// The provider refused the request.
    Rejected,
    /// Permission was denied by the operating system.
    PermissionDenied,
}

/// One of the four audio endpoints of a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    /// The physical microphone (source of the outgoing direction).
    LocalInput,
    /// The tap on the call application (source of the incoming direction).
    RemoteInput,
    /// The headphones (sink of the incoming direction).
    LocalOutput,
    /// The virtual microphone (sink of the outgoing direction).
    RemoteOutput,
}

impl Endpoint {
    /// The direction whose audio flows through this endpoint.
    pub const fn direction(self) -> Direction {
        match self {
            Self::LocalInput | Self::RemoteOutput => Direction::Outgoing,
            Self::RemoteInput | Self::LocalOutput => Direction::Incoming,
        }
    }
}

/// Why a call cannot start or must stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blocker {
    /// The application is not running from its bundle.
    NotInBundle,
    /// A required permission was not granted.
    PermissionDenied,
    /// The virtual microphone driver is missing, stale or the wrong variant.
    DriverUnusable,
    /// No physical microphone can be opened.
    NoMicrophone,
    /// The routing journal cannot be written, so restoring is not guaranteed.
    RoutingUnsafe,
    /// An endpoint failed while the call was starting.
    EndpointFailed(Endpoint, FaultKind),
    /// Switching the default input failed.
    RoutingFailed(FaultKind),
}

/// A condition that keeps a direction translating in a reduced way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DegradeReason {
    /// A fallback voice is used.
    FallbackVoice,
    /// A fallback translation provider is used.
    MtFallback,
    /// The refine pass keeps failing and streaming text is used.
    RefineFailing,
    /// Clauses are skipped to keep up.
    BacklogSkipping,
    /// Capture overruns lose audio.
    CaptureOverrun,
}
