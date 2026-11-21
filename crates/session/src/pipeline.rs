//! One direction of the call as a streaming pipeline that a coordinator can start, feed, pause, cancel and stop (pipeline-design sections 1 to 3, CS-2.4).
//!
//! ```text
//! frames -> [Q1] recognize -> [Q2] control -> [Q3] refine -> [Q2]
//!                                  |  \--[Q4] translate --[Q5] synthesize -> [Q2]
//!                                  |
//!                                  \--[Q6 hold, ordered release]--[Q7] events -> caller
//! ```
//!
//! # Threads (one set per direction, all named `prxvc-<direction>-<role>`)
//!
//! | Thread | Does | Owns |
//! |---|---|---|
//! | `recognize` | re-frames audio for the VAD, segments speech, feeds the streaming recogniser, applies the recognition commit policy, finalises at clause pauses and segment ends | the VAD engine, the recognition stream, `Segmenter` |
//! | `refine` (two-pass only) | decodes the audio of each committed clause again | nothing |
//! | `translate` | one streamed request per clause with `mt_context_clauses` previous pairs of context; cuts the token stream into pieces by `decide_tts_commit` | the context history |
//! | `synthesize` | one streamed request per piece, with the direction's voice | nothing |
//! | `control` | the single writer of the direction machine and the segment book (CS-2.5.2); executes their actions; publishes snapshots | `Core` |
//! | `capture` (optional) | pulls frames from an `AudioSource` into Q1 | the source |
//!
//! Stage threads never touch a state machine: they send envelopes to the
//! control thread, which feeds them to `SegmentBook::on` and
//! `DirectionState::on` and executes the actions that come back. What the
//! listener hears changes only through the direction machine's `ChangeOutput`,
//! which the pipeline surfaces as [`PipelineEvent::Output`].
//!
//! # Queues
//!
//! No unbounded channel exists. Capacities are in [`QueueCaps`]; the policy of
//! each queue is fixed here and enforced by `queue::BoundedQueue`.
//!
//! | Queue | From to | Capacity | Policy when full |
//! |---|---|---|---|
//! | Q1 `audio_in` | caller to `recognize` | 2000 ms of audio | drop the oldest frames, count `capture_overruns`, mark the next frame as a discontinuity; sustained drops raise `DegradeReason::CaptureOverrun` |
//! | Q2 `inbox` | stages and caller commands to `control` | 256 events | block the producer (a blocked push ends when its scope is cancelled or the pipeline closes); partials of one segment coalesce in place; commands use a bounded urgent lane that ignores the capacity |
//! | Q3 `refine_in` | `control` to `refine` | 8 clauses | drop the oldest: that clause keeps its streaming text (`refine_fallbacks`) |
//! | Q4 `mt_in` | `control` to `translate` | 8 clauses | drop the oldest: that clause is skipped (`SkipReason::QueueFull`) and reported |
//! | Q5 `tts_in` | `translate` to `synthesize` | 8 pieces | block the producer (backpressure upstream) |
//! | Q6 hold | `synthesize` through `control`, until the clause is next in order | 20 s of audio in total | fail the clause (`hold_overflows`), the direction carries on |
//! | Q7 `events_out` | `control` to the caller | 256 events | block `control` (the caller must drain); closed at stop |
//! | snapshots | `control` to the UI | 1 slot | latest wins |
//!
//! # Cancellation
//!
//! `TokenTree` is the tree call, direction, segment, clause. The machines'
//! cancel actions are executed on the control thread: cancel the token of the
//! scope (which cancels every provider request below it), purge the queued
//! items whose token is cancelled (only those), and tell the caller to flush
//! what it was already handed ([`PipelineEvent::Flush`]). A result that still
//! arrives for a cancelled, superseded or finished segment or clause is
//! dropped by the book (`DropLate`) or by the stage that noticed, counted in
//! `late_dropped`, and never spoken.
//!
//! # Playback order
//!
//! `SegmentBook` releases only the first unfinished clause for playback. The
//! pipeline holds the audio of later clauses until the caller reports, with
//! `playback_started` and `playback_finished`, that the earlier clause played.
//!
//! The configuration of a direction is [`PipelineParams`]; the public entry is
//! [`DirectionPipeline::start`].

mod align;
mod core;
mod events;
mod faults;
mod handle;
mod input;
mod messages;
mod outbox;
mod params;
mod pieces;
mod queue;
mod recognition;
mod refine;
mod segmenter;
mod self_echo;
mod snapshot;
mod synth;
mod trace;
mod translate;
mod tree;
mod utterance;
mod workers;

// The recognition stage is shared with the assistant's loop (`crate::assistant`).
pub(crate) use messages::RecognitionEvent;
pub(crate) use recognition::{RecognitionFault, RecognitionStage};
pub(crate) use snapshot::Stats;
pub(crate) use trace::Tracer;
pub(crate) use tree::TokenTree;

pub use events::{ClauseAudio, PipelineEvent};
pub use handle::{
    DirectionPipeline, EventReceiver, PipelineError, PipelineProviders, PipelineStats, Started,
    StopReport,
};
pub use input::{AudioInput, PushOutcome};
pub use params::{PipelineParams, QueueCaps, RecognitionMode, RecoveryParams};
pub use queue::{Policy, QueueStats};
pub use utterance::Percentiles;
pub use snapshot::{
    ClauseSnapshot, Counters, DirectionSnapshot, ErrorSnapshot, Ledger, PartialSnapshot, SkipReason, SkippedClause,
    SnapshotSubscriber,
};
