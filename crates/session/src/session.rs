//! The assistant `Session` entity: state changes through the transition table, and turn bookkeeping.

use chrono::{DateTime, Utc};

use crate::config::SessionConfig;
use crate::error::SessionError;
use crate::event::SessionEvent;
use crate::ids::{SessionId, TurnId};
use crate::state::SessionState;
use crate::turn::{Turn, TurnStatus};

/// A single voice-assistant session with enforced state transitions.
#[derive(Debug, Clone)]
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Current session state.
    pub state: SessionState,
    /// Session configuration.
    pub config: SessionConfig,
    /// Timestamp when the session was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last state change.
    pub updated_at: DateTime<Utc>,
    /// Timestamp when the session ended, if it has.
    pub ended_at: Option<DateTime<Utc>>,
    /// Number of conversational turns completed so far.
    pub turn_count: u64,
    /// Active and completed turns for this session.
    pub turns: Vec<Turn>,
}

impl Session {
    /// Create a new session with the given configuration.
    ///
    /// The session starts in [`SessionState::Idle`] with a freshly generated
    /// UUID v7 identifier and timestamps set to the current time.
    pub fn new(config: SessionConfig) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId(uuid::Uuid::now_v7()),
            state: SessionState::Idle,
            config,
            created_at: now,
            updated_at: now,
            ended_at: None,
            turn_count: 0,
            turns: Vec::new(),
        }
    }

    /// Create a new session with a specific identifier (primarily for testing).
    ///
    /// Behaves identically to [`Session::new`] but uses the supplied `id`
    /// instead of generating one.
    pub fn with_id(id: SessionId, config: SessionConfig) -> Self {
        let now = Utc::now();
        Self {
            id,
            state: SessionState::Idle,
            config,
            created_at: now,
            updated_at: now,
            ended_at: None,
            turn_count: 0,
            turns: Vec::new(),
        }
    }

    /// Attempt to transition the session to a new state.
    ///
    /// Returns a [`SessionEvent`] describing the transition on success,
    /// or a [`SessionError`] if the transition is not valid from the
    /// current state.
    pub fn transition(&mut self, to: SessionState) -> Result<SessionEvent, SessionError> {
        if !SessionState::is_valid_transition(self.state, to) {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to,
            });
        }

        let from = self.state;
        self.state = to;
        self.updated_at = Utc::now();

        match to {
            SessionState::Completed => {
                self.ended_at = Some(self.updated_at);
                Ok(SessionEvent::Completed(self.id))
            }
            SessionState::Failed => {
                self.ended_at = Some(self.updated_at);
                Ok(SessionEvent::StateChanged {
                    id: self.id,
                    from,
                    to,
                })
            }
            _ => Ok(SessionEvent::StateChanged {
                id: self.id,
                from,
                to,
            }),
        }
    }

    /// Start a new conversational turn.
    ///
    /// A turn can only be started when the session is in
    /// [`SessionState::Listening`] or [`SessionState::UserSpeaking`].
    /// Increments `turn_count` and returns the new [`TurnId`].
    pub fn start_turn(&mut self) -> Result<TurnId, SessionError> {
        if self.state != SessionState::Listening && self.state != SessionState::UserSpeaking {
            return Err(SessionError::InvalidTurnState { state: self.state });
        }

        self.turn_count += 1;
        let turn_id = TurnId(self.turn_count);
        let turn = Turn {
            id: turn_id,
            session_id: self.id,
            started_at: Utc::now(),
            ended_at: None,
            user_transcript: None,
            assistant_text: None,
            status: TurnStatus::Active,
        };
        self.turns.push(turn);
        Ok(turn_id)
    }

    /// End an existing conversational turn.
    ///
    /// Marks the turn as [`TurnStatus::Completed`] and records the end time.
    /// Returns an error if the turn is not found in this session.
    pub fn end_turn(&mut self, id: TurnId) -> Result<(), SessionError> {
        let turn = self
            .turns
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(SessionError::TurnNotFound(id))?;
        turn.ended_at = Some(Utc::now());
        turn.status = TurnStatus::Completed;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
