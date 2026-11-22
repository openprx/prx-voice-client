//! Mock gateway for development and testing.
//!
//! Provides [`MockGateway`], a [`Transport`]
//! implementation that simulates a backend by echoing commands as events.

use std::collections::VecDeque;

use uuid::Uuid;

use crate::transport::{Transport, TransportError, TransportState};
use crate::{Command, DeviceInfo, DeviceKind, Event};

/// Mock gateway that simulates a backend for development and testing.
///
/// Commands are logged and translated into corresponding response events
/// that are queued for retrieval via [`Transport::poll_event`].
///
/// Beyond basic command echoing, this gateway can simulate realistic
/// multi-turn conversation flows including user speech recognition,
/// assistant replies, and interruption handling.
pub struct MockGateway {
    /// Current transport state.
    state: TransportState,
    /// Pending events waiting to be polled.
    event_queue: VecDeque<Event>,
    /// History of all commands sent through this gateway.
    command_log: Vec<Command>,
    /// Internal counter for audio data calls (used to generate transcripts).
    audio_counter: u64,
    /// Simulated conversation turn counter.
    conversation_turn: u32,
    /// Whether a response is "in progress" (prevents overlapping turns).
    response_pending: bool,
    /// Simulated response phrases for testing.
    response_phrases: Vec<String>,
    /// Session id stored when `StartSession` is processed, used by audio-triggered turns.
    current_session_id: Option<Uuid>,
}

impl MockGateway {
    /// Create a new `MockGateway` in the [`Disconnected`](TransportState::Disconnected) state.
    ///
    /// The gateway is initialised with a default set of response phrases used
    /// when simulating conversation turns.
    pub fn new() -> Self {
        Self {
            state: TransportState::Disconnected,
            event_queue: VecDeque::new(),
            command_log: Vec::new(),
            audio_counter: 0,
            conversation_turn: 0,
            response_pending: false,
            response_phrases: vec![
                "I understand your question. Let me help you with that.".into(),
                "That's a great point. Here's what I think...".into(),
                "Based on my analysis, I would recommend the following approach.".into(),
                "Let me break that down for you step by step.".into(),
                "I hope that helps! Is there anything else you'd like to know?".into(),
            ],
            current_session_id: None,
        }
    }

    /// Inject an event into the queue for testing purposes.
    pub fn inject_event(&mut self, event: Event) {
        self.event_queue.push_back(event);
    }

    /// Return a reference to all commands sent through this gateway.
    pub fn command_log(&self) -> &[Command] {
        &self.command_log
    }

    /// Clear the command history.
    pub fn clear_command_log(&mut self) {
        self.command_log.clear();
    }

