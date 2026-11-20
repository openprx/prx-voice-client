//! Errors raised by session operations.

use crate::ids::TurnId;
use crate::state::SessionState;

/// Errors that can occur during session operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SessionError {
    /// The requested state transition is not allowed.
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition {
        /// State the session was in.
        from: SessionState,
        /// State that was requested.
        to: SessionState,
    },

    /// Attempted to start a session that is already started.
    #[error("session already started")]
    AlreadyStarted,

    /// Attempted an operation that requires a started session.
    #[error("session not started")]
    NotStarted,

    /// Attempted to act on a session that has already completed.
    #[error("session already completed")]
    AlreadyCompleted,

    /// Attempted to start a turn in an invalid state.
    #[error("cannot start turn in state {state}")]
    InvalidTurnState {
        /// The state the session was in when the turn was attempted.
        state: SessionState,
    },

    /// The specified turn was not found.
    #[error("turn {0:?} not found")]
    TurnNotFound(TurnId),
}
