//! The seam between the adapter and the native engine.
//!
//! [`RecognizeEngine`] is what `QwenAsr` needs from the engine: decode one buffer of audio. The
//! FFI wrapper (`native`, feature `transcribe`) implements it over `prxvc-transcribe-sys`; tests
//! implement it with a scripted fake, so the adapter's scheduling, stability and fairness are tested
//! without a model or a GPU.

use prxvc_media::Lang;

/// A failure of the engine.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The call stopped because the stop poll returned true.
    #[error("decode aborted")]
    Aborted,
    /// The call failed.
    #[error("decode failed: {0}")]
    Failed(String),
}

/// A loaded recognition engine. One worker thread owns it and calls it, one decode at a time (the
/// runtime allows one compute in flight per loaded model).
pub trait RecognizeEngine: Send + 'static {
    /// Decode `audio` (16 kHz mono, finite samples) with the language hint `lang` and return the
    /// text. Blocking. `stop` is polled while the engine computes; true ends the call with
    /// [`EngineError::Aborted`] as soon as the engine can.
    fn decode(
        &mut self,
        audio: &[f32],
        lang: Lang,
        stop: &dyn Fn() -> bool,
    ) -> Result<String, EngineError>;
}
