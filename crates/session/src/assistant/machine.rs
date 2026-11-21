//! The state machine of the assistant's voice loop: idle, listening, thinking, speaking.
//!
//! Pure (CS-2.3): no clock, no threads, no I/O. [`AssistantState::on`] takes an event and returns
//! the next state with the actions the loop executes; an event that is illegal in the current
//! state is an [`InvalidTransition`] that leaves the state as it is.
//!
//! ```text
//!            Start                 Turn(text)                ReplyAudio
//!   Idle ───────────▶ Listening ────────────────▶ Thinking ─────────────▶ Speaking
//!     ▲                  ▲   ▲   SpeechStarted        │                        │
//!     │ Stop / Fault     │   └──────────────────────┘ (cancels the reply,     │
//!     └── (from any)     │        the text is carried into the next turn)     │
//!                        └───────────────────────────────────────────────────┘
//!                          ReplyDone, Interrupt, SpeechStarted (barge-in), ReplyFailed
//! ```
//!
//! * Speech while the assistant is **thinking** cancels the pending reply and keeps the user's
//!   words: they are joined with what is said next, so a sentence split by a pause is one turn.
//! * Speech while the assistant is **speaking** cuts the reply (barge-in); an interrupt command
//!   (a click on the orb) does the same.
//! * A **stop** ends the session from any state; a fatal fault does too.

use super::fault::AssistantFault;
use crate::rejection::InvalidTransition;

/// Where the loop is.
#[derive(Clone, Debug, PartialEq)]
pub enum AssistantState {
    /// No session: the microphone is not open.
    Idle,
    /// The microphone is open and the assistant waits for the user to speak.
    Listening {
        /// The user is speaking now.
        speaking: bool,
        /// The words of a turn whose reply was cancelled because the user went on speaking; they
        /// are joined with the next turn.
        carried: Option<String>,
    },
    /// The user's turn went to the dialogue backend and no audio of the reply plays yet.
    Thinking {
        /// The turn's text.
        text: String,
    },
    /// The reply plays.
    Speaking {
        /// The turn's text.
        text: String,
    },
}

/// What happened.
#[derive(Clone, Debug, PartialEq)]
pub enum AssistantInput {
    /// The microphone is open.
    Start,
    /// The user began to speak.
    SpeechStarted,
    /// The user's speech ended with these words.
    Turn(String),
    /// The user's speech ended with nothing but filler: no turn.
    NoTurn,
    /// The first audio of the reply reached the output.
    ReplyAudio,
    /// The whole reply was heard (the output drained).
    ReplyDone,
    /// The reply failed.
    ReplyFailed(AssistantFault),
    /// Cut what the assistant says now (an interrupt command).
    Interrupt,
    /// End the session.
    Stop,
    /// The session cannot go on.
    Fatal(AssistantFault),
}

/// What the loop does as a result.
#[derive(Clone, Debug, PartialEq)]
pub enum AssistantAction {
    /// Send this text to the dialogue backend as a turn.
    StartReply(String),
    /// Cancel the reply in flight (dialogue and synthesis requests).
    CancelReply,
    /// Silence the output at once.
    FlushOutput,
    /// Tell the user why something did not work.
    ShowFault(AssistantFault),
    /// The conversation is over: release the microphone and let the backend forget it.
    EndSession,
}

use AssistantAction as A;
use AssistantInput as I;
use AssistantState as S;

fn listening(speaking: bool, carried: Option<String>) -> S {
    S::Listening { speaking, carried }
}

/// `carried` and `text` as one turn.
fn joined(carried: Option<String>, text: String) -> String {
    match carried {
        Some(before) if !before.trim().is_empty() => {
            let before = before.trim_end();
            // Chinese text is not separated by spaces.
            let cjk = before.chars().last().is_some_and(is_cjk)
                && text.chars().next().is_some_and(is_cjk);
            if cjk {
                format!("{before}{text}")
            } else {
                format!("{before} {text}")
            }
        }
        _ => text,
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FFEF}')
}

type Rejection = InvalidTransition<AssistantState, AssistantInput>;

impl AssistantState {
    /// The state before a session.
    pub fn idle() -> Self {
        Self::Idle
    }

