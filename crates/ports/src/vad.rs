//! Voice activity detection port: a per-direction engine that scores audio frames for speech.

use prxvc_media::SampleRate;

use crate::descriptor::ProviderDescriptor;

/// A VAD failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VadError {
    /// The engine cannot be opened or has stopped working.
    #[error("vad unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The frame does not have the length the engine needs.
    #[error("vad frame has {got} samples, expected {expected}")]
    FrameLength {
        /// Samples the engine needs per call.
        expected: usize,
        /// Samples it was given.
        got: usize,
    },
    /// An unexpected engine failure.
    #[error("vad internal error: {0}")]
    Internal(String),
}

/// One open VAD engine. Not shared: each direction opens its own.
pub trait VadEngine: Send {
    /// Samples per call to [`VadEngine::probability`], at [`VadEngine::rate`].
    fn frame_len(&self) -> usize;

    /// The sample rate the engine expects.
    fn rate(&self) -> SampleRate;

    /// The probability, in `0.0..=1.0`, that `frame` contains speech.
    fn probability(&mut self, frame: &[f32]) -> Result<f32, VadError>;

    /// Forget all history (the start of a new call).
    fn reset(&mut self);
}

/// A VAD implementation that opens engines.
pub trait VadProvider: Send + Sync {
    /// What the provider is and supports.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Open an engine; one per direction.
    fn open(&self) -> Result<Box<dyn VadEngine>, VadError>;
}
