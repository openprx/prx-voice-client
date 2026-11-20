//! The seam between the adapter and the native engine.
//!
//! [`SynthEngine`] is what `QwenTts` needs from the engine: prepare a voice, run one blocking
//! synthesis call. The FFI wrapper (`engine::NativeEngine`, feature `qwentts`) implements it over
//! `prxvc-qwentts-sys`; tests implement it with a scripted fake, so the adapter's threading, caps,
//! runaway handling and back-pressure are tested without a model or a GPU.

use prxvc_media::Lang;

/// A failure of the engine itself.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The model could not be loaded.
    #[error("engine load failed: {0}")]
    Load(String),
    /// A call failed; `status` is the runtime's status code when it gave one.
    #[error("engine call failed (status {status}): {message}")]
    Call {
        /// The runtime's status code.
        status: i32,
        /// The runtime's last error message.
        message: String,
    },
    /// The request cannot be passed to the runtime (an embedded NUL byte, a size over `i32`).
    #[error("invalid engine input: {0}")]
    InvalidInput(String),
    /// A native callback panicked; the panic was caught at the boundary.
    #[error("engine callback panicked")]
    CallbackPanicked,
}

/// How a synthesis call ended without an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallEnd {
    /// The runtime finished: it emitted end of speech or reached its generation cap. The runtime
    /// reports both as success; the adapter tells them apart by the audio produced.
    Completed,
    /// The call stopped because [`CallControl::should_abort`] returned true.
    Aborted,
}

/// One synthesis call.
#[derive(Clone, Copy, Debug)]
pub struct SynthCall<'a> {
    /// The clause to speak; non-empty.
    pub text: &'a str,
    /// The language of `text`.
    pub lang: Lang,
    /// The speaker embedding of the voice, prepared for this engine's talker model.
    pub speaker: &'a [f32],
    /// Sampler seed; -1 asks the runtime for a random one.
    pub seed: i64,
    /// Generation cap in codec frames (12.5 per second of audio).
    pub max_new_tokens: u32,
}

/// The adapter's side of a call: it receives audio and says when to stop.
///
/// Both methods run on a thread of the runtime (its compute worker), not on the thread that
/// called [`SynthEngine::synthesize`]. They must not block: a blocked callback stalls every
/// request batched with this one.
pub trait CallControl: Sync {
    /// Streamed audio: mono `f32` at 24 kHz. The slice is valid for the call only.
    fn on_chunk(&self, samples: &[f32]);

    /// Polled before every decode step; true stops the call.
    fn should_abort(&self) -> bool;
}

/// A loaded synthesis engine, shared by every request.
pub trait SynthEngine: Send + Sync + 'static {
    /// Prepare a speaker embedding from 24 kHz mono samples. The embedding is specific to the
    /// loaded talker model.
    fn extract_voice(&self, samples_24k: &[f32]) -> Result<Vec<f32>, EngineError>;

    /// Run one synthesis call to its end, streaming audio through `ctl`. Blocking; concurrent calls
    /// on one engine are batched by the runtime up to its batch size.
    fn synthesize(
        &self,
        call: &SynthCall<'_>,
        ctl: &dyn CallControl,
    ) -> Result<CallEnd, EngineError>;
}
