//! Interpreter audio-routing abstractions.
//!
//! Two platform-agnostic traits sit between the translation pipelines and the
//! OS-specific audio plumbing, so the pipelines never touch Core Audio / WASAPI:
//!
//! * [`AppAudioSource`] — capture the audio *rendered by another application*
//!   (the other party). macOS impl: [`crate::core_audio_tap::CoreAudioTapSource`].
//!   Windows impl (later): `WasapiProcessLoopbackSource`.
//! * [`VirtualMicSink`] — inject synthesized audio into the meeting app's
//!   microphone. macOS impl: [`crate::blackhole_sink::BlackHoleSink`].
//!   Windows impl (later): `VbCableSink`.
//!
//! These complement the device-level [`crate::pipeline::AudioInput`] /
//! [`AudioOutput`](crate::pipeline::AudioOutput) traits; they exist specifically
//! for the per-app capture / virtual-mic injection the interpreter needs.

use crate::{AudioError, AudioFrame, AudioState};

/// Which process(es) an [`AppAudioSource`] should capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessFilter {
    /// Capture only the audio rendered by this process id (the meeting app).
    IncludeApp(i32),
    // Future: ExcludeSelf — capture everything except our own process, using
    // CATapDescription's exclude mode / WASAPI EXCLUDE_TARGET_PROCESS_TREE.
}

/// A source of audio rendered by another application — the "other party" side
/// of the interpreter. Frames are delivered in the source's **native format**
/// (typically interleaved float32 at 48 kHz); convert with
/// [`crate::resample::MonoResampler16k`] before ASR.
pub trait AppAudioSource: Send {
    /// Begin capturing. Idempotent failure: returns an error if already started
    /// or if the OS denies capture (permission / unsupported).
    fn start(&mut self) -> Result<(), AudioError>;

    /// Stop capturing and release native resources.
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Non-blocking: return the next captured frame if one is ready, else `None`.
    fn try_recv(&mut self) -> Option<AudioFrame>;

    /// Current capture state.
    fn state(&self) -> AudioState;
}

/// A sink that injects audio into the meeting app's microphone — the channel by
/// which the other party hears our cloned voice. Backed by a user-installed
/// virtual audio device (BlackHole / VB-CABLE).
pub trait VirtualMicSink: Send {
    /// Open the virtual device and begin streaming.
    fn start(&mut self) -> Result<(), AudioError>;

    /// Stop streaming and close the device.
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Enqueue a frame to be played into the virtual mic.
    fn write(&mut self, frame: &AudioFrame) -> Result<(), AudioError>;

    /// Current sink state.
    fn state(&self) -> AudioState;
}
