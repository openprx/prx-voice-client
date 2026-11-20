//! Assistant session states and the table of valid transitions between them.

use serde::{Deserialize, Serialize};

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

impl SessionState {
    /// Check whether transitioning from `from` to `to` is permitted.
    pub(crate) fn is_valid_transition(from: SessionState, to: SessionState) -> bool {
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

#[cfg(test)]
mod tests;