    /// Whether the loop is in the state named `Idle`.
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Apply `event`.
    ///
    /// # Errors
    /// The event is not legal in this state; the state is handed back in the error.
    pub fn on(
        self,
        event: AssistantInput,
    ) -> Result<(AssistantState, Vec<AssistantAction>), Rejection> {
        match (self, event) {
            (S::Idle, I::Start) => Ok((listening(false, None), vec![])),
            (S::Idle, I::Stop) => Ok((S::Idle, vec![])),

            // Listening.
            (S::Listening { carried, .. }, I::SpeechStarted) => {
                Ok((listening(true, carried), vec![]))
            }
            (S::Listening { carried, .. }, I::Turn(text)) => {
                let text = joined(carried, text);
                Ok((
                    S::Thinking { text: text.clone() },
                    vec![A::StartReply(text)],
                ))
            }
            (S::Listening { carried, .. }, I::NoTurn) => Ok((listening(false, carried), vec![])),
            (s @ S::Listening { .. }, I::Interrupt) => Ok((s, vec![])),
            (S::Listening { .. }, I::Stop) => Ok((S::Idle, vec![A::EndSession])),

            // Thinking.
            (S::Thinking { text }, I::ReplyAudio) => Ok((S::Speaking { text }, vec![])),
            (S::Thinking { text }, I::SpeechStarted) => {
                Ok((listening(true, Some(text)), vec![A::CancelReply]))
            }
            (S::Thinking { .. }, I::ReplyFailed(f)) => Ok((
                listening(false, None),
                vec![A::CancelReply, A::ShowFault(f)],
            )),
            (S::Thinking { .. }, I::Interrupt) => {
                Ok((listening(false, None), vec![A::CancelReply]))
            }
            (S::Thinking { .. }, I::Stop) => Ok((S::Idle, vec![A::CancelReply, A::EndSession])),

            // Speaking.
            (S::Speaking { .. }, I::SpeechStarted) => {
                Ok((listening(true, None), vec![A::CancelReply, A::FlushOutput]))
            }
            (S::Speaking { .. }, I::Interrupt) => {
                Ok((listening(false, None), vec![A::CancelReply, A::FlushOutput]))
            }
            (S::Speaking { .. }, I::ReplyDone) => Ok((listening(false, None), vec![])),
            (S::Speaking { .. }, I::ReplyFailed(f)) => {
                // What was already produced plays out; nothing more is made.
                Ok((
                    listening(false, None),
                    vec![A::CancelReply, A::ShowFault(f)],
                ))
            }
            (S::Speaking { .. }, I::Stop) => {
                Ok((S::Idle, vec![A::CancelReply, A::FlushOutput, A::EndSession]))
            }

            // A thinking turn whose reply ended with no audio at all is a failure of the reply,
            // never a silent turn: the loop reports `ReplyFailed(NoAudio)` for it.

            // A fatal fault ends the session from anywhere it is running.
            (S::Idle, I::Fatal(f)) => Ok((S::Idle, vec![A::ShowFault(f)])),
            (S::Listening { .. }, I::Fatal(f)) => {
                Ok((S::Idle, vec![A::ShowFault(f), A::EndSession]))
            }
            (S::Thinking { .. }, I::Fatal(f)) => Ok((
                S::Idle,
                vec![A::CancelReply, A::ShowFault(f), A::EndSession],
            )),
            (S::Speaking { .. }, I::Fatal(f)) => Ok((
                S::Idle,
                vec![
                    A::CancelReply,
                    A::FlushOutput,
                    A::ShowFault(f),
                    A::EndSession,
                ],
            )),

            (from, event) => Err(InvalidTransition::new(from, event)),
        }
    }
}

#[cfg(test)]
mod tests {
    use prxvc_ports::{DialogueError, EndpointError};

    use super::*;

    fn fault() -> AssistantFault {
        AssistantFault::Dialogue(DialogueError::Unavailable {
            reason: "down".into(),
        })
    }

    fn l(speaking: bool) -> S {
        listening(speaking, None)
    }

    fn thinking() -> S {
        S::Thinking { text: "hi".into() }
    }

    fn speaking() -> S {
        S::Speaking { text: "hi".into() }
    }

    fn step(s: S, e: I) -> (S, Vec<A>) {
        s.on(e).expect("the transition is legal")
    }

    #[test]
    fn a_session_starts_listening_and_stop_of_an_idle_loop_is_harmless() {
        assert_eq!(step(S::Idle, I::Start), (l(false), vec![]));
        assert_eq!(step(S::Idle, I::Stop), (S::Idle, vec![]));
    }

    #[test]
    fn a_turn_goes_to_the_dialogue_and_the_reply_audio_makes_the_assistant_speak() {
        let (s, a) = step(l(true), I::Turn("hello there".into()));
        assert_eq!(
            s,
            S::Thinking {
                text: "hello there".into()
            }
        );
        assert_eq!(a, vec![A::StartReply("hello there".into())]);
        let (s, a) = step(s, I::ReplyAudio);
        assert_eq!(
            s,
            S::Speaking {
                text: "hello there".into()
            }
        );
        assert!(a.is_empty());
        let (s, a) = step(s, I::ReplyDone);
        assert_eq!(s, l(false));
        assert!(a.is_empty());
    }

    #[test]
    fn speech_marks_listening_and_filler_makes_no_turn() {
        let (s, _) = step(l(false), I::SpeechStarted);
        assert_eq!(s, l(true));
        let (s, a) = step(s, I::NoTurn);
        assert_eq!(s, l(false));
        assert!(a.is_empty(), "filler creates no turn and no request");
    }

