//! Identifier types for sessions and turns.

use serde::{Deserialize, Serialize};

/// Unique identifier for a voice session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub uuid::Uuid);

/// Monotonically increasing turn counter within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnId(pub u64);