    /// Simulate a complete conversation turn: user speech, recognition, and AI response.
    ///
    /// Call this to generate a sequence of events that represent one full turn
    /// of a voice conversation. The response phrase is selected from
    /// [`response_phrases`](Self::set_response_phrases) using the current turn
    /// counter (wrapping around when the counter exceeds the number of phrases).
    pub fn simulate_conversation_turn(&mut self, session_id: Uuid) {
        // 1. User starts speaking
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "Listening".to_owned(),
            to: "UserSpeaking".to_owned(),
        });

        // 2. Interim transcript
        self.event_queue
            .push_back(Event::InterimTranscriptProduced {
                session_id,
                text: "Hello...".to_owned(),
            });

        // 3. Final transcript
        self.event_queue.push_back(Event::FinalTranscriptProduced {
            session_id,
            text: "Hello, can you help me with this?".to_owned(),
        });

        // 4. Recognizing
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "UserSpeaking".to_owned(),
            to: "Recognizing".to_owned(),
        });

        // 5. Thinking
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "Recognizing".to_owned(),
            to: "Thinking".to_owned(),
        });

        // 6. Assistant reply started
        self.event_queue
            .push_back(Event::AssistantReplyStarted { session_id });

        // 7. Assistant reply text delta (pick phrase based on turn counter)
        let phrase_index = if self.response_phrases.is_empty() {
            None
        } else {
            Some(self.conversation_turn as usize % self.response_phrases.len())
        };
        let delta = phrase_index
            .map(|i| self.response_phrases[i].clone())
            .unwrap_or_default();
        self.event_queue
            .push_back(Event::AssistantReplyTextDelta { session_id, delta });

        // 8. Assistant speaking
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "Thinking".to_owned(),
            to: "AssistantSpeaking".to_owned(),
        });

        // 9. Playback completed
        self.event_queue
            .push_back(Event::PlaybackCompleted { session_id });

        // 10. Back to listening
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "AssistantSpeaking".to_owned(),
            to: "Listening".to_owned(),
        });

        self.conversation_turn += 1;
        self.response_pending = false;
    }

    /// Simulate an interrupt during assistant speaking.
    ///
    /// Pushes a [`PlaybackInterrupted`](Event::PlaybackInterrupted) event
    /// followed by state transitions through `Interrupted` back to `Listening`.
    pub fn simulate_interrupt(&mut self, session_id: Uuid) {
        self.event_queue
            .push_back(Event::PlaybackInterrupted { session_id });
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "AssistantSpeaking".to_owned(),
            to: "Interrupted".to_owned(),
        });
        self.event_queue.push_back(Event::SessionStateChanged {
            session_id,
            from: "Interrupted".to_owned(),
            to: "Listening".to_owned(),
        });
    }

    /// Set custom response phrases for testing.
    ///
    /// These phrases are cycled through by [`simulate_conversation_turn`](Self::simulate_conversation_turn)
    /// using the conversation turn counter modulo the number of phrases.
    pub fn set_response_phrases(&mut self, phrases: Vec<String>) {
        self.response_phrases = phrases;
    }

    /// Force the transport into a specific state for testing purposes.
    ///
    /// This bypasses the normal connect/disconnect flow and directly sets the
    /// internal state. Useful for simulating failure and reconnection scenarios.
    pub fn force_state(&mut self, state: TransportState) {
        self.state = state;
    }
}

impl Default for MockGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for MockGateway {
    fn connect(&mut self) -> Result<(), TransportError> {
        self.state = TransportState::Connected;
        self.event_queue.push_back(Event::AppReady);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        self.state = TransportState::Disconnected;
        Ok(())
    }

    fn send_command(&mut self, command: &Command) -> Result<(), TransportError> {
        if self.state != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        self.command_log.push(command.clone());

        match command {
            Command::CreateSession => {
                let session_id = Uuid::now_v7();
                self.event_queue
                    .push_back(Event::SessionCreated { session_id });
            }
            Command::StartSession { session_id } => {
                self.current_session_id = Some(*session_id);
                self.event_queue.push_back(Event::SessionStarted {
                    session_id: *session_id,
                });
                self.event_queue.push_back(Event::SessionStateChanged {
                    session_id: *session_id,
                    from: "Created".to_owned(),
                    to: "Listening".to_owned(),
                });
            }
            Command::StopSession { session_id } => {
                self.event_queue.push_back(Event::SessionStateChanged {
                    session_id: *session_id,
                    from: "Active".to_owned(),
                    to: "Completed".to_owned(),
                });
                self.conversation_turn = 0;
                self.response_pending = false;
                self.current_session_id = None;
            }
            Command::InterruptPlayback { session_id } => {
                self.event_queue.push_back(Event::PlaybackInterrupted {
                    session_id: *session_id,
                });
            }
            Command::RefreshDeviceList => {
                self.event_queue.push_back(Event::DeviceListUpdated {
                    devices: vec![
                        DeviceInfo {
                            id: "mock-input-1".to_owned(),
                            name: "Mock Mic".to_owned(),
                            kind: DeviceKind::Input,
                            is_default: true,
                        },
                        DeviceInfo {
                            id: "mock-output-1".to_owned(),
                            name: "Mock Speaker".to_owned(),
                            kind: DeviceKind::Output,
                            is_default: true,
                        },
                    ],
                });
            }
            Command::SendText { session_id, text } => {
                // Simulate a full conversation turn triggered by local ASR text.
                self.event_queue.push_back(Event::FinalTranscriptProduced {
                    session_id: *session_id,
                    text: text.clone(),
                });
                let sid = self.current_session_id.unwrap_or(*session_id);
                self.simulate_conversation_turn(sid);
            }
            Command::ExportDiagnostics => {
                self.event_queue.push_back(Event::DiagnosticsExported {
                    path: "/tmp/mock-diagnostics.json".to_owned(),
                });
            }
            other => {
                // For any other command, produce a generic state change event.
                // Extract a session_id if one is available, otherwise use a nil UUID.
                let session_id = match other {
                    Command::PauseSession { session_id }
                    | Command::ResumeSession { session_id } => *session_id,
                    _ => Uuid::nil(),
                };
                self.event_queue.push_back(Event::SessionStateChanged {
                    session_id,
                    from: "Unknown".to_owned(),
                    to: "Updated".to_owned(),
                });
            }
        }

        Ok(())
    }