    #[test]
    fn speech_while_thinking_cancels_the_reply_and_carries_the_words_into_the_next_turn() {
        let (s, a) = step(thinking(), I::SpeechStarted);
        assert_eq!(s, listening(true, Some("hi".into())));
        assert_eq!(a, vec![A::CancelReply]);
        let (s, a) = step(s, I::Turn("how are you".into()));
        assert_eq!(
            s,
            S::Thinking {
                text: "hi how are you".into()
            }
        );
        assert_eq!(a, vec![A::StartReply("hi how are you".into())]);
    }

    #[test]
    fn carried_chinese_text_is_joined_without_a_space() {
        let s = listening(true, Some("今天天气".into()));
        let (s, _) = step(s, I::Turn("怎么样".into()));
        assert_eq!(
            s,
            S::Thinking {
                text: "今天天气怎么样".into()
            }
        );
    }

    #[test]
    fn a_carried_turn_survives_filler() {
        let s = listening(true, Some("hi".into()));
        let (s, _) = step(s, I::NoTurn);
        assert_eq!(s, listening(false, Some("hi".into())));
    }

    #[test]
    fn speech_while_speaking_cuts_the_reply_and_the_output() {
        let (s, a) = step(speaking(), I::SpeechStarted);
        assert_eq!(s, l(true));
        assert_eq!(a, vec![A::CancelReply, A::FlushOutput]);
    }

    #[test]
    fn an_interrupt_cuts_what_is_said_and_does_not_end_the_session() {
        let (s, a) = step(speaking(), I::Interrupt);
        assert_eq!(s, l(false));
        assert_eq!(a, vec![A::CancelReply, A::FlushOutput]);
        let (s, a) = step(thinking(), I::Interrupt);
        assert_eq!(s, l(false));
        assert_eq!(a, vec![A::CancelReply]);
        let (s, a) = step(l(false), I::Interrupt);
        assert_eq!(s, l(false));
        assert!(a.is_empty());
    }

    #[test]
    fn stop_ends_the_session_from_every_running_state_and_cancels_what_runs() {
        assert_eq!(step(l(false), I::Stop), (S::Idle, vec![A::EndSession]));
        assert_eq!(step(l(true), I::Stop), (S::Idle, vec![A::EndSession]));
        assert_eq!(
            step(thinking(), I::Stop),
            (S::Idle, vec![A::CancelReply, A::EndSession])
        );
        assert_eq!(
            step(speaking(), I::Stop),
            (S::Idle, vec![A::CancelReply, A::FlushOutput, A::EndSession])
        );
    }

    #[test]
    fn a_failed_reply_is_shown_and_the_assistant_listens_again() {
        let (s, a) = step(thinking(), I::ReplyFailed(fault()));
        assert_eq!(s, l(false));
        assert_eq!(a, vec![A::CancelReply, A::ShowFault(fault())]);
        let (s, a) = step(speaking(), I::ReplyFailed(AssistantFault::NoAudio));
        assert_eq!(s, l(false));
        assert_eq!(
            a,
            vec![A::CancelReply, A::ShowFault(AssistantFault::NoAudio)],
            "what was produced plays out: no flush"
        );
    }

    #[test]
    fn a_fatal_fault_ends_the_session_and_says_why() {
        let mic = AssistantFault::Microphone(EndpointError::DeviceLost {
            device: "usb".into(),
        });
        assert!(mic.is_fatal());
        assert!(!fault().is_fatal());
        for (from, extra) in [
            (l(false), vec![]),
            (thinking(), vec![A::CancelReply]),
            (speaking(), vec![A::CancelReply, A::FlushOutput]),
        ] {
            let (s, a) = step(from, I::Fatal(mic.clone()));
            assert_eq!(s, S::Idle);
            let mut expected = extra;
            expected.push(A::ShowFault(mic.clone()));
            expected.push(A::EndSession);
            assert_eq!(a, expected);
        }
        let (s, a) = step(S::Idle, I::Fatal(mic.clone()));
        assert_eq!(s, S::Idle);
        assert_eq!(a, vec![A::ShowFault(mic)]);
    }

    #[test]
    fn every_illegal_event_is_rejected_and_keeps_the_state() {
        let illegal = [
            (S::Idle, I::SpeechStarted),
            (S::Idle, I::Turn("x".into())),
            (S::Idle, I::NoTurn),
            (S::Idle, I::ReplyAudio),
            (S::Idle, I::ReplyDone),
            (S::Idle, I::ReplyFailed(fault())),
            (S::Idle, I::Interrupt),
            (l(false), I::Start),
            (l(false), I::ReplyAudio),
            (l(false), I::ReplyDone),
            (l(false), I::ReplyFailed(fault())),
            (thinking(), I::Start),
            (thinking(), I::Turn("x".into())),
            (thinking(), I::NoTurn),
            (thinking(), I::ReplyDone),
            (speaking(), I::Start),
            (speaking(), I::Turn("x".into())),
            (speaking(), I::NoTurn),
            (speaking(), I::ReplyAudio),
        ];
        for (state, event) in illegal {
            let err = state
                .clone()
                .on(event.clone())
                .expect_err("must be rejected");
            assert_eq!(err.from, state, "{event:?}");
            assert_eq!(err.event, event);
        }
    }
}
