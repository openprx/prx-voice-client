//! Deterministic, scriptable fakes of every port, for tests of the pipeline and the registry (feature `fakes`).
//!
//! Nothing here touches the network, audio hardware or the wall clock.
//! Delays in scripts are measured on an injected [`prxvc_media::Clock`]: a
//! fake stream that is due to deliver at time `t` blocks until the test's
//! [`prxvc_media::ManualClock`] reaches `t`, or until the request's
//! cancellation token fires. Every fake that serves requests records an
//! [`EventLog`] so a test can observe what was started, completed, failed,
//! cancelled or abandoned.
//!
//! * [`log`]: the shared [`EventLog`].
//! * [`paced`]: the timed stream the MT and TTS fakes are built on.
//! * [`vad`], [`asr`], [`mt`], [`tts`], [`dialogue`], [`endpoint`], [`routing`], [`trace`]:
//!   one fake per port.

pub mod asr;
pub mod dialogue;
pub mod endpoint;
pub mod log;
pub mod mt;
pub mod paced;
pub mod routing;
pub mod trace;
pub mod tts;
pub mod vad;

pub use asr::{AsrScript, FakeRefineAsr, FakeStreamingAsr, RefineScript};
pub use dialogue::{DialogueScript, FakeDialogue};
pub use endpoint::{FakeAudioSink, FakeAudioSource, SinkProbe, WrittenFrame};
pub use log::{EventKind, EventLog, FakeEvent};
pub use mt::{FakeTranslator, MtScript};
pub use routing::{FakeSystemAudioRouting, RoutingProbe};
pub use trace::FakeTraceSink;
pub use tts::{FakeSpeechSynth, TtsScript};
pub use vad::{FakeVadProvider, VadScript};
