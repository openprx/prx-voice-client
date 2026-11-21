//! Engine loop: the polling loop on the background runtime that drives the engine, ASR text, playback and UI updates.

use prxvc_audio::audio_loop::{AudioProcessingLoop, LoopConfig, RecognizedUtterance};
use prxvc_audio::endpoint::EndpointConfig;
use prxvc_audio::turn_policy::{TurnAction, TurnPolicy};
use prxvc_core::engine::Engine;
use prxvc_desktop_ui::{EventSnapshot, NetworkStatus, SharedAppState, TranscriptLine};

use crate::asr_session::AudioWiring;
use crate::call_compose::CallControl;
use prxvc_audio::input_supervisor::AudioLoopEvent;
use crate::commands::{CommandContext, process_command};
use crate::compose;
use crate::event_map::{handle_audio_event, handle_engine_event};
use crate::persistence::load_history_from_storage;
use crate::playback::PlaybackDriver;

/// Everything the engine thread owns while the loop runs.
struct EngineLoop {
    engine: Engine,
    shared: SharedAppState,
    storage: Option<prxvc_storage::StorageManager>,
    audio_loop: AudioProcessingLoop,
    audio_wiring: AudioWiring,
    asr_text_rx: std::sync::mpsc::Receiver<RecognizedUtterance>,
    audio_events_rx: std::sync::mpsc::Receiver<AudioLoopEvent>,
    playback: PlaybackDriver,
    call: CallControl,
    loop_config: LoopConfig,
    turn_policy: TurnPolicy,
    /// Cancels sent for a reply that had not started speaking: their
    /// `PlaybackInterrupted` events must not flush playback or close the gate.
    silent_cancels: u32,
}

