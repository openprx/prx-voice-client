//! In-memory registry of sessions: create, look up, remove and list the active ones.

use std::collections::HashMap;

use crate::config::SessionConfig;
use crate::ids::SessionId;
use crate::session::Session;
use crate::state::SessionState;

/// Manages a collection of active and completed sessions.
///
/// Provides CRUD operations and queries over the set of sessions currently
/// held in memory.
#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
}

impl SessionManager {
    /// Create a new, empty session manager.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Create a new session with the given configuration and return its id.
    pub fn create_session(&mut self, config: SessionConfig) -> SessionId {
        let session = Session::new(config);
        let id = session.id;
        self.sessions.insert(id, session);
        id
    }

    /// Get a shared reference to a session by id.
    pub fn get_session(&self, id: &SessionId) -> Option<&Session> {
        self.sessions.get(id)
    }

    /// Get a mutable reference to a session by id.
    pub fn get_session_mut(&mut self, id: &SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    /// Remove a session by id, returning it if it existed.
    pub fn remove_session(&mut self, id: &SessionId) -> Option<Session> {
        self.sessions.remove(id)
    }

    /// Return references to all sessions that are not in
    /// [`SessionState::Completed`] or [`SessionState::Failed`].
    pub fn active_sessions(&self) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|s| s.state != SessionState::Completed && s.state != SessionState::Failed)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_config;

    #[test]
    fn test_session_manager_create_and_get() {
        let mut mgr = SessionManager::new();
        let id = mgr.create_session(test_config());

        let session = mgr.get_session(&id);
        assert!(session.is_some());
        assert_eq!(session.unwrap().id, id);
        assert_eq!(session.unwrap().state, SessionState::Idle);

        // Mutate through manager
        let session_mut = mgr.get_session_mut(&id).unwrap();
        session_mut.transition(SessionState::Starting).unwrap();
        assert_eq!(mgr.get_session(&id).unwrap().state, SessionState::Starting);

        // Remove
        let removed = mgr.remove_session(&id);
        assert!(removed.is_some());
        assert!(mgr.get_session(&id).is_none());
    }

    #[test]
    fn test_session_manager_active_sessions() {
        let mut mgr = SessionManager::new();

        let id1 = mgr.create_session(test_config());
        let id2 = mgr.create_session(test_config());
        let id3 = mgr.create_session(test_config());

        // All three are Idle (active)
        assert_eq!(mgr.active_sessions().len(), 3);

        // Complete session 1
        {
            let s1 = mgr.get_session_mut(&id1).unwrap();
            s1.transition(SessionState::Starting).unwrap();
            s1.transition(SessionState::Listening).unwrap();
            s1.transition(SessionState::Completed).unwrap();
        }

        // Fail session 2
        {
            let s2 = mgr.get_session_mut(&id2).unwrap();
            s2.transition(SessionState::Starting).unwrap();
            s2.transition(SessionState::Failed).unwrap();
        }

        let active = mgr.active_sessions();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id3);
    }
}
