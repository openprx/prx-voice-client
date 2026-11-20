//! Session state machine for the prx-voice-client system.
//!
//! Manages the lifecycle of a single voice session from creation through
//! completion or failure, enforcing valid state transitions. Also provides
//! turn management and a [`SessionManager`] for tracking multiple sessions.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod adapter;
pub mod driver;
pub mod interpret;

// ---------------------------------------------------------------------------
// Identity types
// ---------------------------------------------------------------------------

/// Unique identifier for a voice session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub uuid::Uuid);

/// Monotonically increasing turn counter within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnId(pub u64);

// ---------------------------------------------------------------------------
// Session configuration
// ---------------------------------------------------------------------------

/// Controls how the user can interrupt assistant playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterruptMode {
    /// User must explicitly press a button to interrupt.
    Manual,
    /// Voice activity detection triggers an interrupt automatically.
    VoiceActivity,
}

/// Controls transcript generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptMode {
    /// No transcript output.
    Off,
    /// Emit interim (partial) transcripts only.
    InterimOnly,
    /// Emit both interim and final transcripts.
    Full,
}

/// Per-session configuration supplied at creation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Language for speech recognition input.
    pub input_language: String,
    /// Language for assistant speech output.
    pub output_language: String,
    /// Whether to automatically play the assistant reply audio.
    pub auto_play_reply: bool,
    /// Whether to persist conversation history.
    pub save_history: bool,
    /// How the user can interrupt playback.
    pub interrupt_mode: InterruptMode,
    /// What level of transcription to produce.
    pub transcript_mode: TranscriptMode,
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// All possible states a voice session can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session created but not yet started.
    Idle,
    /// Waiting for wake-word detection before starting.
    WakePending,
    /// Session is initializing (connecting to services, etc.).
    Starting,
    /// Session is actively listening for user speech.
    Listening,
    /// User speech detected and being captured.
    UserSpeaking,
    /// User stopped speaking; audio captured.
    UserSpeechEnded,
    /// Speech-to-text recognition in progress.
    Recognizing,
    /// Waiting for the assistant model to generate a reply.
    Thinking,
    /// Assistant reply audio is being played.
    AssistantSpeaking,
    /// Playback was interrupted by the user.
    Interrupted,
    /// Session is temporarily paused by the user.
    Paused,
    /// Recovering from a transient error.
    Recovering,
    /// Session finished normally.
    Completed,
    /// Session terminated due to an unrecoverable error.
    Failed,
}

impl SessionState {
    /// Returns the i18n translation key for this state.
    ///
    /// The session crate does not depend on i18n — it only returns key strings.
    /// The UI layer is responsible for calling `t()` on the returned key.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            Self::Idle => "state.idle",
            Self::WakePending => "state.standby",
            Self::Starting => "state.starting",
            Self::Listening => "state.listening",
            Self::UserSpeaking => "state.user_speaking",
            Self::UserSpeechEnded => "state.recognizing",
            Self::Recognizing => "state.recognizing",
            Self::Thinking => "state.thinking",
            Self::AssistantSpeaking => "state.assistant_speaking",
            Self::Interrupted => "state.interrupted",
            Self::Paused => "state.paused",
            Self::Recovering => "state.recovering",
            Self::Completed => "state.completed",
            Self::Failed => "state.failed",
        }
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ---------------------------------------------------------------------------
// Session errors
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Session events
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Turn
// ---------------------------------------------------------------------------

/// Status of a conversational turn within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStatus {
    /// The turn is currently in progress.
    Active,
    /// The turn completed normally.
    Completed,
    /// The turn was interrupted by the user.
    Interrupted,
    /// The turn failed due to an error.
    Failed,
}