/// Main engine loop backed by a real [`Engine`] with [`MockGateway`].
///
/// Initializes storage and config, creates the engine, loads history from
/// the database, then enters a polling loop that processes UI commands
/// through the engine and relays transport events back to the UI.
pub(crate) async fn run_engine_loop(
    shared: SharedAppState,
    config: prxvc_config::AppConfig,
    call: CallControl,
) {
    // Initialize storage (graceful degradation if unavailable).
    let storage = compose::initialize_storage();

    // Load persisted history into the UI.
    if let Some(ref storage) = storage {
        load_history_from_storage(storage, &shared);
    }

    // Create Engine with transport selected by config.
    let (mut engine, transport_label) = compose::build_engine(&config);
    let playback = compose::build_playback(&config);

    // Set initial network status based on mode.
    if !config.network.use_mock {
        shared.set_network_status(NetworkStatus::Connecting);
    }

    // Connect transport and poll the initial AppReady event.
    if let Err(e) = engine.initialize() {
        shared.push_event(EventSnapshot {
            event_type: "Error".into(),
            detail: format!("Engine init failed: {e}"),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
        shared.update_session_state("Failed", false);
        if !config.network.use_mock {
            shared.set_network_status(NetworkStatus::Failed);
        }
        return;
    }

    // Update network status after successful initialization.
    shared.set_network_status(NetworkStatus::Connected);

    shared.push_event(EventSnapshot {
        event_type: "AppReady".into(),
        detail: format!("Engine initialized with {transport_label}"),
        timestamp: chrono::Utc::now().to_rfc3339(),
    });

    // Audio processing loop (local Whisper ASR).
    let (asr_text_tx, asr_text_rx) = std::sync::mpsc::channel::<RecognizedUtterance>();
    let (audio_events_tx, audio_events_rx) = std::sync::mpsc::channel::<AudioLoopEvent>();
    let mut state = EngineLoop {
        engine,
        shared,
        storage,
        audio_loop: AudioProcessingLoop::new(),
        audio_wiring: AudioWiring {
            text_tx: asr_text_tx,
            events_tx: audio_events_tx,
            input_device: config.audio.input_device.clone(),
        },
        asr_text_rx,
        audio_events_rx,
        playback,
        call,
        loop_config: LoopConfig {
            endpoint: EndpointConfig::from_end_silence_ms(config.audio.end_silence_ms),
        },
        turn_policy: compose::build_turn_policy(&config),
        silent_cancels: 0,
    };

    // Main polling loop.
    loop {
        state.drain_commands();
        state.release_assistant_when_idle();
        state.relay_audio_events();
        state.relay_utterances();

        // 3. Play assistant speech the server has sent (honours auto_play).
        state.playback.play_pending(&mut state.engine, std::time::Instant::now());

        state.poll_transport();
        state.tick_playback();
        state.publish_session_state();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

impl EngineLoop {
    /// 1. Drain UI commands and process them through the engine.
    fn drain_commands(&mut self) {
        // "End assistant and start" ends the assistant session like the end button does; the call
        // starts when the session has let go of the microphone (`release_assistant_when_idle`).
        let commands: Vec<String> = self
            .shared
            .take_commands()
            .into_iter()
            .map(|c| {
                if c == prxvc_desktop_ui::call_view::command::END_ASSISTANT_AND_START {
                    self.call.arm_start_after_assistant();
                    "StopSession".to_owned()
                } else {
                    c
                }
            })
            .collect();
        if commands.iter().any(|c| c == "StopSession") {
            self.turn_policy.reset();
        }
        for cmd in &commands {
            // The `Call*` group belongs to call mode and never reaches the assistant's handler.
            if self.call.handle_command(cmd) {
                continue;
            }
            // An assistant session and call mode never share the microphone path.
            if cmd == "StartSession"
                && let Err(message) = self.call.claim_assistant()
            {
                crate::event_map::push_error(&self.shared, &message);
                continue;
            }
            let mut ctx = CommandContext {
                engine: &mut self.engine,
                shared: &self.shared,
                storage: &self.storage,
                audio_loop: &mut self.audio_loop,
                audio_wiring: &self.audio_wiring,
                playback: &mut self.playback,
                loop_config: self.loop_config,
            };
            process_command(&mut ctx, cmd);
        }
    }

    /// Give the microphone back to call mode once no assistant session or capture runs.
    fn release_assistant_when_idle(&mut self) {
        let session_live = self.engine.active_session().is_some_and(|s| {
            s.state != prxvc_session::SessionState::Completed
                && s.state != prxvc_session::SessionState::Failed
        });
        if !session_live && !self.audio_loop.is_running() {
            self.call.release_assistant();
        }
    }

    /// 1b. Report microphone changes (device chosen, switched, lost) to the UI.
    fn relay_audio_events(&mut self) {
        while let Ok(event) = self.audio_events_rx.try_recv() {
            handle_audio_event(&event, &self.shared);
        }
    }

    /// 2. Check for locally recognized speech and send to server.
    fn relay_utterances(&mut self) {
        while let Ok(utterance) = self.asr_text_rx.try_recv() {
            if let Some(session) = self.engine.active_session() {
                let sid = session.id.0;
                // Speaking over the assistant (echo control off) cuts it off.
                let barge_in = session.state == prxvc_session::SessionState::AssistantSpeaking;
                if barge_in {
                    // Past the point where an utterance continues the last turn.
                    self.turn_policy.assistant_started();
                }
                let action = self.turn_policy.on_utterance(
                    std::time::Instant::now(),
                    &utterance.text,
                    utterance.speech,
                );
                let TurnAction::Send { text, cancel_previous } = action else {
                    tracing::info!(
                        text_chars = utterance.text.chars().count(),
                        speech_ms = utterance.speech.as_millis() as u64,
                        "dropped utterance without enough content"
                    );
                    continue;
                };
                if cancel_previous {
                    // One sentence split by a pause: replace the turn whose
                    // reply is still being prepared with the merged text.
                    match self.engine.cancel_pending_reply() {
                        Ok(true) => self.silent_cancels += 1,
                        Ok(false) => {}
                        Err(e) => tracing::warn!("cancelling the pending reply failed: {e}"),
                    }
                    self.shared.replace_last_user_line(text.clone());
                } else {
                    self.shared.add_transcript_line(TranscriptLine {
                        role: "user".into(),
                        text: text.clone(),
                        is_final: true,
                    });
                }
                tracing::info!(text_chars = text.chars().count(), merged = cancel_previous, "sending local ASR text to server");
                tracing::debug!(%text, "text sent to the server");
                let cmd = prxvc_protocol::Command::SendText { session_id: sid, text };
                if let Err(e) = self.engine.process_command(cmd) {
                    tracing::warn!("SendText failed: {e}");
                }
                if barge_in {
                    self.playback.flush(&mut self.engine, std::time::Instant::now());
                }
            }
        }
    }

    /// 4. Poll transport events from the engine and update the UI.
    fn poll_transport(&mut self) {
        match self.engine.poll_transport_events() {
            Ok(events) => {
                for event in &events {
                    handle_engine_event(event, &self.shared);
                    let now = std::time::Instant::now();
                    match event {
                        prxvc_protocol::Event::PlaybackInterrupted { .. } => {
                            if self.silent_cancels > 0 {
                                self.silent_cancels -= 1;
                            } else {
                                self.turn_policy.assistant_started();
                                self.playback.flush(&mut self.engine, now);
                            }
                        }
                        prxvc_protocol::Event::AssistantReplyStarted { .. } => {
                            self.turn_policy.assistant_started();
                            self.playback.on_reply_started(&self.engine, now);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                tracing::warn!("transport poll error: {e}");
            }
        }

        // A cancel's event is produced synchronously, so none can be pending.
        self.silent_cancels = 0;
    }

    /// 4b. Finish a reply whose end the server announced once the output
    /// has drained, and keep the echo gate (and its UI flag) current.
    fn tick_playback(&mut self) {
        let now = std::time::Instant::now();
        self.playback.tick(&mut self.engine, now);
        self.shared.set_mic_paused(self.playback.mic_paused(&self.engine, now));
    }

    /// 5. Update debug state info from the active session.
    fn publish_session_state(&self) {
        if let Some(session) = self.engine.active_session() {
            let state_str = format!("{:?}", session.state);
            let is_active = session.state != prxvc_session::SessionState::Completed
                && session.state != prxvc_session::SessionState::Failed;
            self.shared.update_session_state(&state_str, is_active);
        }
    }
}
