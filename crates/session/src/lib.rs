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

pub mod adapter;
pub mod driver;
pub mod interpret;

mod config;
mod error;
mod event;
mod ids;
mod manager;
mod session;
mod state;
mod turn;

pub use config::{InterruptMode, SessionConfig, TranscriptMode};
pub use error::SessionError;
pub use event::SessionEvent;
pub use ids::{SessionId, TurnId};
pub use manager::SessionManager;
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