/// A single conversational turn within a session.
///
/// A turn represents one user-utterance / assistant-reply cycle. It tracks
/// timing, transcript content, and completion status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    /// Unique turn identifier (monotonically increasing within a session).
    pub id: TurnId,
    /// The session this turn belongs to.
    pub session_id: SessionId,
    /// When the turn started.
    pub started_at: DateTime<Utc>,
    /// When the turn ended, if it has.
    pub ended_at: Option<DateTime<Utc>>,
    /// Transcribed user speech for this turn.
    pub user_transcript: Option<String>,
    /// Assistant text reply for this turn.
    pub assistant_text: Option<String>,
    /// Current status of the turn.
    pub status: TurnStatus,
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

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
        if !Self::is_valid_transition(self.state, to) {
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

    /// Check whether transitioning from `from` to `to` is permitted.
    fn is_valid_transition(from: SessionState, to: SessionState) -> bool {
        matches!(
            (from, to),
            // Idle
            (SessionState::Idle, SessionState::Starting)
            // WakePending
            | (SessionState::WakePending, SessionState::Starting)
            | (SessionState::WakePending, SessionState::Failed)
            // Starting
            | (SessionState::Starting, SessionState::Listening)
            | (SessionState::Starting, SessionState::Failed)
            // Listening
            | (SessionState::Listening, SessionState::UserSpeaking)
            | (SessionState::Listening, SessionState::Paused)
            | (SessionState::Listening, SessionState::Completed)
            | (SessionState::Listening, SessionState::Recovering)
            | (SessionState::Listening, SessionState::Failed)
            // UserSpeaking
            | (SessionState::UserSpeaking, SessionState::UserSpeechEnded)
            | (SessionState::UserSpeaking, SessionState::Interrupted)
            | (SessionState::UserSpeaking, SessionState::Failed)
            // UserSpeechEnded
            | (SessionState::UserSpeechEnded, SessionState::Recognizing)
            | (SessionState::UserSpeechEnded, SessionState::Failed)
            // Recognizing
            | (SessionState::Recognizing, SessionState::Thinking)
            | (SessionState::Recognizing, SessionState::Recovering)
            | (SessionState::Recognizing, SessionState::Failed)
            // Thinking
            | (SessionState::Thinking, SessionState::AssistantSpeaking)
            // A newer utterance replaces a turn whose reply never spoke.
            | (SessionState::Thinking, SessionState::Listening)
            | (SessionState::Thinking, SessionState::Recovering)
            | (SessionState::Thinking, SessionState::Failed)
            // AssistantSpeaking
            | (SessionState::AssistantSpeaking, SessionState::Interrupted)
            | (SessionState::AssistantSpeaking, SessionState::Listening)
            | (SessionState::AssistantSpeaking, SessionState::Completed)
            | (SessionState::AssistantSpeaking, SessionState::Recovering)
            | (SessionState::AssistantSpeaking, SessionState::Failed)
            // Interrupted
            | (SessionState::Interrupted, SessionState::Listening)
            | (SessionState::Interrupted, SessionState::Completed)
            | (SessionState::Interrupted, SessionState::Failed)
            // Paused
            | (SessionState::Paused, SessionState::Listening)
            | (SessionState::Paused, SessionState::Completed)
            | (SessionState::Paused, SessionState::Failed)
            // Recovering
            | (SessionState::Recovering, SessionState::Listening)
            | (SessionState::Recovering, SessionState::Failed)
        )
    }
}

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

/// Manages a collection of active and completed sessions.
///
/// Provides CRUD operations and queries over the set of sessions currently
/// held in memory.
#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
}

impl SessionManager {
    /// Create a new, empty session manager.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Create a new session with the given configuration and return its id.
    pub fn create_session(&mut self, config: SessionConfig) -> SessionId {
        let session = Session::new(config);
        let id = session.id;
        self.sessions.insert(id, session);
        id
    }

    /// Get a shared reference to a session by id.
    pub fn get_session(&self, id: &SessionId) -> Option<&Session> {
        self.sessions.get(id)
    }

