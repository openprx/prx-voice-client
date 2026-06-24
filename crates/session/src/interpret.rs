//! Consecutive-interpreting state machine for bilingual meetings.
//!
//! Turn-based interpreting: one party speaks, the floor is held, the utterance
//! is translated, and the rendered translation is spoken before the floor is
//! released. The per-turn latency is absorbed by turn-taking, so it reads as a
//! natural hand-off rather than lag.
//!
//! Per-turn flow (drives the orb UI directly):
//! ```text
//! Idle ─BeginTurn(dir)→ Listening ─EndUtterance→ Recognizing
//!      ─RecognitionDone→ Translating ─TranslationDone→ Speaking
//!      ─PlaybackDone→ Idle (floor released, history appended)
//! ```
//!
//! Follows the repo convention: **Command → State Transition → Event Emit**, with
//! the state machine as the single source of truth. [`InterpretSession::apply`]
//! is the only mutator; every invalid `(state, command)` pair is rejected.

use serde::{Deserialize, Serialize};

use crate::{SessionId, TurnId};

// ---------------------------------------------------------------------------
// Direction & configuration
// ---------------------------------------------------------------------------

/// Which way an interpreting turn flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// I speak → the peer hears. Source = my language, output = peer's language,
    /// rendered in my cloned voice (direction A in the design doc).
    SelfToPeer,
    /// The peer speaks → I hear. Source = peer's language, output = mine.
    PeerToSelf,
}

impl Direction {
    /// The other direction.
    pub fn flipped(self) -> Self {
        match self {
            Self::SelfToPeer => Self::PeerToSelf,
            Self::PeerToSelf => Self::SelfToPeer,
        }
    }

    /// Language code of the speech being captured for this direction.
    pub fn source_language<'a>(self, cfg: &'a InterpretConfig) -> &'a str {
        match self {
            Self::SelfToPeer => &cfg.self_language,
            Self::PeerToSelf => &cfg.peer_language,
        }
    }

    /// Language code the translation is rendered into for this direction.
    pub fn target_language<'a>(self, cfg: &'a InterpretConfig) -> &'a str {
        match self {
            Self::SelfToPeer => &cfg.peer_language,
            Self::PeerToSelf => &cfg.self_language,
        }
    }

    /// Whether the synthesized output should use the user's cloned voice.
    pub fn uses_clone_voice(self, cfg: &InterpretConfig) -> bool {
        matches!(self, Self::SelfToPeer) && cfg.clone_voice_self_to_peer
    }
}

/// Static configuration for an interpreting session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterpretConfig {
    /// My language code (e.g. `"zh"`).
    pub self_language: String,
    /// The peer's language code (e.g. `"en"`).
    pub peer_language: String,
    /// Render my outgoing (Self→Peer) speech in my cloned voice.
    pub clone_voice_self_to_peer: bool,
}

impl Default for InterpretConfig {
    fn default() -> Self {
        Self {
            self_language: "zh".to_owned(),
            peer_language: "en".to_owned(),
            clone_voice_self_to_peer: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// States of the consecutive-interpreting turn machine. Active states carry the
/// [`Direction`] of the turn in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterpretState {
    /// No turn in progress; the floor is free for either party.
    Idle,
    /// Capturing the active speaker's utterance.
    Listening(Direction),
    /// Running ASR on the captured utterance.
    Recognizing(Direction),
    /// Running machine translation on the recognized text.
    Translating(Direction),
    /// Synthesizing and playing the translated audio to the listener.
    Speaking(Direction),
    /// Session paused by the user; any in-flight turn was discarded.
    Paused,
    /// Unrecoverable error.
    Failed,
}

impl InterpretState {
    /// The direction of the in-progress turn, if any.
    pub fn direction(self) -> Option<Direction> {
        match self {
            Self::Listening(d) | Self::Recognizing(d) | Self::Translating(d) | Self::Speaking(d) => {
                Some(d)
            }
            Self::Idle | Self::Paused | Self::Failed => None,
        }
    }

    /// Whether a turn is currently in progress (floor held).
    pub fn is_active(self) -> bool {
        self.direction().is_some()
    }

    /// i18n key for this state (the UI calls `t()` on it; this crate has no
    /// i18n dependency).
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Idle => "interpret.idle",
            Self::Listening(_) => "interpret.listening",
            Self::Recognizing(_) => "interpret.recognizing",
            Self::Translating(_) => "interpret.translating",
            Self::Speaking(_) => "interpret.speaking",
            Self::Paused => "interpret.paused",
            Self::Failed => "interpret.failed",
        }
    }
}

