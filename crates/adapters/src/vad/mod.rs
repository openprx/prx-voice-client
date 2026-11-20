//! Voice activity detection (work package CT-11a; pipeline-design 2.5 and PD-3.2).
//!
//! * [`SpeechGate`]: the pure event logic that turns per-frame speech probabilities into
//!   speech-start and speech-end events on the pipeline clock, with the benchmark's rules (start
//!   threshold with a minimum speech duration, a lower end threshold, padding, a segment-ending
//!   pause that is shorter for long segments, a hard length cut). It knows nothing about the engine.
//! * [`VadGate`]: a [`prxvc_ports::VadEngine`] plus a [`SpeechGate`], fed with arbitrary-length
//!   16 kHz chunks.
//! * `native` (feature `transcribe`): `SileroVad`, the Silero v6.2 engine over the `transcribe.cpp`
//!   runtime, and its provider.
//!
//! Everything but `native` builds and is tested without the native library.

pub mod gate;
#[cfg(feature = "transcribe")]
pub mod native;
pub mod stream;

pub use gate::{GateParams, SpeechGate, VadEvent};
#[cfg(feature = "transcribe")]
pub use native::{PROVIDER_ID, SileroVad, SileroVadProvider};
pub use stream::VadGate;
