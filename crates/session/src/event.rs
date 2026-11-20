//! Events emitted when a session changes lifecycle state.

use crate::ids::SessionId;
use crate::state::SessionState;

/// Events emitted as a result of session lifecycle changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// A new session was created.
    Created(SessionId),
    /// A session transitioned between states.
    StateChanged {
        /// The session that changed.
        id: SessionId,
        /// Previous state.
        from: SessionState,
        /// New state.
        to: SessionState,
    },
    /// A session completed normally.
    Completed(SessionId),
    /// A session failed.
    Failed {
        /// The session that failed.
        id: SessionId,
        /// Human-readable failure reason.
        reason: String,
    },
}