impl std::fmt::Display for InterpretState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ---------------------------------------------------------------------------
// Commands & events
// ---------------------------------------------------------------------------

/// Inputs that drive the interpreting state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpretCommand {
    /// A party takes the floor and begins speaking (push-to-talk / VAD onset).
    BeginTurn(Direction),
    /// The speaker finished the utterance (push-to-talk release / VAD offset).
    EndUtterance,
    /// ASR produced text for the captured utterance (empty = nothing heard).
    RecognitionDone { text: String },
    /// MT produced the translated text.
    TranslationDone { text: String },
    /// The synthesized translation finished playing to the listener.
    PlaybackDone,
    /// Pause the session, discarding any in-flight turn.
    Pause,
    /// Resume a paused session (returns to [`InterpretState::Idle`]).
    Resume,
    /// Move to [`InterpretState::Failed`] with a reason.
    Fail { reason: String },
    /// Clear error/pause and return to [`InterpretState::Idle`].
    Reset,
}

impl InterpretCommand {
    /// Stable name for diagnostics / errors.
    fn name(&self) -> &'static str {
        match self {
            Self::BeginTurn(_) => "BeginTurn",
            Self::EndUtterance => "EndUtterance",
            Self::RecognitionDone { .. } => "RecognitionDone",
            Self::TranslationDone { .. } => "TranslationDone",
            Self::PlaybackDone => "PlaybackDone",
            Self::Pause => "Pause",
            Self::Resume => "Resume",
            Self::Fail { .. } => "Fail",
            Self::Reset => "Reset",
        }
    }
}

/// A completed interpreting turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterpretTurn {
    /// Monotonic turn id within the session.
    pub id: TurnId,
    /// Direction this turn flowed.
    pub direction: Direction,
    /// Recognized text in the source language.
    pub source_text: String,
    /// Translated text in the target language.
    pub translated_text: String,
}

/// Observable effects of applying a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpretEvent {
    /// The machine moved between states.
    StateChanged {
        /// Previous state.
        from: InterpretState,
        /// New state.
        to: InterpretState,
    },
    /// A new turn started.
    TurnStarted {
        /// The turn id.
        turn: TurnId,
        /// Direction of the turn.
        direction: Direction,
    },
    /// Source-language text was recognized.
    Transcribed {
        /// The turn id.
        turn: TurnId,
        /// Direction of the turn.
        direction: Direction,
        /// Recognized source text.
        text: String,
    },
    /// Target-language translation was produced.
    Translated {
        /// The turn id.
        turn: TurnId,
        /// Direction of the turn.
        direction: Direction,
        /// Translated text.
        text: String,
    },
    /// A turn finished and was appended to history.
    TurnCompleted {
        /// The completed turn record.
        record: InterpretTurn,
    },
    /// The floor was released (turn ended or aborted with no result).
    FloorReleased,
    /// The session was paused.
    Paused,
    /// The session resumed.
    Resumed,
    /// The session failed.
    Failed {
        /// Human-readable reason.
        reason: String,
    },
}

/// Errors from the interpreting state machine.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InterpretError {
    /// The command is not valid in the current state.
    #[error("command {command} not valid in state {state}")]
    InvalidCommand {
        /// Name of the rejected command.
        command: &'static str,
        /// State the machine was in.
        state: InterpretState,
    },
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// In-flight turn being assembled across the state transitions.
#[derive(Debug, Clone)]
struct PartialTurn {
    id: TurnId,
    direction: Direction,
    source_text: String,
    translated_text: String,
}

/// A bidirectional consecutive-interpreting session with enforced transitions.
#[derive(Debug, Clone)]
pub struct InterpretSession {
    /// Session identifier.
    pub id: SessionId,
    /// Static configuration (languages, clone-voice routing).
    pub config: InterpretConfig,
    state: InterpretState,
    next_turn: u64,
    current: Option<PartialTurn>,
    history: Vec<InterpretTurn>,
}

impl InterpretSession {
    /// Create an idle interpreting session.
    pub fn new(id: SessionId, config: InterpretConfig) -> Self {
        Self {
            id,
            config,
            state: InterpretState::Idle,
            next_turn: 0,
            current: None,
            history: Vec::new(),
        }
    }

    /// Current state.
    pub fn state(&self) -> InterpretState {
        self.state
    }

    /// Direction of the in-progress turn, if any.
    pub fn current_direction(&self) -> Option<Direction> {
        self.state.direction()
    }

    /// Completed turns, oldest first.
    pub fn history(&self) -> &[InterpretTurn] {
        &self.history
    }

