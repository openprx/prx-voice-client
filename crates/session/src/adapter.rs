//! Service adapter interfaces for the interpreting pipeline.
//!
//! These abstract the MT and TTS stages so the [`crate::driver::InterpretDriver`]
//! and the [`crate::interpret`] state machine stay free of any concrete engine,
//! transport, or platform dependency. Concrete impls — local
//! (Qwen/NLLB, CosyVoice 2) or cloud (LLM/DeepL, ElevenLabs/Cartesia) — live in
//! the integration layer that can depend on audio / network crates.
//!
//! The ASR stage is intentionally *not* modeled here: capture + recognition is
//! produced upstream (`prxvc-audio`) and enters the driver as recognized text.

/// Errors from MT / TTS adapters.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AdapterError {
    /// Machine translation failed.
    #[error("translation failed: {0}")]
    Translation(String),
    /// Speech synthesis failed.
    #[error("synthesis failed: {0}")]
    Synthesis(String),
}

/// Machine-translation adapter: source-language text → target-language text.
pub trait Translator: Send {
    /// Translate `text` from `source_lang` to `target_lang` (BCP-47-ish codes,
    /// e.g. `"zh"`, `"en"`).
    fn translate(
        &mut self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AdapterError>;
}

/// A synthesized audio buffer (mono float32) ready to be played into the
/// listener's ear or injected via the virtual mic.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesizedAudio {
    /// Mono float32 PCM samples.
    pub samples: Vec<f32>,
    /// Sample rate of `samples`.
    pub sample_rate: u32,
}

/// Text-to-speech adapter, optionally rendering in the user's cloned voice.
pub trait Synthesizer: Send {
    /// Synthesize `text` in `language`. When `clone_voice` is true, render in the
    /// user's cloned voice (Self→Peer direction); otherwise a default voice.
    fn synthesize(
        &mut self,
        text: &str,
        language: &str,
        clone_voice: bool,
    ) -> Result<SynthesizedAudio, AdapterError>;
}

// Boxed forwarding impls so callers can hold `Box<dyn Translator>` /
// `Box<dyn Synthesizer>` and pick an implementation at runtime.
impl<T: Translator + ?Sized> Translator for Box<T> {
    fn translate(
        &mut self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AdapterError> {
        (**self).translate(text, source_lang, target_lang)
    }
}

impl<S: Synthesizer + ?Sized> Synthesizer for Box<S> {
    fn synthesize(
        &mut self,
        text: &str,
        language: &str,
        clone_voice: bool,
    ) -> Result<SynthesizedAudio, AdapterError> {
        (**self).synthesize(text, language, clone_voice)
    }
}
