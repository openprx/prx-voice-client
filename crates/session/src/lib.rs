//! Session state machine for the prx-voice-client system.
//!
//! Manages the lifecycle of a single voice session from creation through
//! completion or failure, enforcing valid state transitions. Also provides
//! turn management and a [`SessionManager`] for tracking multiple sessions.
//!
//! Module map: `ids`, `config`, `state` (states and transition table),
//! `error`, `event`, `turn`, `session` (the entity), `manager`; plus the
//! interpreting machine in `interpret`, the `driver` that runs it and the
//! `adapter` seam traits.
//!
//! The call-translation machines are pure and synchronous (no threads, no
//! I/O, time only through events or an explicit timestamp):
//!
//! * `call_state`: the call session machine (table 3.6).
//! * `direction_state`: the per-direction machine (table 3.7) with the
//!   audible output of each state; `output` holds the output vocabulary.
//! * `segment`: the segment and clause lifecycle tables (table 3.8);
//!   `segment_book`: the per-direction registry that orders clauses and drops
//!   late results.
//! * `commit_policy`: when text commits to a clause and to synthesis.
//! * `interrupt`: what an interruption cancels (D-P5).
//! * `backlog`: which queued clauses are skipped when a direction lags (3.4.3).
//!
//! The direction pipeline that runs these machines on threads is the module
//! `pipeline`.
//! * `fault`, `dir_set`, `rejection`: the shared vocabulary.
//! * `call` (layer L2): the call coordinator that owns these machines, with
//!   its host seams, snapshot, commands and typed errors.

pub mod adapter;
pub mod call;
pub mod driver;
pub mod interpret;
pub mod pipeline;

mod backlog;
mod call_state;
mod commit_policy;
mod config;
mod dir_set;
mod direction_state;
mod error;
mod event;
mod fault;
mod ids;
mod interrupt;
mod manager;
mod output;
mod rejection;
mod segment;
mod segment_book;
mod session;
mod state;
mod turn;

pub use backlog::{BacklogClause, BacklogDecision, SkippingChange, decide_backlog};
pub use call_state::{
    CallAction, CallEvent, CallReason, CallReasons, CallRejection, CallState, ProviderProgress,
};
pub use commit_policy::{
    ClauseReason, CommitDecision, CommitParams, CutPoint, EndReason, RecognitionInput, Speculation,
    TranslationProgress, TtsCommit, check_speculation, decide_recognition_commit,
    decide_tts_commit, is_clause_sized,
};
pub use config::{InterruptMode, SessionConfig, TranscriptMode};
pub use dir_set::DirSet;
pub use direction_state::{
    DirectionAction, DirectionEvent, DirectionRejection, DirectionState, Reasons,
};
pub use error::SessionError;
pub use event::SessionEvent;
pub use fault::{Blocker, DegradeReason, Endpoint, FaultKind, ProviderRole, Stage};
pub use ids::{SessionId, TurnId};
pub use interrupt::{CancelAction, InterruptParams, InterruptTrigger, decide_interrupt};
pub use manager::SessionManager;
pub use output::{DirectionConfig, OutputCause, OutputPolicy, OutputSource, Passthrough};
pub use rejection::InvalidTransition;
pub use segment::{
    ClauseEventKind, ClauseState, ClauseView, Outcome, PlaybackState, SegmentEventKind,
    SegmentState,
};
pub use segment_book::{
    Activity, LateReason, SegmentAction, SegmentBook, SegmentEvent, SegmentRejection,
};
pub use session::Session;
pub use state::SessionState;
pub use turn::{Turn, TurnStatus};

/// Convenience re-exports for downstream crates.
pub mod prelude {
    pub use crate::{
        InterruptMode, Session, SessionConfig, SessionError, SessionEvent, SessionId,
        SessionManager, SessionState, TranscriptMode, Turn, TurnId, TurnStatus,
    };
}
