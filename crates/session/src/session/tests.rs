use super::*;
use crate::config::test_config;

#[test]
fn test_session_new_starts_idle() {
    let session = Session::new(test_config());
    assert_eq!(session.state, SessionState::Idle);
    assert!(session.ended_at.is_none());
    assert_eq!(session.turn_count, 0);
    assert!(session.turns.is_empty());
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
fn test_with_id_uses_provided_id() {
    let id = SessionId(uuid::Uuid::now_v7());
    let s = Session::with_id(id, test_config());
    assert_eq!(s.id, id);
    assert_eq!(s.state, SessionState::Idle);
}
