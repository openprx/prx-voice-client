//! Central engine coordinating sessions, transport, and events.
//!
//! The [`Engine`] is the primary orchestrator: it owns a
//! [`SessionManager`], a [`Transport`] implementation, and an
//! [`EventBroadcaster`]. Incoming [`Command`]s drive the session state
//! machine and are forwarded to the transport; resulting protocol
//! [`Event`]s are broadcast to all UI subscribers.

use prxvc_protocol::transport::{Transport, TransportError, TransportState};
use prxvc_protocol::{Command, Event};
use prxvc_session::{
    Session, SessionConfig, SessionEvent, SessionId, SessionManager, SessionState,
};

use crate::PrxvcError;
use crate::bus::EventBroadcaster;

// ---------------------------------------------------------------------------
// Error conversion
// ---------------------------------------------------------------------------

impl From<TransportError> for PrxvcError {
    fn from(err: TransportError) -> Self {
        PrxvcError::Protocol(err.to_string())
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Central engine coordinating sessions, transport, and events.
///
/// The engine processes [`Command`]s, manages the session state machine via
/// [`SessionManager`], forwards commands to a [`Transport`], and broadcasts
/// resulting [`Event`]s through an [`EventBroadcaster`].
pub struct Engine {
    session_manager: SessionManager,
    transport: Box<dyn Transport>,
    event_tx: EventBroadcaster,
    /// The currently active session, if any.
    active_session: Option<SessionId>,
    /// Whether auto-reconnect is enabled.
    auto_reconnect: bool,
    /// Maximum reconnect attempts before giving up.
    max_reconnect_attempts: u32,
    /// Current reconnect attempt counter.
    reconnect_attempts: u32,
    /// Base delay for exponential backoff (ms).
    reconnect_delay_ms: u64,
}

impl Engine {
    /// Create a new engine with the given transport and event broadcaster.
    pub fn new(transport: Box<dyn Transport>, event_tx: EventBroadcaster) -> Self {
        Self {
            session_manager: SessionManager::new(),
            transport,
            event_tx,
            active_session: None,
            auto_reconnect: true,
            max_reconnect_attempts: 5,
            reconnect_attempts: 0,
            reconnect_delay_ms: 1000,
        }
    }

    /// Initialize the engine by connecting the transport.
    ///
    /// Connects to the backend and polls for the initial [`Event::AppReady`]
    /// event, broadcasting it to all subscribers.
    pub fn initialize(&mut self) -> Result<(), PrxvcError> {
        self.transport.connect()?;

        // Poll for AppReady event produced by connect().
        if let Some(event) = self.transport.poll_event()? {
            let _ = self.event_tx.broadcast(event);
        }

        Ok(())
    }

    /// Shut down the engine gracefully.
    ///
    /// If an active session exists it is transitioned to [`SessionState::Completed`].
    /// The transport is then disconnected.
    pub fn shutdown(&mut self) -> Result<(), PrxvcError> {
        if let Some(session_id) = self.active_session.take() {
            // Best-effort: transition to Completed. If it fails (e.g. already
            // completed), we log and continue.
            if let Err(e) = self.try_transition(&session_id, SessionState::Completed) {
                tracing::warn!("shutdown: could not complete active session: {e}");
            }
        }

        self.transport.disconnect()?;
        Ok(())
    }

    /// Process a single command, updating sessions and transport state.
    ///
    /// On failure an [`Event::ErrorOccurred`] is broadcast before the error
    /// is returned, so that UI subscribers are notified immediately.
    pub fn process_command(&mut self, command: Command) -> Result<(), PrxvcError> {
        tracing::info!(?command, "Engine::process_command");
        let result = match command {
            Command::CreateSession => self.handle_create_session(),
            Command::StartSession { session_id } => self.handle_start_session(session_id),
            Command::StopSession { session_id } => self.handle_stop_session(session_id),
            Command::InterruptPlayback { session_id } => self.handle_interrupt_playback(session_id),
            Command::RefreshDeviceList => self.handle_refresh_device_list(),
            Command::ExportDiagnostics => self.handle_export_diagnostics(),
            other => {
                tracing::debug!(?other, "forwarding unhandled command to transport");
                self.transport.send_command(&other)?;
                // Drain any resulting events.
                self.drain_and_broadcast()?;
                Ok(())
            }
        };

        if let Err(ref e) = result {
            self.broadcast_error(
                "session.command_failed",
                &format!("Command failed: {e}"),
                true,
            );
        }

        result
    }

    /// Poll the transport for pending events and process them.
    ///
    /// Checks the transport state first: if the transport is disconnected or
    /// failed and an active session exists, auto-reconnection is attempted
    /// (when enabled). If the transport is in the middle of reconnecting,
    /// polling is skipped.
    ///
    /// Transcript and playback events drive the session state machine for
    /// the active session. All events are broadcast to subscribers.
    ///
    /// Returns the list of events that were broadcast.
    pub fn poll_transport_events(&mut self) -> Result<Vec<Event>, PrxvcError> {
        // Check transport state for failure/disconnection.
        match self.transport.state() {
            TransportState::Disconnected | TransportState::Failed => {
                // Persistent connection should always be up — reconnect
                // regardless of whether an active session exists.
                if self.auto_reconnect {
                    self.attempt_reconnect()?;
                }
                return Ok(Vec::new());
            }
            TransportState::Reconnecting => {
                return Ok(Vec::new());
            }
            _ => {}
        }

        let mut broadcast_events = Vec::new();

        while let Some(event) = self.transport.poll_event()? {
            // Extract the event kind and associated session UUID before consuming.
            let action = classify_event(&event);

            let _ = self.event_tx.broadcast(event.clone());
            broadcast_events.push(event);

            // Drive session state machine based on the event kind.
            if let Some((transitions, event_session_id)) = action {
                if let Some(active_id) = self.active_session {
                    let sid = SessionId(event_session_id);
                    if sid == active_id {
                        self.try_transition_sequence(
                            &active_id,
                            transitions,
                            &mut broadcast_events,
                        );
                    }
                }
            }
        }

        // Successful poll — reset reconnect counter.
        self.reset_reconnect_counter();

        Ok(broadcast_events)
    }

    /// Return a reference to the active session, if any.
    pub fn active_session(&self) -> Option<&Session> {
        self.active_session
            .as_ref()
            .and_then(|id| self.session_manager.get_session(id))
    }

    /// Return a reference to the underlying session manager.
    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }

    /// Enable or disable auto-reconnection.
    pub fn set_auto_reconnect(&mut self, enabled: bool) {
        self.auto_reconnect = enabled;
    }

    /// Attempt to reconnect the transport.
    ///
    /// If the maximum number of reconnection attempts has been reached, the
    /// active session is transitioned to [`SessionState::Failed`] and a
    /// non-recoverable error event is broadcast.
    ///
    /// On success the session is transitioned back to
    /// [`SessionState::Listening`] and an [`Event::AppReady`] is broadcast.
    pub fn attempt_reconnect(&mut self) -> Result<(), PrxvcError> {
        if self.reconnect_attempts >= self.max_reconnect_attempts {
            // Transition active session to Failed.
            if let Some(session_id) = self.active_session_id() {
                let _ = self.try_transition(&session_id, SessionState::Failed);
            }
            self.broadcast_error(
                "max_reconnect_attempts_exceeded",
                "Connection lost permanently",
                false,
            );
            return Err(PrxvcError::Protocol(
                "max reconnect attempts exceeded".into(),
            ));
        }

        self.reconnect_attempts += 1;
        let delay = self.reconnect_delay_ms * u64::from(self.reconnect_attempts);
        tracing::info!(
            "reconnect attempt {}/{} (backoff {}ms)",
            self.reconnect_attempts,
            self.max_reconnect_attempts,
            delay
        );

        // Transition active session to Recovering.
        if let Some(session_id) = self.active_session_id() {
            let _ = self.try_transition(&session_id, SessionState::Recovering);
        }

        // Broadcast reconnecting event.
        self.broadcast_error(
            "network.reconnecting",
            &format!("Reconnecting... attempt {}", self.reconnect_attempts),
            true,
        );

        // Try to reconnect transport.
        let _ = self.transport.disconnect();
        match self.transport.connect() {
            Ok(()) => {
                self.reconnect_attempts = 0;
                tracing::info!("reconnected successfully");

                // Transition session back to Listening.
                if let Some(session_id) = self.active_session_id() {
                    let _ = self.try_transition(&session_id, SessionState::Listening);
                }

                let _ = self.event_tx.broadcast(Event::AppReady);
                Ok(())
            }
            Err(e) => {
                tracing::warn!("reconnect failed: {e}");
                Err(PrxvcError::Protocol(format!("reconnect failed: {e}")))
            }
        }
    }

    /// Reset the reconnection counter.
    ///
    /// Call this after successful communication to indicate the connection
    /// is healthy.
    pub fn reset_reconnect_counter(&mut self) {
        self.reconnect_attempts = 0;
    }

    /// Check if the active session has been idle too long.
    ///
    /// If the session's last update timestamp is older than `timeout_ms`
    /// milliseconds, the session is transitioned to
    /// [`SessionState::Recovering`] and a timeout error is broadcast.
    ///
    /// Returns `true` if the session timed out, `false` otherwise.
    pub fn check_session_timeout(&mut self, timeout_ms: u64) -> bool {
        let updated_at = match self
            .active_session
            .as_ref()
            .and_then(|id| self.session_manager.get_session(id))
        {
            Some(session) => session.updated_at,
            None => return false,
        };

        let elapsed = chrono::Utc::now()
            .signed_duration_since(updated_at)
            .num_milliseconds();

        if elapsed as u64 > timeout_ms {
            tracing::warn!("session timeout after {elapsed}ms");
            if let Some(sid) = self.active_session_id() {
                let _ = self.try_transition(&sid, SessionState::Recovering);
                self.broadcast_error("session.timeout", "Session timed out", true);
            }
            return true;
        }

        false
    }

    /// Get the active session id, if one exists.
    fn active_session_id(&self) -> Option<SessionId> {
        self.active_session
    }

    // -----------------------------------------------------------------------
    // Command handlers
    // -----------------------------------------------------------------------

    fn handle_create_session(&mut self) -> Result<(), PrxvcError> {
        let config = SessionConfig {
            input_language: "en".to_owned(),
            output_language: "en".to_owned(),
            auto_play_reply: true,
            save_history: false,
            interrupt_mode: prxvc_session::InterruptMode::VoiceActivity,
            transcript_mode: prxvc_session::TranscriptMode::Full,
        };

        let session_id = self.session_manager.create_session(config);

        // Forward to transport.
        self.transport.send_command(&Command::CreateSession)?;

        // Poll for SessionCreated from transport.
        if let Some(event) = self.transport.poll_event()? {
            let _ = self.event_tx.broadcast(event);
        }

        // Also broadcast our own SessionCreated with the local id.
        let _ = self.event_tx.broadcast(Event::SessionCreated {
            session_id: session_id.0,
        });

        self.active_session = Some(session_id);
        Ok(())
    }

    fn handle_start_session(&mut self, session_uuid: uuid::Uuid) -> Result<(), PrxvcError> {
        let session_id = SessionId(session_uuid);

        // Transition: Idle -> Starting -> Listening
        self.try_transition(&session_id, SessionState::Starting)?;
        self.try_transition(&session_id, SessionState::Listening)?;

        // Forward to transport.
        self.transport.send_command(&Command::StartSession {
            session_id: session_uuid,
        })?;

        // Drain all transport response events (StartSession may produce multiple).
        self.drain_and_broadcast()?;

        // Broadcast our own SessionStarted.
        let _ = self.event_tx.broadcast(Event::SessionStarted {
            session_id: session_uuid,
        });

        Ok(())
    }

    fn handle_stop_session(&mut self, session_uuid: uuid::Uuid) -> Result<(), PrxvcError> {
        let session_id = SessionId(session_uuid);

        self.try_transition(&session_id, SessionState::Completed)?;

        self.transport.send_command(&Command::StopSession {
            session_id: session_uuid,
        })?;

        // Drain transport events.
        self.drain_and_broadcast()?;

        if self.active_session == Some(session_id) {
            self.active_session = None;
        }

        Ok(())
    }

    fn handle_interrupt_playback(&mut self, session_uuid: uuid::Uuid) -> Result<(), PrxvcError> {
        let session_id = SessionId(session_uuid);

        // Only interrupt if currently in AssistantSpeaking.
        let current_state = self
            .session_manager
            .get_session(&session_id)
            .map(|s| s.state);

        if current_state != Some(SessionState::AssistantSpeaking) {
            tracing::warn!(
                "interrupt requested but session not in AssistantSpeaking (state: {:?})",
                current_state
            );
            return Ok(());
        }

        // Transition: AssistantSpeaking -> Interrupted -> Listening
        self.try_transition(&session_id, SessionState::Interrupted)?;
        self.try_transition(&session_id, SessionState::Listening)?;

        self.transport.send_command(&Command::InterruptPlayback {
            session_id: session_uuid,
        })?;

        // Broadcast PlaybackInterrupted.
        let _ = self.event_tx.broadcast(Event::PlaybackInterrupted {
            session_id: session_uuid,
        });

        Ok(())
    }

    fn handle_refresh_device_list(&mut self) -> Result<(), PrxvcError> {
        self.transport.send_command(&Command::RefreshDeviceList)?;

        if let Some(event) = self.transport.poll_event()? {
            let _ = self.event_tx.broadcast(event);
        }

        Ok(())
    }

    fn handle_export_diagnostics(&mut self) -> Result<(), PrxvcError> {
        self.transport.send_command(&Command::ExportDiagnostics)?;

        if let Some(event) = self.transport.poll_event()? {
            let _ = self.event_tx.broadcast(event);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Attempt to transition a session to the target state.
    ///
    /// On success the resulting [`SessionEvent`] is converted to a protocol
    /// [`Event`] and broadcast. On failure a warning is logged and a
    /// [`PrxvcError::Session`] is returned.
    fn try_transition(
        &mut self,
        session_id: &SessionId,
        target: SessionState,
    ) -> Result<(), PrxvcError> {
        if let Some(session) = self.session_manager.get_session_mut(session_id) {
            match session.transition(target) {
                Ok(session_event) => {
                    let proto_event = Self::session_event_to_protocol(session_id, &session_event);
                    let _ = self.event_tx.broadcast(proto_event);
                    Ok(())
                }
                Err(e) => {
                    tracing::warn!("transition failed: {e}");
                    Err(PrxvcError::Session(e.to_string()))
                }
            }
        } else {
            Err(PrxvcError::Session("session not found".into()))
        }
    }

    /// Attempt a sequence of transitions, collecting broadcast events.
    ///
    /// Stops at the first failed transition (logging a warning) rather than
    /// propagating the error, because partial progress through the sequence
    /// is acceptable for transcript-driven state advancement.
    fn try_transition_sequence(
        &mut self,
        session_id: &SessionId,
        targets: &[SessionState],
        broadcast_events: &mut Vec<Event>,
    ) {
        for &target in targets {
            if let Some(session) = self.session_manager.get_session_mut(session_id) {
                match session.transition(target) {
                    Ok(session_event) => {
                        let proto_event =
                            Self::session_event_to_protocol(session_id, &session_event);
                        let _ = self.event_tx.broadcast(proto_event.clone());
                        broadcast_events.push(proto_event);
                    }
                    Err(e) => {
                        tracing::warn!("transition sequence to {target:?} failed: {e}");
                        break;
                    }
                }
            } else {
                tracing::warn!(
                    "session {:?} not found during transition sequence",
                    session_id
                );
                break;
            }
        }
    }

    /// Convert a [`SessionEvent`] to a protocol [`Event`].
    fn session_event_to_protocol(session_id: &SessionId, event: &SessionEvent) -> Event {
        match event {
            SessionEvent::Created(id) => Event::SessionCreated { session_id: id.0 },
            SessionEvent::StateChanged { from, to, .. } => Event::SessionStateChanged {
                session_id: session_id.0,
                from: from.to_string(),
                to: to.to_string(),
            },
            SessionEvent::Completed(id) => Event::SessionStateChanged {
                session_id: id.0,
                from: "Active".to_owned(),
                to: "Completed".to_owned(),
            },
            SessionEvent::Failed { id, reason } => Event::ErrorOccurred {
                code: format!("session.failed.{}", id.0),
                message: reason.clone(),
                recoverable: false,
            },
        }
    }

    /// Broadcast an error event to all subscribers.
    fn broadcast_error(&self, code: &str, message: &str, recoverable: bool) {
        let _ = self.event_tx.broadcast(Event::ErrorOccurred {
            code: code.into(),
            message: message.into(),
            recoverable,
        });
    }

    /// Drain all pending transport events and broadcast them.
    fn drain_and_broadcast(&mut self) -> Result<(), PrxvcError> {
        while let Some(event) = self.transport.poll_event()? {
            let _ = self.event_tx.broadcast(event);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Classify a transport event into optional state transitions to apply.
///
/// Returns `None` for events that do not drive the session state machine,
/// or `Some((transitions, session_uuid))` for events that do.
fn classify_event(event: &Event) -> Option<(&'static [SessionState], uuid::Uuid)> {
    static FINAL_TRANSCRIPT: &[SessionState] = &[
        SessionState::UserSpeaking,
        SessionState::UserSpeechEnded,
        SessionState::Recognizing,
    ];
    static REPLY_STARTED: &[SessionState] =
        &[SessionState::Thinking, SessionState::AssistantSpeaking];
    static PLAYBACK_DONE: &[SessionState] = &[SessionState::Listening];

    match event {
        Event::FinalTranscriptProduced { session_id, .. } => Some((FINAL_TRANSCRIPT, *session_id)),
        Event::AssistantReplyStarted { session_id } => Some((REPLY_STARTED, *session_id)),
        Event::PlaybackCompleted { session_id } => Some((PLAYBACK_DONE, *session_id)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::event_channel;
    use prxvc_protocol::mock::MockGateway;
    fn make_engine() -> (Engine, crate::bus::EventSubscriber) {
        let (event_tx, event_rx) = event_channel(256);
        let transport = Box::new(MockGateway::new());
        let engine = Engine::new(transport, event_tx);
        (engine, event_rx)
    }

    #[test]
    fn test_engine_initialize_and_shutdown() {
        let (mut engine, _rx) = make_engine();

        engine.initialize().expect("initialize should succeed");
        // After initialize, transport should be connected.
        assert!(engine.active_session().is_none());

        engine.shutdown().expect("shutdown should succeed");
    }

    #[test]
    fn test_engine_create_session() {
        let (mut engine, _rx) = make_engine();
        engine.initialize().expect("init failed");

        engine
            .process_command(Command::CreateSession)
            .expect("create session failed");

        // Should have an active session now.
        assert!(engine.active_session.is_some());
        let session = engine.active_session().expect("active session missing");
        assert_eq!(session.state, SessionState::Idle);
    }

    #[test]
    fn test_engine_start_session() {
        let (mut engine, _rx) = make_engine();
        engine.initialize().expect("init failed");

        engine
            .process_command(Command::CreateSession)
            .expect("create session failed");

        let session_uuid = engine.active_session().expect("no active session").id.0;

        engine
            .process_command(Command::StartSession {
                session_id: session_uuid,
            })
            .expect("start session failed");

        let session = engine.active_session().expect("no active session");
        assert_eq!(session.state, SessionState::Listening);
    }

    #[test]
    fn test_engine_stop_session() {
        let (mut engine, _rx) = make_engine();
        engine.initialize().expect("init failed");

        engine
            .process_command(Command::CreateSession)
            .expect("create failed");

        let session_uuid = engine.active_session().expect("no active session").id.0;

        engine
            .process_command(Command::StartSession {
                session_id: session_uuid,
            })
            .expect("start failed");

        engine
            .process_command(Command::StopSession {
                session_id: session_uuid,
            })
            .expect("stop failed");

        // Active session should be cleared.
        assert!(engine.active_session.is_none());

        // Session should be Completed in the manager.
        let session_id = SessionId(session_uuid);
        let session = engine
            .session_manager()
            .get_session(&session_id)
            .expect("session missing from manager");
        assert_eq!(session.state, SessionState::Completed);
    }

    #[test]
    fn test_engine_interrupt_during_speaking() {
        let (mut engine, _rx) = make_engine();
        engine.initialize().expect("init failed");

        engine
            .process_command(Command::CreateSession)
            .expect("create failed");

        let session_uuid = engine.active_session().expect("no active session").id.0;
        let session_id = SessionId(session_uuid);

        engine
            .process_command(Command::StartSession {
                session_id: session_uuid,
            })
            .expect("start failed");

        // Manually advance the session to AssistantSpeaking.
        {
            let session = engine
                .session_manager
                .get_session_mut(&session_id)
                .expect("session missing");
            session
                .transition(SessionState::UserSpeaking)
                .expect("to UserSpeaking");
            session
                .transition(SessionState::UserSpeechEnded)
                .expect("to UserSpeechEnded");
            session
                .transition(SessionState::Recognizing)
                .expect("to Recognizing");
            session
                .transition(SessionState::Thinking)
                .expect("to Thinking");
            session
                .transition(SessionState::AssistantSpeaking)
                .expect("to AssistantSpeaking");
        }

        engine
            .process_command(Command::InterruptPlayback {
                session_id: session_uuid,
            })
            .expect("interrupt failed");

        let session = engine.active_session().expect("no active session");
        assert_eq!(session.state, SessionState::Listening);
    }

    #[test]
    fn test_engine_poll_transport_events() {
        let (mut engine, _rx) = make_engine();
        engine.initialize().expect("init failed");

        engine
            .process_command(Command::CreateSession)
            .expect("create failed");

        let session_uuid = engine.active_session().expect("no active session").id.0;

        engine
            .process_command(Command::StartSession {
                session_id: session_uuid,
            })
            .expect("start failed");

        // Send 10 audio frames; the mock triggers a full conversation turn
        // at frame 10 (interim transcript at frame 5, then a complete
        // user-speech -> recognition -> assistant-reply -> playback cycle).
        let sid_str = session_uuid.to_string();
        for _ in 0..10 {
            engine
                .transport
                .send_audio_data(&sid_str, &[0u8; 160])
                .expect("audio send failed");
        }

        let events = engine.poll_transport_events().expect("poll events failed");

        // Should have received multiple events from the conversation turn.
        assert!(!events.is_empty());

        // The mock simulates the full turn cycle ending with PlaybackCompleted
        // which transitions the session back to Listening.
        let session = engine.active_session().expect("no active session");
        assert_eq!(session.state, SessionState::Listening);
    }

    #[test]
    fn test_engine_interrupt_ignored_when_not_speaking() {
        let (mut engine, _rx) = make_engine();
        engine.initialize().expect("init failed");

        engine
            .process_command(Command::CreateSession)
            .expect("create failed");

        let session_uuid = engine.active_session().expect("no active session").id.0;

        engine
            .process_command(Command::StartSession {
                session_id: session_uuid,
            })
            .expect("start failed");

        // Interrupt while Listening (not AssistantSpeaking) — should be a no-op.
        engine
            .process_command(Command::InterruptPlayback {
                session_id: session_uuid,
            })
            .expect("interrupt should succeed silently");

        let session = engine.active_session().expect("no active session");
        assert_eq!(session.state, SessionState::Listening);
    }

    // -- Reconnection tests -------------------------------------------------

    /// Helper: create an engine with an active session in Listening state.
    fn engine_with_listening_session() -> (Engine, crate::bus::EventSubscriber) {
        let (mut engine, rx) = make_engine();
        engine.initialize().expect("init failed");
        engine
            .process_command(Command::CreateSession)
            .expect("create failed");
        let session_uuid = engine.active_session().expect("no active session").id.0;
        engine
            .process_command(Command::StartSession {
                session_id: session_uuid,
            })
            .expect("start failed");
        (engine, rx)
    }

    /// Downcast the engine's transport to `MockGateway` for test manipulation.
    fn mock_gateway(engine: &mut Engine) -> &mut MockGateway {
        engine
            .transport
            .as_any_mut()
            .downcast_mut::<MockGateway>()
            .expect("transport should be MockGateway")
    }

    #[test]
    fn test_engine_reconnect_success() {
        let (mut engine, _rx) = engine_with_listening_session();
        let session_id = engine.active_session_id().expect("no active session");

        // Force transport into Disconnected state.
        mock_gateway(&mut engine).force_state(TransportState::Disconnected);

        // Attempt reconnect — should succeed because MockGateway::connect()
        // always succeeds.
        engine
            .attempt_reconnect()
            .expect("reconnect should succeed");

        // Session should be back in Listening.
        let session = engine
            .session_manager()
            .get_session(&session_id)
            .expect("session missing");
        assert_eq!(session.state, SessionState::Listening);

        // Reconnect counter should be reset.
        assert_eq!(engine.reconnect_attempts, 0);
    }

    #[test]
    fn test_engine_reconnect_max_attempts() {
        let (mut engine, _rx) = engine_with_listening_session();
        let session_id = engine.active_session_id().expect("no active session");

        // Exhaust all reconnect attempts by setting the counter to max.
        engine.reconnect_attempts = engine.max_reconnect_attempts;

        let result = engine.attempt_reconnect();
        assert!(result.is_err(), "should fail after max attempts");

        // Session should be Failed.
        let session = engine
            .session_manager()
            .get_session(&session_id)
            .expect("session missing");
        assert_eq!(session.state, SessionState::Failed);
    }

    #[test]
    fn test_engine_auto_reconnect_disabled() {
        let (mut engine, _rx) = engine_with_listening_session();

        engine.set_auto_reconnect(false);

        // Force transport into Disconnected state.
        mock_gateway(&mut engine).force_state(TransportState::Disconnected);

        // poll_transport_events should return empty without attempting reconnect.
        let events = engine.poll_transport_events().expect("poll should succeed");
        assert!(events.is_empty());

        // Session should still be Listening (no Recovering transition).
        let session = engine.active_session().expect("no active session");
        assert_eq!(session.state, SessionState::Listening);
    }

    #[test]
    fn test_engine_broadcast_error() {
        let (mut engine, mut rx) = make_engine();
        engine.initialize().expect("init failed");

        // Broadcast an error via the helper.
        engine.broadcast_error("test.code", "test message", true);

        // Collect the event from the subscriber. We need to use try_recv
        // since this is a sync test with a broadcast channel.
        // The broadcast channel is tokio-based, so we build a small runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime build failed");
        let event = rt.block_on(async {
            // Drain the AppReady event from initialize first.
            let _ = rx.recv().await;
            rx.recv().await
        });

        match event {
            Ok(Event::ErrorOccurred {
                code,
                message,
                recoverable,
            }) => {
                assert_eq!(code, "test.code");
                assert_eq!(message, "test message");
                assert!(recoverable);
            }
            other => panic!("expected ErrorOccurred, got {other:?}"),
        }
    }

    #[test]
    fn test_engine_session_timeout() {
        let (mut engine, _rx) = engine_with_listening_session();
        let session_id = engine.active_session_id().expect("no active session");

        // Set updated_at to 2 seconds in the past to guarantee timeout.
        {
            let session = engine
                .session_manager
                .get_session_mut(&session_id)
                .expect("session missing");
            session.updated_at = chrono::Utc::now() - chrono::Duration::milliseconds(2000);
        }

        let timed_out = engine.check_session_timeout(1000);
        assert!(timed_out, "session should have timed out after 1000ms");

        // Session should now be Recovering.
        let session = engine.active_session().expect("no active session");
        assert_eq!(session.state, SessionState::Recovering);
    }

    #[test]
    fn test_engine_recovering_to_listening() {
        let (mut engine, _rx) = engine_with_listening_session();
        let session_id = engine.active_session_id().expect("no active session");

        // Transition to Recovering.
        engine
            .try_transition(&session_id, SessionState::Recovering)
            .expect("transition to Recovering failed");
        let session = engine
            .session_manager()
            .get_session(&session_id)
            .expect("session missing");
        assert_eq!(session.state, SessionState::Recovering);

        // Transition back to Listening.
        engine
            .try_transition(&session_id, SessionState::Listening)
            .expect("transition to Listening failed");
        let session = engine
            .session_manager()
            .get_session(&session_id)
            .expect("session missing");
        assert_eq!(session.state, SessionState::Listening);
    }
}
