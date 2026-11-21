//! What the loop tells its host and what the host can tell the loop.

use super::fault::AssistantFault;

/// What the user would see: the assistant's state as the orb shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// No session.
    Idle,
    /// The microphone is open; nobody speaks.
    Listening,
    /// The user is speaking.
    UserSpeaking,
    /// The user's turn is with the dialogue backend; nothing is heard yet.
    Thinking,
    /// The reply plays.
    Speaking,
}

impl Phase {
    /// The state name the desktop UI's view model uses.
    pub fn ui_name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Listening => "Listening",
            Self::UserSpeaking => "UserSpeaking",
            Self::Thinking => "Thinking",
            Self::Speaking => "AssistantSpeaking",
        }
    }
}

/// One thing the loop reports. The host drains them; the loop blocks when the host does not.
#[derive(Clone, Debug, PartialEq)]
pub enum AssistantEvent {
    /// The microphone is open on this device.
    Started {
        /// The device, for display.
        device: String,
    },
    /// The state the orb shows changed.
    Phase(Phase),
    /// What the user is saying, as far as it is recognised (the whole turn so far).
    Partial {
        /// The words.
        text: String,
    },
    /// The user's turn is final and went to the dialogue backend.
    UserTurn {
        /// The words.
        text: String,
    },
    /// More of the assistant's reply text.
    ReplyDelta {
        /// The text to append.
        text: String,
    },
    /// The whole reply was heard.
    ReplyDone,
    /// The reply was cut (the user spoke, or clicked, or stopped the session).
    ReplyCut,
    /// The microphone is closed because the assistant's own voice would be heard (half duplex).
    MicPaused(bool),
    /// Something failed. A fatal fault is followed by [`AssistantEvent::Ended`].
    Fault(AssistantFault),
    /// The session is over: the microphone is released and no thread of the session runs.
    Ended,
}

/// What the host can ask of a running loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Cut what the assistant says now.
    Interrupt,
    /// End the session.
    Stop,
}