    /// Apply a command, advancing the state machine and returning the resulting
    /// events (a leading [`InterpretEvent::StateChanged`] whenever the state
    /// actually changed).
    ///
    /// # Errors
    /// [`InterpretError::InvalidCommand`] if the command is not valid in the
    /// current state. On error the state is left unchanged.
    pub fn apply(&mut self, command: InterpretCommand) -> Result<Vec<InterpretEvent>, InterpretError> {
        let from = self.state;
        let mut events = Vec::new();

        match (self.state, command) {
            (InterpretState::Idle, InterpretCommand::BeginTurn(dir)) => {
                let turn = TurnId(self.next_turn);
                self.next_turn += 1;
                self.current = Some(PartialTurn {
                    id: turn,
                    direction: dir,
                    source_text: String::new(),
                    translated_text: String::new(),
                });
                self.state = InterpretState::Listening(dir);
                events.push(InterpretEvent::TurnStarted { turn, direction: dir });
            }

            (InterpretState::Listening(dir), InterpretCommand::EndUtterance) => {
                self.state = InterpretState::Recognizing(dir);
            }

            (InterpretState::Recognizing(dir), InterpretCommand::RecognitionDone { text }) => {
                if text.trim().is_empty() {
                    // Nothing heard — abort the turn, release the floor.
                    self.current = None;
                    self.state = InterpretState::Idle;
                    events.push(InterpretEvent::FloorReleased);
                } else {
                    let turn = self.set_source_text(text.clone());
                    self.state = InterpretState::Translating(dir);
                    events.push(InterpretEvent::Transcribed {
                        turn,
                        direction: dir,
                        text,
                    });
                }
            }

            (InterpretState::Translating(dir), InterpretCommand::TranslationDone { text }) => {
                let turn = self.set_translated_text(text.clone());
                self.state = InterpretState::Speaking(dir);
                events.push(InterpretEvent::Translated {
                    turn,
                    direction: dir,
                    text,
                });
            }

            (InterpretState::Speaking(_), InterpretCommand::PlaybackDone) => {
                if let Some(cur) = self.current.take() {
                    let record = InterpretTurn {
                        id: cur.id,
                        direction: cur.direction,
                        source_text: cur.source_text,
                        translated_text: cur.translated_text,
                    };
                    self.history.push(record.clone());
                    events.push(InterpretEvent::TurnCompleted { record });
                }
                self.state = InterpretState::Idle;
                events.push(InterpretEvent::FloorReleased);
            }

            // Pause is allowed from idle or any active state (discards the turn).
            (state, InterpretCommand::Pause)
                if state != InterpretState::Paused && state != InterpretState::Failed =>
            {
                self.current = None;
                self.state = InterpretState::Paused;
                events.push(InterpretEvent::Paused);
            }

            (InterpretState::Paused, InterpretCommand::Resume) => {
                self.state = InterpretState::Idle;
                events.push(InterpretEvent::Resumed);
            }

            (_, InterpretCommand::Fail { reason }) => {
                self.current = None;
                self.state = InterpretState::Failed;
                events.push(InterpretEvent::Failed { reason });
            }

            (_, InterpretCommand::Reset) => {
                self.current = None;
                self.state = InterpretState::Idle;
            }

            (state, command) => {
                return Err(InterpretError::InvalidCommand {
                    command: command.name(),
                    state,
                });
            }
        }

        if self.state != from {
            events.insert(0, InterpretEvent::StateChanged { from, to: self.state });
        }
        Ok(events)
    }

    /// Store recognized source text on the in-flight turn; returns its id.
    fn set_source_text(&mut self, text: String) -> TurnId {
        match self.current.as_mut() {
            Some(cur) => {
                cur.source_text = text;
                cur.id
            }
            None => TurnId(self.next_turn.saturating_sub(1)),
        }
    }

    /// Store translated text on the in-flight turn; returns its id.
    fn set_translated_text(&mut self, text: String) -> TurnId {
        match self.current.as_mut() {
            Some(cur) => {
                cur.translated_text = text;
                cur.id
            }
            None => TurnId(self.next_turn.saturating_sub(1)),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> InterpretSession {
        InterpretSession::new(SessionId(uuid::Uuid::nil()), InterpretConfig::default())
    }

    fn run(s: &mut InterpretSession, c: InterpretCommand) -> Vec<InterpretEvent> {
        s.apply(c).expect("command should be valid")
    }

    #[test]
    fn full_turn_self_to_peer() {
        let mut s = session();

        let ev = run(&mut s, InterpretCommand::BeginTurn(Direction::SelfToPeer));
        assert_eq!(s.state(), InterpretState::Listening(Direction::SelfToPeer));
        assert!(matches!(ev[0], InterpretEvent::StateChanged { .. }));
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::TurnStarted { .. })));

