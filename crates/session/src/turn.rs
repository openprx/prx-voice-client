//! Conversational turn records kept inside a session.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{SessionId, TurnId};

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