    /// Get a mutable reference to a session by id.
    pub fn get_session_mut(&mut self, id: &SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    /// Remove a session by id, returning it if it existed.
    pub fn remove_session(&mut self, id: &SessionId) -> Option<Session> {
        self.sessions.remove(id)
    }

    /// Return references to all sessions that are not in
    /// [`SessionState::Completed`] or [`SessionState::Failed`].
    pub fn active_sessions(&self) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|s| s.state != SessionState::Completed && s.state != SessionState::Failed)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for downstream crates.
pub mod prelude {
    pub use crate::{
        InterruptMode, Session, SessionConfig, SessionError, SessionEvent, SessionId,
        SessionManager, SessionState, TranscriptMode, Turn, TurnId, TurnStatus,
    };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a default test config.
    fn test_config() -> SessionConfig {
        SessionConfig {
            input_language: "en".to_string(),
            output_language: "en".to_string(),
            auto_play_reply: true,
            save_history: false,
            interrupt_mode: InterruptMode::VoiceActivity,
            transcript_mode: TranscriptMode::Full,
        }
    }

    #[test]
    fn test_session_new_starts_idle() {
        let session = Session::new(test_config());
        assert_eq!(session.state, SessionState::Idle);
        assert!(session.ended_at.is_none());
        assert_eq!(session.turn_count, 0);
        assert!(session.turns.is_empty());
    }

    #[test]
    fn test_valid_transitions_happy_path() {
        let mut s = Session::new(test_config());

        // Idle -> Starting -> Listening -> UserSpeaking -> UserSpeechEnded
        // -> Recognizing -> Thinking -> AssistantSpeaking -> Listening -> Completed
        s.transition(SessionState::Starting).unwrap();
        assert_eq!(s.state, SessionState::Starting);

        s.transition(SessionState::Listening).unwrap();
        assert_eq!(s.state, SessionState::Listening);

        s.transition(SessionState::UserSpeaking).unwrap();
        assert_eq!(s.state, SessionState::UserSpeaking);

        s.transition(SessionState::UserSpeechEnded).unwrap();
        assert_eq!(s.state, SessionState::UserSpeechEnded);

        s.transition(SessionState::Recognizing).unwrap();
        assert_eq!(s.state, SessionState::Recognizing);

        s.transition(SessionState::Thinking).unwrap();
        assert_eq!(s.state, SessionState::Thinking);

        s.transition(SessionState::AssistantSpeaking).unwrap();
        assert_eq!(s.state, SessionState::AssistantSpeaking);

        s.transition(SessionState::Listening).unwrap();
        assert_eq!(s.state, SessionState::Listening);

        let event = s.transition(SessionState::Completed).unwrap();
        assert_eq!(s.state, SessionState::Completed);
        assert!(matches!(event, SessionEvent::Completed(_)));
    }

    #[test]
    fn test_interrupt_path() {
        let mut s = Session::new(test_config());
        s.transition(SessionState::Starting).unwrap();
        s.transition(SessionState::Listening).unwrap();
        s.transition(SessionState::UserSpeaking).unwrap();
        s.transition(SessionState::UserSpeechEnded).unwrap();
        s.transition(SessionState::Recognizing).unwrap();
        s.transition(SessionState::Thinking).unwrap();
        s.transition(SessionState::AssistantSpeaking).unwrap();

        // Interrupt and return to listening
        s.transition(SessionState::Interrupted).unwrap();
        assert_eq!(s.state, SessionState::Interrupted);

        s.transition(SessionState::Listening).unwrap();
        assert_eq!(s.state, SessionState::Listening);
    }

    #[test]
    fn test_recovery_path() {
        let mut s = Session::new(test_config());
        s.transition(SessionState::Starting).unwrap();
        s.transition(SessionState::Listening).unwrap();

        // Listening -> Recovering -> Listening
        s.transition(SessionState::Recovering).unwrap();
        assert_eq!(s.state, SessionState::Recovering);

        s.transition(SessionState::Listening).unwrap();
        assert_eq!(s.state, SessionState::Listening);
    }

