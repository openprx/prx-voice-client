//! Stage and endpoint ports of the call-translation pipeline (layer L1).
//!
//! This file only declares the modules and re-exports the public API. The
//! crate depends on `prxvc-media` only; adapters implement these traits in
//! other crates and the pipeline names nothing but the traits.
//!
//! Streaming and cancellation semantics shared by every stage port:
//!
//! * Ports are synchronous: a stage worker thread calls them. Output arrives
//!   incrementally from a stream object that is created per request, so the
//!   stream is the request's own handle and cancelling one request never
//!   touches another request, another direction or the shared engine.
//! * A call that can block takes a [`CancellationToken`]. After the token is
//!   cancelled the call returns promptly with `Cancelled`, the stream yields
//!   nothing further and the engine stays usable. Dropping a stream abandons
//!   its request.
//! * A stream follows one protocol: zero or more `Some(Ok(item))`, then either
//!   the terminal item (`Done`) or one `Some(Err(..))`, then `None` forever.
//!
//! Modules:
//!
//! * `descriptor`: [`ProviderDescriptor`] and the capability checks the
//!   registry applies before it hands a provider to the live path.
//! * `vad`, `asr`, `mt`, `tts`: the stage ports and their errors.
//! * `dialogue`: the conversational backend of the assistant.
//! * `endpoint`: [`AudioSource`] and [`AudioSink`].
//! * `routing`: [`SystemAudioRouting`].
//! * `trace`: [`TraceSink`].
//! * `fakes` (feature `fakes`): scriptable deterministic fakes of every port.

pub mod asr;
pub mod descriptor;
pub mod dialogue;
pub mod endpoint;
#[cfg(feature = "fakes")]
pub mod fakes;
pub mod mt;
pub mod routing;
pub mod trace;
pub mod tts;
pub mod vad;

pub use tokio_util::sync::CancellationToken;

pub use asr::{
    AsrContext, AsrError, AsrHypothesis, AsrStream, RefineAsr, SegmentEnd, StreamingAsr,
};
pub use dialogue::{
    ConversationId, DialogueBackend, DialogueDelta, DialogueError, DialogueMessage, DialogueRequest,
    DialogueRole, DialogueStream,
};
pub use descriptor::{CapabilityError, Egress, ProviderDescriptor, ProviderId, StreamingKind};
pub use endpoint::{
    AudioSink, AudioSource, EndpointError, EndpointHealth, SinkInfo, SourceInfo, WriteOutcome,
};
pub use mt::{MtDelta, MtError, MtPhase, MtRequest, MtStream, Translator};
pub use routing::{
    BundleId, CallActivity, DeviceUid, RoutingChange, RoutingError, SystemAudioRouting,
};
pub use trace::{TraceEvent, TraceSink, TraceStage};
pub use tts::{
    PreparedVoice, SpeechSynth, TtsError, TtsItem, TtsRequest, TtsStream, VoiceReference,
    VoiceSelection,
};
pub use vad::{VadEngine, VadError, VadProvider};