        run(&mut s, InterpretCommand::EndUtterance);
        assert_eq!(s.state(), InterpretState::Recognizing(Direction::SelfToPeer));

        let ev = run(&mut s, InterpretCommand::RecognitionDone { text: "hello".into() });
        assert_eq!(s.state(), InterpretState::Translating(Direction::SelfToPeer));
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::Transcribed { .. })));

        let ev = run(&mut s, InterpretCommand::TranslationDone { text: "Hello".into() });
        assert_eq!(s.state(), InterpretState::Speaking(Direction::SelfToPeer));
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::Translated { .. })));

        let ev = run(&mut s, InterpretCommand::PlaybackDone);
        assert_eq!(s.state(), InterpretState::Idle);
        assert_eq!(s.history().len(), 1);
        let rec = &s.history()[0];
        assert_eq!(rec.source_text, "hello");
        assert_eq!(rec.translated_text, "Hello");
        assert_eq!(rec.direction, Direction::SelfToPeer);
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::TurnCompleted { .. })));
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::FloorReleased)));
    }

    #[test]
    fn empty_recognition_aborts_turn() {
        let mut s = session();
        run(&mut s, InterpretCommand::BeginTurn(Direction::PeerToSelf));
        run(&mut s, InterpretCommand::EndUtterance);
        let ev = run(&mut s, InterpretCommand::RecognitionDone { text: "   ".into() });
        assert_eq!(s.state(), InterpretState::Idle);
        assert!(s.history().is_empty());
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::FloorReleased)));
    }

    #[test]
    fn alternating_turns_increment_ids() {
        let mut s = session();
        for dir in [Direction::SelfToPeer, Direction::PeerToSelf] {
            run(&mut s, InterpretCommand::BeginTurn(dir));
            run(&mut s, InterpretCommand::EndUtterance);
            run(&mut s, InterpretCommand::RecognitionDone { text: "x".into() });
            run(&mut s, InterpretCommand::TranslationDone { text: "y".into() });
            run(&mut s, InterpretCommand::PlaybackDone);
        }
        assert_eq!(s.history().len(), 2);
        assert_eq!(s.history()[0].id, TurnId(0));
        assert_eq!(s.history()[1].id, TurnId(1));
        assert_eq!(s.history()[1].direction, Direction::PeerToSelf);
    }

    #[test]
    fn invalid_command_is_rejected_and_state_unchanged() {
        let mut s = session();
        // EndUtterance is invalid while Idle.
        let err = s.apply(InterpretCommand::EndUtterance).unwrap_err();
        assert!(matches!(err, InterpretError::InvalidCommand { .. }));
        assert_eq!(s.state(), InterpretState::Idle);
    }

    #[test]
    fn pause_discards_turn_then_resume() {
        let mut s = session();
        run(&mut s, InterpretCommand::BeginTurn(Direction::SelfToPeer));
        run(&mut s, InterpretCommand::EndUtterance);
        let ev = run(&mut s, InterpretCommand::Pause);
        assert_eq!(s.state(), InterpretState::Paused);
        assert!(ev.iter().any(|e| matches!(e, InterpretEvent::Paused)));

        run(&mut s, InterpretCommand::Resume);
        assert_eq!(s.state(), InterpretState::Idle);
        assert!(s.history().is_empty());
    }

    #[test]
    fn fail_then_reset() {
        let mut s = session();
        run(&mut s, InterpretCommand::BeginTurn(Direction::SelfToPeer));
        run(&mut s, InterpretCommand::Fail { reason: "asr crashed".into() });
        assert_eq!(s.state(), InterpretState::Failed);
        run(&mut s, InterpretCommand::Reset);
        assert_eq!(s.state(), InterpretState::Idle);
    }

    #[test]
    fn direction_helpers() {
        let cfg = InterpretConfig::default(); // zh self, en peer, clone on
        assert_eq!(Direction::SelfToPeer.source_language(&cfg), "zh");
        assert_eq!(Direction::SelfToPeer.target_language(&cfg), "en");
        assert!(Direction::SelfToPeer.uses_clone_voice(&cfg));
        assert_eq!(Direction::PeerToSelf.source_language(&cfg), "en");
        assert!(!Direction::PeerToSelf.uses_clone_voice(&cfg));
        assert_eq!(Direction::SelfToPeer.flipped(), Direction::PeerToSelf);
    }
}
