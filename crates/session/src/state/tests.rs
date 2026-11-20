use super::*;
use crate::config::test_config;
use crate::error::SessionError;
use crate::event::SessionEvent;
use crate::ids::SessionId;
use crate::session::Session;

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