    #[test]
    fn test_failure_from_any_state() {
        // Test that Failed is reachable from multiple states.
        let states_that_can_fail = [
            SessionState::Starting,
            SessionState::Listening,
            SessionState::UserSpeaking,
            SessionState::UserSpeechEnded,
            SessionState::Recognizing,
            SessionState::Thinking,
            SessionState::AssistantSpeaking,
            SessionState::Interrupted,
            SessionState::Paused,
            SessionState::Recovering,
        ];

        for &state in &states_that_can_fail {
            let id = SessionId(uuid::Uuid::now_v7());
            let mut s = Session::with_id(id, test_config());
            // Force state directly for testing reachability of Failed.
            s.state = state;
            let result = s.transition(SessionState::Failed);
            assert!(result.is_ok(), "Failed should be reachable from {state:?}");
            assert_eq!(s.state, SessionState::Failed);
            assert!(s.ended_at.is_some());
        }
    }

    #[test]
    fn test_thinking_can_return_to_listening_when_the_reply_never_speaks() {
        let mut s = Session::new(test_config());
        for to in [
            SessionState::Starting,
            SessionState::Listening,
            SessionState::UserSpeaking,
            SessionState::UserSpeechEnded,
            SessionState::Recognizing,
            SessionState::Thinking,
            SessionState::Listening,
        ] {
            s.transition(to).expect("valid transition");
        }
        assert_eq!(s.state, SessionState::Listening);
    }

    #[test]
    fn test_invalid_transition_idle_to_listening() {
        let mut s = Session::new(test_config());
        let result = s.transition(SessionState::Listening);
        assert!(result.is_err());
        match result {
            Err(SessionError::InvalidTransition { from, to }) => {
                assert_eq!(from, SessionState::Idle);
                assert_eq!(to, SessionState::Listening);
            }
            _ => panic!("expected InvalidTransition error"),
        }
    }

    #[test]
    fn test_invalid_transition_completed_to_any() {
        let mut s = Session::new(test_config());
        s.transition(SessionState::Starting).unwrap();
        s.transition(SessionState::Listening).unwrap();
        s.transition(SessionState::Completed).unwrap();

        // From Completed, no transition should be valid.
        let targets = [
            SessionState::Idle,
            SessionState::Starting,
            SessionState::Listening,
            SessionState::UserSpeaking,
            SessionState::Failed,
        ];
        for &target in &targets {
            let result = s.transition(target);
            assert!(
                result.is_err(),
                "transition from Completed to {target:?} should fail"
            );
        }
    }

    #[test]
    fn test_session_timestamps_update_on_transition() {
        let mut s = Session::new(test_config());
        let created = s.created_at;
        let initial_updated = s.updated_at;

        // Small sleep not needed — Utc::now() granularity is sufficient for >=.
        s.transition(SessionState::Starting).unwrap();
        assert_eq!(s.created_at, created, "created_at must not change");
        assert!(
            s.updated_at >= initial_updated,
            "updated_at must advance on transition"
        );
    }

    #[test]
    fn test_session_ended_at_set_on_completion() {
        let mut s = Session::new(test_config());
        assert!(s.ended_at.is_none());

        s.transition(SessionState::Starting).unwrap();
        s.transition(SessionState::Listening).unwrap();
        assert!(s.ended_at.is_none());

        s.transition(SessionState::Completed).unwrap();
        assert!(s.ended_at.is_some());
    }

    #[test]
    fn test_turn_management_basic() {
        let mut s = Session::new(test_config());
        s.transition(SessionState::Starting).unwrap();
        s.transition(SessionState::Listening).unwrap();

        // Start a turn
        let tid = s.start_turn().unwrap();
        assert_eq!(tid, TurnId(1));
        assert_eq!(s.turn_count, 1);
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].status, TurnStatus::Active);

        // End the turn
        s.end_turn(tid).unwrap();
        assert_eq!(s.turns[0].status, TurnStatus::Completed);
        assert!(s.turns[0].ended_at.is_some());

