//! Speech recognition by re-decoding (work package CT-11b; pipeline-design 2.5, 2.7 and section 3).
//!
//! `QwenAsr` runs Qwen3-ASR through `transcribe.cpp`. The model is not incremental, so a stream's
//! partial hypotheses come from re-decoding the audio of the open segment at a cadence, always the
//! latest audio, never queued behind an unfinished pass; the endpoint is a final decode of the whole
//! segment. Because every re-decode may rewrite earlier text, the adapter declares a prefix stable
//! only when the last `N` hypotheses agree on it and never lets a later hypothesis retract it.
//!
//! Layout:
//!
//! * [`adapter`]: [`QwenAsr`] (the streaming and the refine port), its parameters, counters and start.
//! * `core` (private): the worker thread, the queue, the order of work and fairness between streams.
//! * `stream` (private): one stream's handle: cadence, stability, forced end at the maximum length.
//! * `stable`, `units` (private): the stable-prefix rule and the comparison units it works on.
//! * [`seam`]: the engine interface the adapter is written against.
//! * `native` (feature `transcribe`): the engine over `crate::transcribe`, where the `unsafe` lives.
//!
//! Capability descriptor: the provider says [`prxvc_ports::StreamingKind::Redecode`] (partial results
//! while audio arrives, produced by re-decoding) with the stable-prefix flag set. It passes
//! `require_live` and `require_partials` and refuses `require_native`, which means true incremental
//! streaming.

pub mod adapter;
mod core;
#[cfg(feature = "transcribe")]
pub mod native;
pub mod seam;
mod stable;
mod stream;
mod units;

#[cfg(all(test, feature = "transcribe"))]
mod sweep_live;
#[cfg(test)]
mod test_engine;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_behaviour;

pub use adapter::{AsrParams, AsrStats, LoadError, PROVIDER_ID, QwenAsr, QwenAsrConfig};
pub use seam::{EngineError, RecognizeEngine};
