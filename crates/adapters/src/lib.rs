//! Concrete MT / TTS adapters for the interpreting pipeline.
//!
//! Implements the [`prxvc_session::adapter`] traits with real engines, keeping
//! `prxvc-session` free of network/audio dependencies:
//!
//! * [`OpenAiTranslator`] — OpenAI-compatible chat-completions MT. Points at a
//!   cloud endpoint or a local server (Ollama / LM Studio / llama.cpp).
//! * [`EchoTranslator`] — offline pass-through, for tests and offline demos.
//! * [`SineToneSynthesizer`] — offline tone synthesizer for tests/development
//!   (a tone whose length tracks the text), used in place of a real TTS engine.

pub mod echo_translator;
pub mod openai_translator;
pub mod tone_synth;

pub use echo_translator::EchoTranslator;
pub use openai_translator::OpenAiTranslator;
pub use tone_synth::SineToneSynthesizer;

// Re-export the trait surface so downstream code needs one import.
pub use prxvc_session::adapter::{AdapterError, SynthesizedAudio, Synthesizer, Translator};
