//! `QwenTts`: in-process text to speech with `qwentts.cpp` behind the `prxvc-ports` synthesis port
//! (work package CT-10; pipeline-design 2.7 and 2.8; voice-cloning section 0 and work package C3).
//!
//! Layout:
//!
//! * [`adapter`]: [`QwenTts`], the worker threads, the job queue, retries and shutdown.
//! * [`request`]: one request's sink (the native callbacks' side) and stream (the consumer's side),
//!   and the back-pressure policy.
//! * [`cap`]: the generation cap and the runaway decision, with their constants.
//! * [`voice`]: preparing voices from reference recordings and the on-disk cache.
//! * [`seam`]: the engine interface the adapter is written against.
//! * `engine` (feature `qwentts`): the safe wrapper over `prxvc-qwentts-sys`, the only module of the
//!   crate that uses `unsafe` (CS-2.10.1, pipeline-design PD-2.7.1).
//!
//! Everything but `engine` builds and is tested without the native library. The native tests are
//! `#[ignore]`d live tests in `tests/qwentts_live.rs`.

pub mod adapter;
pub mod cap;
#[cfg(feature = "qwentts")]
pub mod engine;
pub mod model_hash;
pub mod request;
pub mod seam;
pub mod voice;
pub mod wav;

#[cfg(test)]
mod test_engine;
#[cfg(test)]
mod tests;

pub use adapter::{LoadError, PROVIDER_ID, QwenTts, QwenTtsConfig, TtsStats};
pub use cap::RunawayRetry;
pub use model_hash::hash_model_files;
pub use seam::{CallControl, CallEnd, EngineError, SynthCall, SynthEngine};
pub use voice::{CacheKey, VoiceCache, load_reference_wav};