        // Start another turn
        let tid2 = s.start_turn().unwrap();
        assert_eq!(tid2, TurnId(2));
        assert_eq!(s.turn_count, 2);
    }

    #[test]
    fn test_turn_start_requires_listening_state() {
        let mut s = Session::new(test_config());

        // Idle — should fail
        let result = s.start_turn();
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(SessionError::InvalidTurnState {
                state: SessionState::Idle
            })
        ));

        // Move to Starting — should still fail
        s.transition(SessionState::Starting).unwrap();
        let result = s.start_turn();
        assert!(result.is_err());

        // Move to Listening — should succeed
        s.transition(SessionState::Listening).unwrap();
        let tid = s.start_turn().unwrap();
        assert_eq!(tid, TurnId(1));

        // Move to UserSpeaking — should also succeed
        s.transition(SessionState::UserSpeaking).unwrap();
        let tid2 = s.start_turn().unwrap();
        assert_eq!(tid2, TurnId(2));
    }

    #[test]
    fn test_turn_end_not_found() {
        let mut s = Session::new(test_config());
        s.transition(SessionState::Starting).unwrap();
        s.transition(SessionState::Listening).unwrap();

        let result = s.end_turn(TurnId(999));
        assert!(matches!(
            result,
            Err(SessionError::TurnNotFound(TurnId(999)))
        ));
    }

    #[test]
    fn test_session_manager_create_and_get() {
        let mut mgr = SessionManager::new();
        let id = mgr.create_session(test_config());

        let session = mgr.get_session(&id);
        assert!(session.is_some());
        assert_eq!(session.unwrap().id, id);
        assert_eq!(session.unwrap().state, SessionState::Idle);

        // Mutate through manager
        let session_mut = mgr.get_session_mut(&id).unwrap();
        session_mut.transition(SessionState::Starting).unwrap();
        assert_eq!(mgr.get_session(&id).unwrap().state, SessionState::Starting);

        // Remove
        let removed = mgr.remove_session(&id);
        assert!(removed.is_some());
        assert!(mgr.get_session(&id).is_none());
    }

    #[test]
    fn test_session_manager_active_sessions() {
        let mut mgr = SessionManager::new();

        let id1 = mgr.create_session(test_config());
        let id2 = mgr.create_session(test_config());
        let id3 = mgr.create_session(test_config());

        // All three are Idle (active)
        assert_eq!(mgr.active_sessions().len(), 3);

        // Complete session 1
        {
            let s1 = mgr.get_session_mut(&id1).unwrap();
            s1.transition(SessionState::Starting).unwrap();
            s1.transition(SessionState::Listening).unwrap();
            s1.transition(SessionState::Completed).unwrap();
        }

        // Fail session 2
        {
            let s2 = mgr.get_session_mut(&id2).unwrap();
            s2.transition(SessionState::Starting).unwrap();
            s2.transition(SessionState::Failed).unwrap();
        }

        let active = mgr.active_sessions();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id3);
    }

    #[test]
    fn test_with_id_uses_provided_id() {
        let id = SessionId(uuid::Uuid::now_v7());
        let s = Session::with_id(id, test_config());
        assert_eq!(s.id, id);
        assert_eq!(s.state, SessionState::Idle);
    }

    #[test]
    fn test_session_state_i18n_keys() {
        let all_states = [
            SessionState::Idle,
            SessionState::WakePending,
            SessionState::Starting,
            SessionState::Listening,
            SessionState::UserSpeaking,
            SessionState::UserSpeechEnded,
            SessionState::Recognizing,
            SessionState::Thinking,
            SessionState::AssistantSpeaking,
            SessionState::Interrupted,
            SessionState::Paused,
            SessionState::Recovering,
            SessionState::Completed,
            SessionState::Failed,
        ];

        for state in &all_states {
            let key = state.i18n_key();
            assert!(!key.is_empty(), "i18n key for {state:?} must not be empty");
            assert!(
                key.starts_with("state."),
                "i18n key for {state:?} must start with \"state.\", got \"{key}\""
            );
        }
    }
}
