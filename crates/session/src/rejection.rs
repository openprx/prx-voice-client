//! The typed rejection returned by a state machine for an event that is illegal in its current state.

use std::fmt::Debug;

/// The machine was in `from` and `event` is not a legal event there.
///
/// The rejected state is handed back so that the caller keeps it, logs the
/// rejection at `warn` and carries on (CS-2.3.4); nothing is ever forced.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("event {event:?} is not valid in state {from:?}")]
pub struct InvalidTransition<S: Debug, E: Debug> {
    /// The state the machine stays in.
    pub from: S,
    /// The rejected event.
    pub event: E,
}

impl<S: Debug, E: Debug> InvalidTransition<S, E> {
    /// Reject `event` in state `from`.
    pub fn new(from: S, event: E) -> Self {
        Self { from, event }
    }
}