    fn send_audio_data(&mut self, session_id: &str, _data: &[u8]) -> Result<(), TransportError> {
        if self.state != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        self.audio_counter += 1;

        let parsed_id = Uuid::parse_str(session_id)
            .map_err(|e| TransportError::SendFailed(format!("invalid session id: {e}")))?;

        if self.audio_counter.is_multiple_of(10) && !self.response_pending {
            self.response_pending = true;
            self.simulate_conversation_turn(parsed_id);
        } else if self.audio_counter.is_multiple_of(5) {
            self.event_queue
                .push_back(Event::InterimTranscriptProduced {
                    session_id: parsed_id,
                    text: format!("Mock interim transcript #{}", self.audio_counter / 5),
                });
        }

        Ok(())
    }

    fn poll_event(&mut self) -> Result<Option<Event>, TransportError> {
        Ok(self.event_queue.pop_front())
    }

    fn state(&self) -> TransportState {
        self.state.clone()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_gateway_connect_produces_app_ready() {
        let mut gw = MockGateway::new();
        assert_eq!(gw.state(), TransportState::Disconnected);

        gw.connect().unwrap();
        assert_eq!(gw.state(), TransportState::Connected);

        let event = gw.poll_event().unwrap();
        assert_eq!(event, Some(Event::AppReady));
    }

    #[test]
    fn test_mock_gateway_send_create_session() {
        let mut gw = MockGateway::new();
        gw.connect().unwrap();
        let _ = gw.poll_event(); // consume AppReady

        gw.send_command(&Command::CreateSession).unwrap();

        let event = gw.poll_event().unwrap();
        assert!(
            matches!(event, Some(Event::SessionCreated { .. })),
            "expected SessionCreated, got {event:?}"
        );
    }

    #[test]
    fn test_mock_gateway_send_start_session() {
        let mut gw = MockGateway::new();
        gw.connect().unwrap();
        let _ = gw.poll_event(); // consume AppReady

        let sid = Uuid::now_v7();
        gw.send_command(&Command::StartSession { session_id: sid })
            .unwrap();

        let event = gw.poll_event().unwrap();
        assert_eq!(event, Some(Event::SessionStarted { session_id: sid }));

        // StartSession now also emits a state change to Listening.
        let event2 = gw.poll_event().unwrap();
        assert_eq!(
            event2,
            Some(Event::SessionStateChanged {
                session_id: sid,
                from: "Created".to_owned(),
                to: "Listening".to_owned(),
            })
        );
    }

    #[test]
    fn test_mock_gateway_audio_data_produces_transcripts() {
        let mut gw = MockGateway::new();
        gw.connect().unwrap();
        let _ = gw.poll_event(); // consume AppReady

        let sid = Uuid::now_v7();
        let sid_str = sid.to_string();
        let data = [0u8; 160];

        // Send 10 audio frames; at frame 5 an interim transcript is produced,
        // and at frame 10 a full conversation turn is triggered.
        for _ in 0..10 {
            gw.send_audio_data(&sid_str, &data).unwrap();
        }

        let mut interim_count = 0u32;
        let mut final_count = 0u32;
        let mut reply_started = false;

        while let Ok(Some(event)) = gw.poll_event() {
            match event {
                Event::InterimTranscriptProduced { .. } => interim_count += 1,
                Event::FinalTranscriptProduced { .. } => final_count += 1,
                Event::AssistantReplyStarted { .. } => reply_started = true,
                _ => {}
            }
        }

        // The conversation turn produces its own interim + final transcript.
        assert!(interim_count >= 1, "expected at least 1 interim transcript");
        assert!(final_count >= 1, "expected at least 1 final transcript");
        assert!(reply_started, "expected assistant reply to start");
    }

    #[test]
    fn test_mock_gateway_disconnect() {
        let mut gw = MockGateway::new();
        gw.connect().unwrap();
        gw.disconnect().unwrap();
        assert_eq!(gw.state(), TransportState::Disconnected);

        // Sending a command while disconnected should fail.
        let result = gw.send_command(&Command::CreateSession);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_gateway_inject_event() {
        let mut gw = MockGateway::new();
        let injected = Event::ErrorOccurred {
            code: "test.error".to_owned(),
            message: "injected error".to_owned(),
            recoverable: true,
        };

        gw.inject_event(injected.clone());

        let event = gw.poll_event().unwrap();
        assert_eq!(event, Some(injected));
    }

    #[test]
    fn test_mock_gateway_command_log() {
        let mut gw = MockGateway::new();
        gw.connect().unwrap();

        gw.send_command(&Command::CreateSession).unwrap();
        gw.send_command(&Command::RefreshDeviceList).unwrap();

        assert_eq!(gw.command_log().len(), 2);
        assert_eq!(gw.command_log()[0], Command::CreateSession);
        assert_eq!(gw.command_log()[1], Command::RefreshDeviceList);

        gw.clear_command_log();
        assert!(gw.command_log().is_empty());
    }

    #[test]
    fn test_simulate_conversation_turn_produces_correct_events() {
        let mut gw = MockGateway::new();
        let sid = Uuid::now_v7();

        gw.simulate_conversation_turn(sid);

        let events: Vec<Event> = std::iter::from_fn(|| gw.poll_event().ok().flatten()).collect();

        assert_eq!(
            events.len(),
            10,
            "expected 10 events in a conversation turn"
        );

        // 1. UserSpeaking
        assert_eq!(
            events[0],
            Event::SessionStateChanged {
                session_id: sid,
                from: "Listening".to_owned(),
                to: "UserSpeaking".to_owned(),
            }
        );
        // 2. Interim transcript
        assert!(
            matches!(&events[1], Event::InterimTranscriptProduced { text, .. } if text == "Hello..."),
            "expected interim transcript"
        );
        // 3. Final transcript
        assert!(
            matches!(&events[2], Event::FinalTranscriptProduced { text, .. } if text == "Hello, can you help me with this?"),
            "expected final transcript"
        );
        // 4. Recognizing
        assert_eq!(
            events[3],
            Event::SessionStateChanged {
                session_id: sid,
                from: "UserSpeaking".to_owned(),
                to: "Recognizing".to_owned(),
            }
        );
        // 5. Thinking
        assert_eq!(
            events[4],
            Event::SessionStateChanged {
                session_id: sid,
                from: "Recognizing".to_owned(),
                to: "Thinking".to_owned(),
            }
        );
        // 6. AssistantReplyStarted
        assert_eq!(events[5], Event::AssistantReplyStarted { session_id: sid });
        // 7. AssistantReplyTextDelta with first phrase
        assert_eq!(
            events[6],
            Event::AssistantReplyTextDelta {
                session_id: sid,
                delta: "I understand your question. Let me help you with that.".to_owned(),
            }
        );
        // 8. AssistantSpeaking
        assert_eq!(
            events[7],
            Event::SessionStateChanged {
                session_id: sid,
                from: "Thinking".to_owned(),
                to: "AssistantSpeaking".to_owned(),
            }
        );
        // 9. PlaybackCompleted
        assert_eq!(events[8], Event::PlaybackCompleted { session_id: sid });
        // 10. Back to Listening
        assert_eq!(
            events[9],
            Event::SessionStateChanged {
                session_id: sid,
                from: "AssistantSpeaking".to_owned(),
                to: "Listening".to_owned(),
            }
        );
    }

    #[test]
    fn test_simulate_conversation_turn_cycles_phrases() {
        let mut gw = MockGateway::new();
        gw.set_response_phrases(vec!["Alpha".into(), "Beta".into()]);
        let sid = Uuid::now_v7();

        // Turn 0 -> "Alpha", Turn 1 -> "Beta", Turn 2 -> "Alpha" (wraps)
        for expected in &["Alpha", "Beta", "Alpha"] {
            gw.simulate_conversation_turn(sid);
            let events: Vec<Event> =
                std::iter::from_fn(|| gw.poll_event().ok().flatten()).collect();
            let delta = events.iter().find_map(|e| {
                if let Event::AssistantReplyTextDelta { delta, .. } = e {
                    Some(delta.clone())
                } else {
                    None
                }
            });
            assert_eq!(delta.as_deref(), Some(*expected), "phrase mismatch on turn");
        }
    }

    #[test]
    fn test_simulate_interrupt() {
        let mut gw = MockGateway::new();
        let sid = Uuid::now_v7();

        gw.simulate_interrupt(sid);

        let events: Vec<Event> = std::iter::from_fn(|| gw.poll_event().ok().flatten()).collect();

        assert_eq!(events.len(), 3);
        assert_eq!(events[0], Event::PlaybackInterrupted { session_id: sid });
        assert_eq!(
            events[1],
            Event::SessionStateChanged {
                session_id: sid,
                from: "AssistantSpeaking".to_owned(),
                to: "Interrupted".to_owned(),
            }
        );
        assert_eq!(
            events[2],
            Event::SessionStateChanged {
                session_id: sid,
                from: "Interrupted".to_owned(),
                to: "Listening".to_owned(),
            }
        );
    }

    #[test]
    fn test_audio_triggers_conversation() {
        let mut gw = MockGateway::new();
        gw.connect().unwrap();
        let _ = gw.poll_event(); // consume AppReady

        let sid = Uuid::now_v7();
        let sid_str = sid.to_string();
        let data = [0u8; 160];

        // Send exactly 10 frames to trigger a conversation turn.
        for _ in 0..10 {
            gw.send_audio_data(&sid_str, &data).unwrap();
        }

        let events: Vec<Event> = std::iter::from_fn(|| gw.poll_event().ok().flatten()).collect();

        // Should contain AssistantReplyStarted from the triggered conversation turn.
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::AssistantReplyStarted { .. })),
            "expected conversation turn to be triggered after 10 audio frames"
        );

        // The conversation turn sets response_pending = false at the end,
        // so sending another 10 frames should trigger again.
        for _ in 0..10 {
            gw.send_audio_data(&sid_str, &data).unwrap();
        }
        let events2: Vec<Event> = std::iter::from_fn(|| gw.poll_event().ok().flatten()).collect();
        assert!(
            events2
                .iter()
                .any(|e| matches!(e, Event::AssistantReplyStarted { .. })),
            "expected second conversation turn after another 10 frames"
        );
    }

    #[test]
    fn test_custom_response_phrases() {
        let mut gw = MockGateway::new();
        let sid = Uuid::now_v7();

        let custom = vec!["Custom reply one.".into(), "Custom reply two.".into()];
        gw.set_response_phrases(custom);

        gw.simulate_conversation_turn(sid);
        let events: Vec<Event> = std::iter::from_fn(|| gw.poll_event().ok().flatten()).collect();

        let delta = events.iter().find_map(|e| {
            if let Event::AssistantReplyTextDelta { delta, .. } = e {
                Some(delta.clone())
            } else {
                None
            }
        });
        assert_eq!(delta.as_deref(), Some("Custom reply one."));
    }
}
