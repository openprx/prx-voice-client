//! Central engine coordinating sessions, transport, and events.
//!
//! The [`Engine`] is the primary orchestrator: it owns a
//! [`SessionManager`], a [`Transport`] implementation, and an
//! [`EventBroadcaster`]. Incoming [`Command`]s drive the session state
//! machine and are forwarded to the transport; resulting protocol
//! [`Event`]s are broadcast to all UI subscribers.

use std::time::{Duration, Instant};

use prxvc_protocol::transport::{Transport, TransportError, TransportState, TtsAudioChunk};
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
    /// Earliest time the next automatic reconnect attempt may run.
    next_reconnect_at: Option<Instant>,
    /// Set once the attempt cap has been reported, so it is reported once.
    reconnect_exhausted: bool,
    /// Whether new sessions play the assistant's spoken reply.
    auto_play_reply: bool,
    /// The caller plays the reply audio and reports when it has drained
    /// ([`Engine::playback_drained`]); `PlaybackCompleted` then only marks the
    /// end of the reply instead of returning the session to listening.
    external_playback: bool,
    /// The server announced the end of the current reply (`tts_end`) and the
    /// external player has not yet reported it drained.
    reply_end_pending: bool,
}

/// Longest wait between two automatic reconnect attempts.
const MAX_RECONNECT_DELAY_MS: u64 = 30_000;

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
            next_reconnect_at: None,
            reconnect_exhausted: false,
            auto_play_reply: true,
            external_playback: false,
            reply_end_pending: false,
        }
    }

    /// Initialize the engine by connecting the transport.
    ///
    /// Connects to the backend and polls for the initial [`Event::AppReady`]
    /// event, broadcasting it to all subscribers.
    pub fn initialize(&mut self) -> Result<(), PrxvcError> {
        self.transport.connect()?;
        self.clear_reconnect_state();

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
            Command::SendText { session_id, text } => self.handle_send_text(session_id, text),
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
                if self.auto_reconnect && !self.reconnect_exhausted {
                    // Wait out the backoff of the previous failed attempt.
                    if self.next_reconnect_at.is_some_and(|at| Instant::now() < at) {
                        return Ok(Vec::new());
                    }
                    let at_cap = self.reconnect_attempts >= self.max_reconnect_attempts;
                    if let Err(e) = self.attempt_reconnect() {
                        // The cap is reported once (it has been broadcast by
                        // now); stay quiet until a manual retry succeeds.
                        if at_cap {
                            self.reconnect_exhausted = true;
                        }
                        return Err(e);
                    }
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
            let _ = self.event_tx.broadcast(event.clone());
            broadcast_events.push(event.clone());

            // Drive the session state machine based on the event kind.
            self.apply_turn_event(&event, &mut broadcast_events);
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
        let delay = self.backoff_delay_ms(self.reconnect_attempts);
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
                self.clear_reconnect_state();
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
                // The next automatic attempt waits for the backoff.
                self.next_reconnect_at = Some(Instant::now() + Duration::from_millis(delay));
                Err(PrxvcError::Protocol(format!("reconnect failed: {e}")))
            }
        }
    }

    /// Reset the reconnection counter.
    ///
    /// Call this after successful communication to indicate the connection
    /// is healthy.
    pub fn reset_reconnect_counter(&mut self) {
        self.clear_reconnect_state();
    }

    /// Exponential backoff before attempt number `attempt` (1-based).
    fn backoff_delay_ms(&self, attempt: u32) -> u64 {
        let exp = attempt.saturating_sub(1).min(16);
        self.reconnect_delay_ms
            .saturating_mul(1u64 << exp)
            .min(MAX_RECONNECT_DELAY_MS)
    }

    fn clear_reconnect_state(&mut self) {
        self.reconnect_attempts = 0;
        self.next_reconnect_at = None;
        self.reconnect_exhausted = false;
    }

    /// Drain assistant audio the transport has received.
    ///
    /// Only audio of the active session is returned, and only when the
    /// session's `auto_play_reply` is on; anything else is discarded so it
    /// cannot pile up in the transport. The caller owns playback.
    pub fn poll_assistant_audio(&mut self) -> Vec<TtsAudioChunk> {
        let play = self
            .active_session()
            .is_some_and(|s| s.config.auto_play_reply);
        let active = self.active_session.map(|id| id.0);
        let mut out = Vec::new();
        while let Some(chunk) = self.transport.poll_audio() {
            if play && Some(chunk.session_id) == active {
                out.push(chunk);
            }
        }
        out
    }

    /// Set whether the assistant's spoken reply is played, for the active
    /// session and for sessions created from now on.
    pub fn set_auto_play_reply(&mut self, enabled: bool) {
        self.auto_play_reply = enabled;
        if let Some(id) = self.active_session
            && let Some(session) = self.session_manager.get_session_mut(&id)
        {
            session.config.auto_play_reply = enabled;
        }
    }

    /// Declare that the caller plays the assistant audio itself.
    ///
    /// With external playback the end-of-reply marker (`tts_end`) no longer
    /// returns the session to listening: audio may still be queued in the
    /// output. The caller finishes the reply with [`playback_drained`]
    /// once its output has drained.
    ///
    /// [`playback_drained`]: Engine::playback_drained
    pub fn set_external_playback(&mut self, enabled: bool) {
        self.external_playback = enabled;
    }

    /// Whether the server has announced the end of the current reply and the
    /// caller has not yet reported its output drained.
    pub fn reply_end_pending(&self) -> bool {
        self.reply_end_pending
    }

    /// The caller's output has played everything of the finished reply:
    /// return the session from speaking to listening.
    ///
    /// Does nothing (beyond clearing the pending marker) when the session is
    /// not speaking, e.g. because the reply was interrupted meanwhile.
    pub fn playback_drained(&mut self) {
        self.reply_end_pending = false;
        let Some(id) = self.active_session else {
            return;
        };
        let speaking = self
            .session_manager
            .get_session(&id)
            .is_some_and(|s| s.state == SessionState::AssistantSpeaking);
        if speaking {
            let _ = self.try_transition(&id, SessionState::Listening);
        }
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
            auto_play_reply: self.auto_play_reply,
            save_history: false,
            interrupt_mode: prxvc_session::InterruptMode::VoiceActivity,
            transcript_mode: prxvc_session::TranscriptMode::Full,
        };

        let session_id = self.session_manager.create_session(config);

        // Forward to transport, which maps this ID to the backend's own.
        if let Err(e) = self.transport.create_session(session_id.0) {
            // The session never existed on the backend.
            let _ = self.session_manager.remove_session(&session_id);
            return Err(e.into());
        }

        // Relay what the transport produced, except the SessionCreated for
        // the ID broadcast below.
        while let Some(event) = self.transport.poll_event()? {
            if event
                == (Event::SessionCreated {
                    session_id: session_id.0,
                })
            {
                continue;
            }
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

        // The interrupted reply has no end to wait for.
        self.reply_end_pending = false;

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

    /// Cancel the reply the server is preparing for the last turn (the
    /// assistant has not started speaking yet), so a corrected or extended
    /// turn can replace it. The session state is left alone: the next
    /// [`Command::SendText`] moves it on. Returns whether a cancel was sent.
    ///
    /// A reply that already speaks is cut off with
    /// [`Command::InterruptPlayback`] instead.
    ///
    /// # Errors
    /// Propagates a transport failure.
    pub fn cancel_pending_reply(&mut self) -> Result<bool, PrxvcError> {
        let Some(session) = self.active_session() else {
            return Ok(false);
        };
        if !matches!(
            session.state,
            SessionState::Thinking | SessionState::Recognizing
        ) {
            return Ok(false);
        }
        let session_id = session.id.0;
        self.transport
            .send_command(&Command::InterruptPlayback { session_id })?;
        Ok(true)
    }

    /// Forward locally recognised text to the server and move the session
    /// through the turn: the user spoke, the speech was recognised, the
    /// reply is awaited.
    fn handle_send_text(
        &mut self,
        session_uuid: uuid::Uuid,
        text: String,
    ) -> Result<(), PrxvcError> {
        let session_id = SessionId(session_uuid);
        let state = self
            .session_manager
            .get_session(&session_id)
            .map(|s| s.state);

        // Speaking over the assistant cuts it off first.
        if state == Some(SessionState::AssistantSpeaking) {
            self.handle_interrupt_playback(session_uuid)?;
        }

        self.transport.send_command(&Command::SendText {
            session_id: session_uuid,
            text,
        })?;

        let state = self
            .session_manager
            .get_session(&session_id)
            .map(|s| s.state);
        if let Some(path) = state.and_then(send_text_path) {
            let mut sink = Vec::new();
            self.try_transition_sequence(&session_id, path, &mut sink);
        }

        // Whatever the transport answered immediately drives the turn too.
        while let Some(event) = self.transport.poll_event()? {
            let _ = self.event_tx.broadcast(event.clone());
            let mut sink = Vec::new();
            self.apply_turn_event(&event, &mut sink);
        }
        Ok(())
    }

    /// Advance the active session's state machine for a transport event.
    ///
    /// Events that do not fit the session's current state (stale events of an
    /// interrupted reply, repeats) are ignored rather than forced.
    fn apply_turn_event(&mut self, event: &Event, broadcast_events: &mut Vec<Event>) {
        let (session_uuid, kind) = match event {
            Event::FinalTranscriptProduced { session_id, .. } => {
                (*session_id, TurnEvent::FinalTranscript)
            }
            Event::AssistantReplyStarted { session_id } => (*session_id, TurnEvent::ReplyStarted),
            Event::PlaybackCompleted { session_id } => (*session_id, TurnEvent::ReplyEnded),
            _ => return,
        };
        let Some(active_id) = self.active_session else {
            return;
        };
        if SessionId(session_uuid) != active_id {
            return;
        }
        let Some(state) = self
            .session_manager
            .get_session(&active_id)
            .map(|s| s.state)
        else {
            return;
        };

        match kind {
            TurnEvent::ReplyEnded if self.external_playback => {
                self.reply_end_pending = true;
            }
            TurnEvent::ReplyEnded => {
                if state == SessionState::AssistantSpeaking {
                    self.try_transition_sequence(
                        &active_id,
                        &[SessionState::Listening],
                        broadcast_events,
                    );
                }
            }
            _ => match turn_path(state, kind) {
                Some(path) => self.try_transition_sequence(&active_id, path, broadcast_events),
                None => tracing::debug!(?state, ?kind, "turn event ignored in this state"),
            },
        }
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

/// Transport events that move a turn along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnEvent {
    /// A final transcript of speech the server recognised.
    FinalTranscript,
    /// The server started the spoken reply (`tts_start`).
    ReplyStarted,
    /// The server finished the spoken reply (`tts_end`).
    ReplyEnded,
}

/// Transitions to apply for `kind` when the session is in `state`, or `None`
/// when the event does not belong there.
fn turn_path(state: SessionState, kind: TurnEvent) -> Option<&'static [SessionState]> {
    use SessionState::*;
    match (kind, state) {
        // Server-side recognition of speech the client streamed.
        (TurnEvent::FinalTranscript, Listening) => {
            Some(&[UserSpeaking, UserSpeechEnded, Recognizing])
        }
        // The reply starts speaking once the client is waiting for it.
        (TurnEvent::ReplyStarted, Recognizing) => Some(&[Thinking, AssistantSpeaking]),
        (TurnEvent::ReplyStarted, Thinking) => Some(&[AssistantSpeaking]),
        _ => None,
    }
}

