//! Concrete MT / TTS adapters for the interpreting pipeline.
//!
//! Implements the [`prxvc_session::adapter`] traits with real engines, keeping
//! `prxvc-session` free of network/audio dependencies:
//!
//! * [`OpenAiTranslator`] — OpenAI-compatible chat-completions MT. Points at a
//!   cloud endpoint or a local server (Ollama / LM Studio / llama.cpp).
//! * [`EchoTranslator`] — offline pass-through, for tests and offline demos.
//! * [`vad`] and [`asr`]: voice activity detection (a speech gate over any `VadEngine`, and Silero v6.2
//!   with the feature `transcribe`) and recognition by re-decoding (`QwenAsr`, Qwen3-ASR through
//!   `transcribe.cpp`), implementing the `prxvc-ports` VAD, streaming-ASR and refine-ASR ports.
//! * [`SineToneSynthesizer`] — offline tone synthesizer for tests/development
//!   (a tone whose length tracks the text), used in place of a real TTS engine.

pub mod asr;
pub mod chat_http;
pub mod dialogue;
pub mod echo_translator;
pub mod mt;
pub mod openai_translator;
pub mod tone_synth;
#[cfg(feature = "transcribe")]
pub mod transcribe;
pub mod tts;
pub mod vad;

pub use echo_translator::EchoTranslator;
pub use openai_translator::OpenAiTranslator;
pub use tone_synth::SineToneSynthesizer;

// Re-export the trait surface so downstream code needs one import.
pub use prxvc_session::adapter::{AdapterError, SynthesizedAudio, Synthesizer, Translator};
