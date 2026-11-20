//! Shared value types of the call-translation pipeline (layer L0).
//!
//! This file only declares the modules and re-exports the public API. The
//! crate has no dependency on another workspace crate and performs no I/O.
//!
//! * `time`: [`MonoTime`], the [`Clock`] abstraction, [`SystemClock`] and the
//!   manually advanced [`ManualClock`] for tests.
//! * `rate`: [`SampleRate`], a validated non-zero sample rate.
//! * `audio`: [`AudioFrame`] (mono f32 with rate, capture time and sequence)
//!   and [`PcmChunk`] (audio produced by a synthesiser).
//! * `lang`: [`Lang`], the languages the pipeline translates between.
//! * `ids`: [`Direction`], [`SegmentId`] and [`ClauseId`].
//! * `clause_split`: the incremental splitter from translated text to clauses.
//! * `text_units`: the comparison units of a transcript (one per CJK character, one per other word).
//! * `envelope`: [`Envelope`] and the [`RevisionTracker`] that drops late results.

pub mod audio;
pub mod clause_split;
pub mod envelope;
pub mod ids;
pub mod lang;
pub mod rate;
pub mod text_units;
pub mod time;

pub use audio::{AudioFrame, FrameError, PcmChunk};
pub use clause_split::{ClauseSplitter, SplitParams, first_clause_end, trim_stray_start};
pub use envelope::{Envelope, EnvelopeKey, RevisionTracker};
pub use ids::{ClauseId, Direction, SegmentId};
pub use lang::{Lang, UnknownLang};
pub use rate::{InvalidSampleRate, SampleRate};
pub use time::{Clock, ManualClock, MonoTime, SystemClock};
