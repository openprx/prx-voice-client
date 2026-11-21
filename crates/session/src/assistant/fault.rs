//! What can go wrong in the assistant's loop, as typed values the host maps onto issues the user can act on.

use prxvc_ports::{DialogueError, EndpointError, TtsError};

use crate::fault::{FaultKind, Stage};

/// A fault of the assistant's loop or of one turn of it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AssistantFault {
    /// The dialogue backend failed: a server that does not answer, a rejected request, a timeout.
    Dialogue(DialogueError),
    /// Speech synthesis failed.
    Synthesis(TtsError),
    /// Speech synthesis finished a reply without producing any audio.
    NoAudio,
    /// Voice activity detection or recognition failed while listening.
    Listening {
        /// The stage that failed (`Vad` or `Asr`).
        stage: Stage,
        /// How it failed.
        kind: FaultKind,
        /// What the stage said (no spoken content).
        detail: String,
    },
    /// The microphone failed or was lost.
    Microphone(EndpointError),
    /// The output device refused the reply's audio.
    Output(String),
}

impl AssistantFault {
    /// Whether the assistant can go on listening after this fault (a failed turn), or the session
    /// cannot continue (the microphone, recognition).
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Self::Listening { .. } | Self::Microphone(_) | Self::Output(_)
        )
    }
}

impl std::fmt::Display for AssistantFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dialogue(e) => write!(f, "{e}"),
            Self::Synthesis(e) => write!(f, "{e}"),
            Self::NoAudio => f.write_str("the reply produced no audio"),
            Self::Listening { stage, detail, .. } => {
                write!(f, "listening failed at {stage:?}: {detail}")
            }
            Self::Microphone(e) => write!(f, "{e}"),
            Self::Output(detail) => write!(f, "the output failed: {detail}"),
        }
    }
}