/// Transitions for locally recognised text sent in `state`: speech heard and
/// recognised, now waiting for the reply. A new utterance replaces a turn
/// that never got to speak.
fn send_text_path(state: SessionState) -> Option<&'static [SessionState]> {
    use SessionState::*;
    match state {
        Listening => Some(&[UserSpeaking, UserSpeechEnded, Recognizing, Thinking]),
        Thinking => Some(&[
            Listening,
            UserSpeaking,
            UserSpeechEnded,
            Recognizing,
            Thinking,
        ]),
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

    // -----------------------------------------------------------------------
    // Stub transport for reconnect / handshake / audio tests
    // -----------------------------------------------------------------------

    use prxvc_protocol::transport::TtsAudioChunk;
    use std::collections::VecDeque;

    /// Transport whose `connect` can be made to fail and that records the
    /// engine IDs it is asked to create sessions for.
    struct StubTransport {
        state: TransportState,
        connect_ok: bool,
        connect_calls: u32,
        created: Vec<uuid::Uuid>,
        audio: VecDeque<TtsAudioChunk>,
        events: VecDeque<Event>,
        sent: Vec<Command>,
    }

    impl StubTransport {
        fn new() -> Self {
            Self {
                state: TransportState::Connected,
                connect_ok: true,
                connect_calls: 0,
                created: Vec::new(),
                audio: VecDeque::new(),
                events: VecDeque::new(),
                sent: Vec::new(),
            }
        }
    }

    impl Transport for StubTransport {
        fn connect(&mut self) -> Result<(), TransportError> {
            self.connect_calls += 1;
            if self.connect_ok {
                self.state = TransportState::Connected;
                Ok(())
            } else {
                self.state = TransportState::Disconnected;
                Err(TransportError::ConnectionFailed("stub".into()))
            }
        }
        fn disconnect(&mut self) -> Result<(), TransportError> {
            self.state = TransportState::Disconnected;
            Ok(())
        }
        fn create_session(&mut self, session_id: uuid::Uuid) -> Result<(), TransportError> {
            self.created.push(session_id);
            Ok(())
        }
        fn send_command(&mut self, command: &Command) -> Result<(), TransportError> {
            self.sent.push(command.clone());
            Ok(())
        }
        fn send_audio_data(&mut self, _: &str, _: &[u8]) -> Result<(), TransportError> {
            Ok(())
        }
        fn poll_event(&mut self) -> Result<Option<Event>, TransportError> {
            Ok(self.events.pop_front())
        }
        fn poll_audio(&mut self) -> Option<TtsAudioChunk> {
            self.audio.pop_front()
        }
        fn state(&self) -> TransportState {
            self.state.clone()
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn stub_engine() -> Engine {
        let (event_tx, _rx) = event_channel(64);
        Engine::new(Box::new(StubTransport::new()), event_tx)
    }

    fn stub(engine: &mut Engine) -> &mut StubTransport {
        engine
            .transport
            .as_any_mut()
            .downcast_mut::<StubTransport>()
            .expect("stub transport")
    }

    #[test]
    fn test_create_session_hands_the_engine_id_to_the_transport() {
        let mut engine = stub_engine();
        engine
            .process_command(Command::CreateSession)
            .expect("create");
        let id = engine.active_session_id().expect("active").0;
        assert_eq!(stub(&mut engine).created, vec![id]);
    }

    #[test]
    fn test_reconnect_backoff_is_waited_out() {
        let mut engine = stub_engine();
        {
            let t = stub(&mut engine);
            t.state = TransportState::Disconnected;
            t.connect_ok = false;
        }

        // First failure: attempted immediately.
        let _ = engine.poll_transport_events();
        assert_eq!(stub(&mut engine).connect_calls, 1);
        assert!(engine.next_reconnect_at.is_some());

        // Polling again right away must not retry: the backoff is pending.
        for _ in 0..5 {
            let events = engine.poll_transport_events().expect("poll");
            assert!(events.is_empty());
        }
        assert_eq!(stub(&mut engine).connect_calls, 1);

        // Once the backoff has elapsed the next attempt runs.
        engine.next_reconnect_at = Some(Instant::now() - Duration::from_millis(1));
        let _ = engine.poll_transport_events();
        assert_eq!(stub(&mut engine).connect_calls, 2);
    }

    #[test]
    fn test_backoff_grows_exponentially_and_is_capped() {
        let engine = stub_engine();
        assert_eq!(engine.backoff_delay_ms(1), 1_000);
        assert_eq!(engine.backoff_delay_ms(2), 2_000);
        assert_eq!(engine.backoff_delay_ms(3), 4_000);
        assert_eq!(engine.backoff_delay_ms(30), MAX_RECONNECT_DELAY_MS);
    }

    #[test]
    fn test_reconnect_gives_up_once_and_a_manual_retry_recovers() {
        let mut engine = stub_engine();
        {
            let t = stub(&mut engine);
            t.state = TransportState::Disconnected;
            t.connect_ok = false;
        }
        // Burn through every attempt, skipping the waits.
        for _ in 0..engine.max_reconnect_attempts {
            engine.next_reconnect_at = None;
            let _ = engine.poll_transport_events();
        }
        engine.next_reconnect_at = None;
        assert!(engine.poll_transport_events().is_err(), "cap reported once");
        let calls = stub(&mut engine).connect_calls;
        for _ in 0..3 {
            assert!(engine.poll_transport_events().expect("quiet").is_empty());
        }
        assert_eq!(
            stub(&mut engine).connect_calls,
            calls,
            "no further attempts"
        );

        // A manual retry that succeeds re-arms automatic reconnection.
        stub(&mut engine).connect_ok = true;
        engine.initialize().expect("manual retry");
        assert!(!engine.reconnect_exhausted);
        assert_eq!(engine.reconnect_attempts, 0);
    }

    fn chunk(session_id: uuid::Uuid) -> TtsAudioChunk {
        TtsAudioChunk {
            session_id,
            sample_rate: 16_000,
            pcm16: vec![0; 640],
        }
    }

    #[test]
    fn test_assistant_audio_is_delivered_for_the_active_session_only() {
        let mut engine = stub_engine();
        engine.process_command(Command::CreateSession).unwrap();
        let id = engine.active_session_id().unwrap().0;
        {
            let t = stub(&mut engine);
            t.audio.push_back(chunk(id));
            t.audio.push_back(chunk(uuid::Uuid::new_v4()));
        }
        let out = engine.poll_assistant_audio();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].session_id, id);
        assert!(stub(&mut engine).audio.is_empty());
    }

    #[test]
    fn test_assistant_audio_is_discarded_when_auto_play_is_off() {
        let mut engine = stub_engine();
        engine.process_command(Command::CreateSession).unwrap();
        let id = engine.active_session_id().unwrap().0;
        engine.set_auto_play_reply(false);
        stub(&mut engine).audio.push_back(chunk(id));
        assert!(engine.poll_assistant_audio().is_empty());
        assert!(stub(&mut engine).audio.is_empty(), "discarded, not queued");
    }

    // -----------------------------------------------------------------------
    // Live turn: locally recognised text, server-driven reply
    // -----------------------------------------------------------------------

    /// Engine over the stub transport with an active, listening session, in
    /// external-playback mode (as the desktop app runs it), plus a receiver
    /// of everything the engine broadcasts.
    fn live_engine() -> (Engine, uuid::Uuid, crate::bus::EventSubscriber) {
        let (event_tx, rx) = event_channel(256);
        let mut engine = Engine::new(Box::new(StubTransport::new()), event_tx);
        engine.set_external_playback(true);
        engine
            .process_command(Command::CreateSession)
            .expect("create");
        let id = engine.active_session_id().expect("active").0;
        engine
            .process_command(Command::StartSession { session_id: id })
            .expect("start");
        (engine, id, rx)
    }

    /// Everything broadcast so far, as state names and error markers.
    fn drain_states(rx: &mut crate::bus::EventSubscriber) -> Vec<String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut out = Vec::new();
        rt.block_on(async {
            while let Ok(Ok(event)) =
                tokio::time::timeout(Duration::from_millis(5), rx.recv()).await
            {
                match event {
                    Event::SessionStateChanged { to, .. } => out.push(to),
                    Event::ErrorOccurred { code, .. } => out.push(format!("error:{code}")),
                    _ => {}
                }
            }
        });
        out
    }

    fn state_of(engine: &Engine) -> SessionState {
        engine.active_session().expect("active").state
    }

    fn send_text(engine: &mut Engine, id: uuid::Uuid, text: &str) {
        engine
            .process_command(Command::SendText {
                session_id: id,
                text: text.into(),
            })
            .expect("send text");
    }

    /// Feed the events of a spoken reply to the engine.
    fn reply_events(engine: &mut Engine, events: Vec<Event>) {
        stub(engine).events.extend(events);
        engine.poll_transport_events().expect("poll");
    }

    fn spoken_reply(id: uuid::Uuid) -> Vec<Event> {
        vec![
            Event::SessionStateChanged {
                session_id: id,
                from: String::new(),
                to: "Thinking".into(),
            },
            Event::AssistantReplyTextDelta {
                session_id: id,
                delta: "Hello".into(),
            },
            Event::AssistantReplyTextDelta {
                session_id: id,
                delta: " there".into(),
            },
            Event::AssistantReplyStarted { session_id: id },
            Event::PlaybackCompleted { session_id: id },
        ]
    }

    #[test]
    fn test_live_turn_follows_the_states_without_transition_errors() {
        let (mut engine, id, mut rx) = live_engine();
        let _ = drain_states(&mut rx);

        // Speech recognised locally and sent: waiting for the reply.
        send_text(&mut engine, id, "hello");
        assert_eq!(state_of(&engine), SessionState::Thinking);
        assert!(stub(&mut engine).sent.contains(&Command::SendText {
            session_id: id,
            text: "hello".into()
        }));

        // Reply tokens arrive; the reply starts speaking; tts_end comes.
        reply_events(&mut engine, spoken_reply(id));
        assert_eq!(state_of(&engine), SessionState::AssistantSpeaking);
        assert!(engine.reply_end_pending(), "tts_end only marks the end");

        // Back to listening only once the output has drained.
        engine.playback_drained();
        assert_eq!(state_of(&engine), SessionState::Listening);
        assert!(!engine.reply_end_pending());

        let states = drain_states(&mut rx);
        let own: Vec<&str> = states.iter().map(String::as_str).collect();
        assert!(!own.iter().any(|s| s.starts_with("error:")), "{own:?}");
        assert_eq!(
            own,
            [
                "UserSpeaking",
                "UserSpeechEnded",
                "Recognizing",
                "Thinking",
                "Thinking", // the server's status message, forwarded as is
                "AssistantSpeaking",
                "Listening",
            ]
        );
    }

    #[test]
    fn test_second_turn_right_after_the_first() {
        let (mut engine, id, mut rx) = live_engine();
        for text in ["one", "two"] {
            send_text(&mut engine, id, text);
            assert_eq!(state_of(&engine), SessionState::Thinking);
            reply_events(&mut engine, spoken_reply(id));
            assert_eq!(state_of(&engine), SessionState::AssistantSpeaking);
            engine.playback_drained();
            assert_eq!(state_of(&engine), SessionState::Listening);
        }
        let states = drain_states(&mut rx);
        assert!(
            !states.iter().any(|s| s.starts_with("error:")),
            "{states:?}"
        );
        assert_eq!(
            states.iter().filter(|s| *s == "AssistantSpeaking").count(),
            2
        );
    }

    #[test]
    fn test_cancel_pending_reply_only_while_the_reply_is_awaited() {
        let (mut engine, id, _rx) = live_engine();
        // Listening: nothing to cancel.
        assert!(!engine.cancel_pending_reply().expect("cancel"));
        send_text(&mut engine, id, "first part");
        assert_eq!(state_of(&engine), SessionState::Thinking);
        assert!(engine.cancel_pending_reply().expect("cancel"));
        assert!(
            stub(&mut engine)
                .sent
                .contains(&Command::InterruptPlayback { session_id: id })
        );
        // The state is untouched and the replacement turn is accepted.
        assert_eq!(state_of(&engine), SessionState::Thinking);
        send_text(&mut engine, id, "first part and second part");
        assert_eq!(state_of(&engine), SessionState::Thinking);
        // Speaking: the playback interrupt is the way, not this.
        stub(&mut engine)
            .events
            .push_back(Event::AssistantReplyStarted { session_id: id });
        engine.poll_transport_events().expect("poll");
        assert!(!engine.cancel_pending_reply().expect("cancel"));
    }

    #[test]
    fn test_interrupt_during_speaking_returns_to_listening() {
        let (mut engine, id, mut rx) = live_engine();
        send_text(&mut engine, id, "tell me a story");
        stub(&mut engine)
            .events
            .push_back(Event::AssistantReplyStarted { session_id: id });
        engine.poll_transport_events().expect("poll");
        assert_eq!(state_of(&engine), SessionState::AssistantSpeaking);

        engine
            .process_command(Command::InterruptPlayback { session_id: id })
            .expect("interrupt");
        assert_eq!(state_of(&engine), SessionState::Listening);
        assert!(
            stub(&mut engine)
                .sent
                .contains(&Command::InterruptPlayback { session_id: id })
        );

        // The interrupted reply's late tts_end must not disturb anything.
        stub(&mut engine)
            .events
            .push_back(Event::PlaybackCompleted { session_id: id });
        engine.poll_transport_events().expect("poll");
        engine.playback_drained();
        assert_eq!(state_of(&engine), SessionState::Listening);

        let states = drain_states(&mut rx);
        assert!(
            !states.iter().any(|s| s.starts_with("error:")),
            "{states:?}"
        );
        assert!(states.contains(&"Interrupted".to_owned()));
    }

    #[test]
    fn test_speaking_over_the_assistant_interrupts_it_first() {
        let (mut engine, id, mut rx) = live_engine();
        send_text(&mut engine, id, "first");
        stub(&mut engine)
            .events
            .push_back(Event::AssistantReplyStarted { session_id: id });
        engine.poll_transport_events().expect("poll");
        assert_eq!(state_of(&engine), SessionState::AssistantSpeaking);

        send_text(&mut engine, id, "second");
        assert_eq!(state_of(&engine), SessionState::Thinking);
        let sent = stub(&mut engine).sent.clone();
        let interrupt_at = sent
            .iter()
            .position(|c| matches!(c, Command::InterruptPlayback { .. }));
        let second_at = sent
            .iter()
            .position(|c| matches!(c, Command::SendText { text, .. } if text == "second"));
        assert!(
            interrupt_at.is_some() && interrupt_at < second_at,
            "{sent:?}"
        );
        let states = drain_states(&mut rx);
        assert!(
            !states.iter().any(|s| s.starts_with("error:")),
            "{states:?}"
        );
    }

    #[test]
    fn test_new_utterance_replaces_a_turn_that_never_spoke() {
        let (mut engine, id, mut rx) = live_engine();
        send_text(&mut engine, id, "first");
        assert_eq!(state_of(&engine), SessionState::Thinking);
        send_text(&mut engine, id, "second");
        assert_eq!(state_of(&engine), SessionState::Thinking);
        let states = drain_states(&mut rx);
        assert!(
            !states.iter().any(|s| s.starts_with("error:")),
            "{states:?}"
        );
    }

    #[test]
    fn test_stale_events_are_ignored_not_forced() {
        let (mut engine, id, _rx) = live_engine();
        // A reply start and end while Listening (stale after an interrupt).
        stub(&mut engine).events.extend([
            Event::AssistantReplyStarted { session_id: id },
            Event::PlaybackCompleted { session_id: id },
        ]);
        engine.poll_transport_events().expect("poll");
        assert_eq!(state_of(&engine), SessionState::Listening);
    }

    #[test]
    fn test_without_external_playback_tts_end_returns_to_listening() {
        let (mut engine, id, _rx) = live_engine();
        engine.set_external_playback(false);
        send_text(&mut engine, id, "hello");
        reply_events(&mut engine, spoken_reply(id));
        assert_eq!(state_of(&engine), SessionState::Listening);
    }
}
