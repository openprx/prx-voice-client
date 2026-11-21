//! The session record of an assistant session: what the History window shows and the store keeps.
//!
//! The host feeds it the loop's events; it builds a [`Session`] with one [`Turn`](crate::Turn) per
//! user turn. Nothing here decides what is stored: the host asks for the finished session and
//! applies the privacy settings when it persists it.

use crate::config::{InterruptMode, SessionConfig, TranscriptMode};
use crate::error::SessionError;
use crate::ids::TurnId;
use crate::session::Session;
use crate::state::SessionState;
use crate::turn::TurnStatus;

use super::events::AssistantEvent;
use super::fault::AssistantFault;

/// Builds the [`Session`] of one assistant session from the loop's events.
#[derive(Debug)]
pub struct SessionRecorder {
    session: Session,
    /// The turn whose reply is being heard.
    open_turn: Option<TurnId>,
    reply: String,
    failed: bool,
}

impl SessionRecorder {
    /// A recorder for a session in `language` (`"zh-CN"` or `"en"`), listening from now.
    ///
    /// # Errors
    /// Never in practice: the session starts in `Idle` and moves to `Listening`.
    pub fn new(
        language: &str,
        auto_play_reply: bool,
        save_history: bool,
    ) -> Result<Self, SessionError> {
        let mut session = Session::new(SessionConfig {
            input_language: language.to_owned(),
            output_language: language.to_owned(),
            auto_play_reply,
            save_history,
            interrupt_mode: InterruptMode::VoiceActivity,
            transcript_mode: TranscriptMode::Full,
        });
        session.transition(SessionState::Starting)?;
        session.transition(SessionState::Listening)?;
        Ok(Self {
            session,
            open_turn: None,
            reply: String::new(),
            failed: false,
        })
    }

    /// The session so far.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Record an event.
    pub fn on_event(&mut self, event: &AssistantEvent) {
        match event {
            AssistantEvent::UserTurn { text } => {
                self.close_turn(TurnStatus::Interrupted);
                if let Ok(id) = self.session.start_turn() {
                    if let Some(turn) = self.session.turns.iter_mut().find(|t| t.id == id) {
                        turn.user_transcript = Some(text.clone());
                    }
                    self.open_turn = Some(id);
                    self.reply.clear();
                }
            }
            AssistantEvent::ReplyDelta { text } => self.reply.push_str(text),
            AssistantEvent::ReplyDone => self.close_turn(TurnStatus::Completed),
            AssistantEvent::ReplyCut => self.close_turn(TurnStatus::Interrupted),
            AssistantEvent::Fault(fault) => {
                if matches!(
                    fault,
                    AssistantFault::Dialogue(_)
                        | AssistantFault::Synthesis(_)
                        | AssistantFault::NoAudio
                ) {
                    self.close_turn(TurnStatus::Failed);
                }
                if fault.is_fatal() {
                    self.failed = true;
                }
            }
            _ => {}
        }
    }

    fn close_turn(&mut self, status: TurnStatus) {
        let Some(id) = self.open_turn.take() else {
            return;
        };
        let reply = std::mem::take(&mut self.reply);
        if let Some(turn) = self.session.turns.iter_mut().find(|t| t.id == id) {
            if !reply.trim().is_empty() {
                turn.assistant_text = Some(reply);
            }
            turn.ended_at = Some(chrono::Utc::now());
            turn.status = status;
        }
    }

    /// The session ended: close the open turn and give the finished session.
    pub fn finish(mut self) -> Session {
        self.close_turn(TurnStatus::Interrupted);
        let end = if self.failed {
            SessionState::Failed
        } else {
            SessionState::Completed
        };
        if let Err(e) = self.session.transition(end) {
            tracing::warn!("the assistant session record could not be closed: {e}");
        }
        self.session
    }
}

#[cfg(test)]
mod tests {
    use prxvc_ports::{DialogueError, EndpointError};

    use super::*;

    fn recorder() -> SessionRecorder {
        SessionRecorder::new("en", true, true).expect("starts")
    }

    fn text(s: &str) -> String {
        s.to_owned()
    }

    #[test]
    fn a_completed_turn_keeps_both_sides_and_the_session_completes() {
        let mut r = recorder();
        r.on_event(&AssistantEvent::UserTurn {
            text: text("what time is it"),
        });
        r.on_event(&AssistantEvent::ReplyDelta {
            text: text("It is "),
        });
        r.on_event(&AssistantEvent::ReplyDelta {
            text: text("noon."),
        });
        r.on_event(&AssistantEvent::ReplyDone);
        let session = r.finish();
        assert_eq!(session.state, SessionState::Completed);
        assert_eq!(session.turns.len(), 1);
        let turn = &session.turns[0];
        assert_eq!(turn.user_transcript.as_deref(), Some("what time is it"));
        assert_eq!(turn.assistant_text.as_deref(), Some("It is noon."));
        assert_eq!(turn.status, TurnStatus::Completed);
        assert!(turn.ended_at.is_some());
    }

    #[test]
    fn a_cut_reply_keeps_the_part_that_was_said_and_a_failed_one_is_marked() {
        let mut r = recorder();
        r.on_event(&AssistantEvent::UserTurn {
            text: text("tell me a story"),
        });
        r.on_event(&AssistantEvent::ReplyDelta {
            text: text("Once upon"),
        });
        r.on_event(&AssistantEvent::ReplyCut);
        r.on_event(&AssistantEvent::UserTurn {
            text: text("never mind"),
        });
        r.on_event(&AssistantEvent::Fault(AssistantFault::Dialogue(
            DialogueError::Unavailable {
                reason: text("down"),
            },
        )));
        let session = r.finish();
        assert_eq!(session.turns[0].status, TurnStatus::Interrupted);
        assert_eq!(
            session.turns[0].assistant_text.as_deref(),
            Some("Once upon")
        );
        assert_eq!(session.turns[1].status, TurnStatus::Failed);
        assert!(session.turns[1].assistant_text.is_none());
        assert_eq!(
            session.state,
            SessionState::Completed,
            "a failed turn does not fail the session"
        );
    }

    #[test]
    fn a_fatal_fault_fails_the_session_and_stopping_closes_an_open_turn() {
        let mut r = recorder();
        r.on_event(&AssistantEvent::UserTurn {
            text: text("hello"),
        });
        r.on_event(&AssistantEvent::Fault(AssistantFault::Microphone(
            EndpointError::DeviceLost {
                device: text("usb"),
            },
        )));
        let session = r.finish();
        assert_eq!(session.state, SessionState::Failed);
        assert_eq!(session.turns[0].status, TurnStatus::Interrupted);

        let mut r = recorder();
        r.on_event(&AssistantEvent::UserTurn {
            text: text("hello"),
        });
        let session = r.finish();
        assert_eq!(
            session.turns[0].status,
            TurnStatus::Interrupted,
            "stopped while the reply was pending"
        );
        assert_eq!(session.state, SessionState::Completed);
        assert!(session.ended_at.is_some());
    }

    #[test]
    fn a_session_without_turns_is_an_empty_completed_session() {
        let session = recorder().finish();
        assert!(session.turns.is_empty());
        assert_eq!(session.state, SessionState::Completed);
        assert_eq!(session.config.input_language, "en");
    }
}
