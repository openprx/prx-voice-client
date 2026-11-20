//! PRX Voice Client desktop application.
//!
//! Runs the egui UI on the main thread and a tokio async runtime on a
//! background thread. The two communicate through [`SharedAppState`] which
//! uses `parking_lot::Mutex` for minimal-contention synchronization.

mod hotkey;
mod playback;
mod settings;
mod tray;

use std::thread::JoinHandle;

use eframe::egui;
use prxvc_audio::audio_loop::{AudioProcessingLoop, LoopConfig, RecognizedUtterance};
use prxvc_audio::echo_gate::{DEFAULT_HOLD_OFF, EchoGate, EchoGateHandle};
use prxvc_audio::endpoint::EndpointConfig;
use prxvc_audio::playback::AssistantPlayer;
use prxvc_audio::turn_policy::{TurnAction, TurnPolicy, TurnPolicyConfig};
use prxvc_desktop_ui::tool_window::{Geometry, ToolKind};
use prxvc_desktop_ui::{
    EventSnapshot, HistoryEntry, NetworkStatus, SharedAppState, Theme, TranscriptLine,
};

// ---------------------------------------------------------------------------
// Application struct
// ---------------------------------------------------------------------------

/// Top-level application wiring egui to the async runtime.
struct PrxVoiceApp {
    /// Mutable UI state consumed by the render functions.
    ui_state: prxvc_desktop_ui::DesktopUiState,
    /// Shared state bridging the UI thread and the tokio runtime.
    shared: SharedAppState,
    /// Handle to the background runtime thread (kept alive for the app lifetime).
    _runtime_handle: Option<JoinHandle<()>>,
    /// Global hotkey manager (optional — degrades gracefully).
    hotkey_manager: Option<hotkey::HotkeyManager>,
    /// System tray manager (optional — degrades gracefully).
    tray_manager: Option<tray::TrayManager>,
    /// Last observed i18n locale, used to detect language changes reliably.
    last_locale: String,
    /// The configuration in effect (loaded at startup, the one the engine is
    /// built from). Settings saves start from it and change only what the
    /// popup edits.
    config: prxvc_config::AppConfig,
    /// Persisted subset of the settings last written (or loaded).
    last_saved: settings::SettingsSnapshot,
}

impl PrxVoiceApp {
    /// Bootstrap the application: create shared state and spawn the runtime.
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Embedded UI fonts (Hanken Grotesk, JetBrains Mono, Noto Sans SC
        // subset). A system CJK font is the fallback after the embedded
        // Chinese face, for characters outside the embedded subset.
        let cjk_paths = [
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "C:\\Windows\\Fonts\\msyh.ttc",
        ];
        let system_cjk = cjk_paths.iter().find_map(|path| {
            let data = std::fs::read(path).ok()?;
            tracing::info!("System CJK fallback font loaded from {}", path);
            Some(data)
        });
        if system_cjk.is_none() {
            tracing::warn!("No system CJK font found; only the embedded subset covers Chinese");
        }
        prxvc_desktop_ui::fonts::install(&cc.egui_ctx, system_cjk);

        // The RF theme is the default: install its light and dark styles and
        // follow the system preference. The central panel draws with an
        // explicit transparent frame, so no panel fill override is needed.
        prxvc_desktop_ui::theme::install(&cc.egui_ctx);

        // Initialize i18n with default locale
        prxvc_i18n::set_locale(prxvc_i18n::Locale::En);

        // One configuration load serves both the engine and the settings popup.
        let config = initialize_config();
        let mut ui_state = prxvc_desktop_ui::DesktopUiState::new();
        ui_state.settings = settings::ui_settings_from_config(&config);
        let last_saved = settings::snapshot(&ui_state.settings);

        // Remembered position and size of the settings/history/debug windows
        // (eframe persists only the main window).
        if let Some(storage) = cc.storage {
            for kind in ToolKind::ALL {
                let saved = eframe::get_value::<(f32, f32, f32, f32)>(storage, kind.storage_key())
                    .and_then(Geometry::from_tuple);
                if let Some(geometry) = saved {
                    ui_state.windows.get_mut(kind).restore(geometry);
                }
            }
        }
        let initial_locale = if config.ui.language == "zh-CN" {
            prxvc_i18n::set_locale(prxvc_i18n::Locale::ZhCn);
            "zh-CN"
        } else {
            "en"
        };

        let shared = SharedAppState::new();
        let shared_clone = shared.clone();
        let engine_config = config.clone();

        let handle = std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("Failed to create tokio runtime: {}", e);
                    return;
                }
            };
            rt.block_on(run_engine_loop(shared_clone, engine_config));
        });

        let hotkey_manager = match hotkey::HotkeyManager::new(cc.egui_ctx.clone()) {
            Ok(m) => {
                tracing::info!("Global hotkey registered: Ctrl+Shift+Space");
                Some(m)
            }
            Err(e) => {
                tracing::warn!("Failed to register hotkey: {}", e);
                None
            }
        };

        let tray_manager = match tray::TrayManager::new(cc.egui_ctx.clone()) {
            Ok(t) => {
                tracing::info!("System tray created");
                Some(t)
            }
            Err(e) => {
                tracing::warn!("Failed to create tray: {}", e);
                None
            }
        };

        Self {
            ui_state,
            shared,
            _runtime_handle: Some(handle),
            hotkey_manager,
            tray_manager,
            last_locale: initial_locale.to_owned(),
            config,
            last_saved,
        }
    }
}

impl eframe::App for PrxVoiceApp {
    /// Tell the glow renderer to clear with fully transparent color.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        for kind in ToolKind::ALL {
            if let Some(geometry) = self.ui_state.windows.get(kind).geometry() {
                eframe::set_value(storage, kind.storage_key(), &geometry.to_tuple());
            }
        }
    }

    /// Non-drawing work. eframe calls this before every `ui` pass and also
    /// while the window is hidden when a repaint was requested (the hotkey
    /// and tray handlers request one).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Sync state from async runtime
        self.ui_state.sync_from_shared(&self.shared);

        // Check hotkey
        if let Some(ref mut hk) = self.hotkey_manager {
            if hk.was_triggered() {
                if !self.ui_state.session_active {
                    self.shared.push_command("CreateSession");
                    self.shared.push_command("StartSession");
                } else {
                    self.shared.push_command("StopSession");
                }
            }
        }

        // Check tray actions
        if let Some(ref tray) = self.tray_manager {
            if let Some(action) = tray.poll_action() {
                match action {
                    tray::TrayAction::StartSession => {
                        self.shared.push_command("CreateSession");
                        self.shared.push_command("StartSession");
                        // Optimistic update so orb responds immediately
                        self.shared.update_session_state("Listening", true);
                    }
                    tray::TrayAction::StopSession => {
                        self.shared.push_command("StopSession");
                        // Optimistic update so orb responds immediately
                        self.shared.update_session_state("Idle", false);
                        self.shared.clear_transcript();
                    }
                    tray::TrayAction::Settings => {
                        self.ui_state
                            .windows
                            .request_open(ToolKind::Settings);
                    }
                    tray::TrayAction::Quit => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    tray::TrayAction::Show => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            }
        }

    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Render the entire UI
        prxvc_desktop_ui::render_app(ui, &mut self.ui_state);

        // Apply pending theme change (if settings changed the theme)
        if let Some(theme_name) = self.ui_state.pending_theme.take() {
            if let Some(new_theme) = Theme::all()
                .into_iter()
                .find(|t| t.name == theme_name)
            {
                // The new theme's style is applied by the UI on its next frame.
                self.ui_state.theme = new_theme;
            }
        }

        // Refresh tray menu labels when locale changes
        let current_locale = prxvc_i18n::current_locale().as_str().to_owned();
        if current_locale != self.last_locale {
            self.last_locale = current_locale;
            if let Some(ref tray) = self.tray_manager {
                tray.refresh_labels();
            }
        }

        // Persist settings to config.toml when they change
        save_settings_to_config(&mut self.config, &mut self.last_saved, &self.ui_state.settings);

        // Flush commands to async runtime
        self.ui_state.flush_commands_to_shared(&self.shared);
    }
}

// ---------------------------------------------------------------------------
// Async engine loop (runs on the background tokio runtime)
// ---------------------------------------------------------------------------

/// Main engine loop backed by a real [`Engine`] with [`MockGateway`].
///
/// Initializes storage and config, creates the engine, loads history from
/// the database, then enters a polling loop that processes UI commands
/// through the engine and relays transport events back to the UI.
async fn run_engine_loop(shared: SharedAppState, config: prxvc_config::AppConfig) {
    // Initialize storage (graceful degradation if unavailable).
    let storage = initialize_storage();

    // Load persisted history into the UI.
    if let Some(ref storage) = storage {
        load_history_from_storage(storage, &shared);
    }

    // Create Engine with transport selected by config.
    let (event_tx, _event_rx) = prxvc_core::bus::event_channel(256);

    let transport: Box<dyn prxvc_protocol::transport::Transport> = if config.network.use_mock {
        tracing::info!("Using MockGateway (offline mode)");
        Box::new(prxvc_protocol::mock::MockGateway::new())
    } else {
        tracing::info!(
            "Connecting to prx-voice server: {} (asr={}, agent={}, tts={}, language={})",
            config.network.server_url,
            config.network.asr_provider,
            config.network.agent_provider,
            config.network.tts_provider,
            config.network.language,
        );
        let prx_config = prxvc_protocol::prx_voice_transport::PrxVoiceConfig {
            base_url: config.network.server_url.clone(),
            language: config.network.language.clone(),
            asr_provider: config.network.asr_provider.clone(),
            agent_provider: config.network.agent_provider.clone(),
            tts_provider: config.network.tts_provider.clone(),
            channel: "app_sdk".into(),
            client_id: uuid::Uuid::new_v4().to_string(),
        };
        Box::new(prxvc_protocol::prx_voice_transport::PrxVoiceTransport::new(prx_config))
    };

    let transport_label = if config.network.use_mock { "MockGateway" } else { "PrxVoiceTransport" };
    let mut engine = prxvc_core::engine::Engine::new(transport, event_tx);
    engine.set_auto_play_reply(config.audio.auto_play_reply);
    // This loop plays the reply audio: the end of a reply returns the session
    // to listening only once the output has drained.
    engine.set_external_playback(true);

    // Assistant speech goes to the default output device, opened on first use.
    // The echo gate keeps the microphone away from the ASR while it plays
    // (half duplex), unless `echo_control` is off (headphones).
    let player = AssistantPlayer::new(Box::new(|| {
        Ok(Box::new(prxvc_audio::cpal_output::CpalAudioOutput::new()?)
            as Box<dyn prxvc_audio::pipeline::AudioOutput>)
    }));
    let gate = EchoGateHandle::new(EchoGate::new(config.audio.echo_control, DEFAULT_HOLD_OFF));
    let mut playback = playback::PlaybackDriver::new(player, gate);

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
    let mut audio_loop = AudioProcessingLoop::new();
    let (asr_text_tx, asr_text_rx) = std::sync::mpsc::channel::<RecognizedUtterance>();
    let loop_config = LoopConfig {
        endpoint: EndpointConfig::from_end_silence_ms(config.audio.end_silence_ms),
    };
    let mut turn_policy = TurnPolicy::new(TurnPolicyConfig::from_ms(
        config.audio.merge_window_ms,
        config.audio.min_speech_ms,
    ));
    // Cancels sent for a reply that had not started speaking: their
    // `PlaybackInterrupted` events must not flush playback or close the gate.
    let mut silent_cancels: u32 = 0;

    // Main polling loop.
    loop {
        // 1. Drain UI commands and process them through the engine.
        let commands = shared.take_commands();
        if commands.iter().any(|c| c == "StopSession") {
            turn_policy.reset();
        }
        for cmd in &commands {
            process_command(
                &mut engine,
                cmd,
                &shared,
                &storage,
                &mut audio_loop,
                &asr_text_tx,
                &mut playback,
                loop_config,
            );
        }

        // 2. Check for locally recognized speech and send to server.
        while let Ok(utterance) = asr_text_rx.try_recv() {
            if let Some(session) = engine.active_session() {
                let sid = session.id.0;
                // Speaking over the assistant (echo control off) cuts it off.
                let barge_in = session.state == prxvc_session::SessionState::AssistantSpeaking;
                if barge_in {
                    // Past the point where an utterance continues the last turn.
                    turn_policy.assistant_started();
                }
                let action = turn_policy.on_utterance(
                    std::time::Instant::now(),
                    &utterance.text,
                    utterance.speech,
                );
                let TurnAction::Send { text, cancel_previous } = action else {
                    tracing::info!(
                        text = %utterance.text,
                        speech_ms = utterance.speech.as_millis() as u64,
                        "dropped utterance without enough content"
                    );
                    continue;
                };
                if cancel_previous {
                    // One sentence split by a pause: replace the turn whose
                    // reply is still being prepared with the merged text.
                    match engine.cancel_pending_reply() {
                        Ok(true) => silent_cancels += 1,
                        Ok(false) => {}
                        Err(e) => tracing::warn!("cancelling the pending reply failed: {e}"),
                    }
                    shared.replace_last_user_line(text.clone());
                } else {
                    shared.add_transcript_line(TranscriptLine {
                        role: "user".into(),
                        text: text.clone(),
                        is_final: true,
                    });
                }
                tracing::info!(%text, merged = cancel_previous, "sending local ASR text to server");
                let cmd = prxvc_protocol::Command::SendText { session_id: sid, text };
                if let Err(e) = engine.process_command(cmd) {
                    tracing::warn!("SendText failed: {e}");
                }
                if barge_in {
                    playback.flush(&mut engine, std::time::Instant::now());
                }
            }
        }

        // 3. Play assistant speech the server has sent (honours auto_play).
        playback.play_pending(&mut engine, std::time::Instant::now());

        // 4. Poll transport events from the engine and update the UI.
        match engine.poll_transport_events() {
            Ok(events) => {
                for event in &events {
                    handle_engine_event(event, &shared);
                    let now = std::time::Instant::now();
                    match event {
                        prxvc_protocol::Event::PlaybackInterrupted { .. } => {
                            if silent_cancels > 0 {
                                silent_cancels -= 1;
                            } else {
                                turn_policy.assistant_started();
                                playback.flush(&mut engine, now);
                            }
                        }
                        prxvc_protocol::Event::AssistantReplyStarted { .. } => {
                            turn_policy.assistant_started();
                            playback.on_reply_started(&engine, now);
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
        silent_cancels = 0;

        // 4b. Finish a reply whose end the server announced once the output
        // has drained, and keep the echo gate (and its UI flag) current.
        let now = std::time::Instant::now();
        playback.tick(&mut engine, now);
        shared.set_mic_paused(playback.mic_paused(&engine, now));

        // 5. Update debug state info from the active session.
        if let Some(session) = engine.active_session() {
            let state_str = format!("{:?}", session.state);
            let is_active = session.state != prxvc_session::SessionState::Completed
                && session.state != prxvc_session::SessionState::Failed;
            shared.update_session_state(&state_str, is_active);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// Command processing
// ---------------------------------------------------------------------------

/// Map a UI command string to the appropriate [`Command`] and dispatch it
/// through the engine. Handles session creation/start chaining and
/// post-stop persistence.
#[allow(clippy::too_many_arguments)] // the engine thread's collaborators, passed explicitly
fn process_command(
    engine: &mut prxvc_core::engine::Engine,
    cmd: &str,
    shared: &SharedAppState,
    storage: &Option<prxvc_storage::StorageManager>,
    audio_loop: &mut AudioProcessingLoop,
    asr_text_tx: &std::sync::mpsc::Sender<RecognizedUtterance>,
    playback: &mut playback::PlaybackDriver,
    loop_config: LoopConfig,
) {
    use prxvc_protocol::Command;

    tracing::info!("process_command: {}", cmd);

    let command = match cmd {
        "CreateSession" => Command::CreateSession,
        "StartSession" => {
            // If there is already an active session, start it.
            // Otherwise create one first, then start it.
            if let Some(session) = engine.active_session() {
                Command::StartSession {
                    session_id: session.id.0,
                }
            } else {
                if let Err(e) = engine.process_command(Command::CreateSession) {
                    push_error(shared, &format!("Create session: {e}"));
                    return;
                }
                if let Some(session) = engine.active_session() {
                    let sid = session.id.0;
                    shared.set_session_id(Some(sid.to_string()));
                    Command::StartSession { session_id: sid }
                } else {
                    push_error(shared, "No active session after create");
                    return;
                }
            }
        }
        "StopSession" => {
            if let Some(session) = engine.active_session() {
                Command::StopSession {
                    session_id: session.id.0,
                }
            } else {
                tracing::debug!("StopSession ignored: no active session");
                return;
            }
        }
        "InterruptPlayback" => {
            if let Some(session) = engine.active_session() {
                Command::InterruptPlayback {
                    session_id: session.id.0,
                }
            } else {
                tracing::debug!("InterruptPlayback: no active session, flushing local playback");
                playback.flush(engine, std::time::Instant::now());
                return;
            }
        }
        "RefreshDeviceList" => Command::RefreshDeviceList,
        "RetryConnection" => {
            shared.clear_error();
            shared.set_network_status(NetworkStatus::Reconnecting);
            // Re-initialize the engine transport to attempt reconnection.
            if let Err(e) = engine.initialize() {
                shared.set_error(&format!("Reconnect failed: {e}"), true);
                shared.set_network_status(NetworkStatus::Failed);
            } else {
                shared.set_network_status(NetworkStatus::Connected);
            }
            return;
        }
        "ExportDiagnostics" => {
            export_diagnostics(shared);
            return;
        }
        _ => {
            tracing::debug!("unknown command: {cmd}");
            return;
        }
    };

    // Capture session id before the command mutates state (for StopSession).
    let is_stop = cmd == "StopSession";
    let pre_session_id = engine.active_session().map(|s| s.id);

    // Stop audio capture before StopSession.
    if is_stop && audio_loop.is_running() {
        audio_loop.stop();
    }

    let result = engine.process_command(command);

    // Interrupt and stop silence the assistant locally whatever the engine
    // decided: it may consider the reply finished while queued audio plays on.
    if cmd == "InterruptPlayback" || is_stop {
        playback.flush(engine, std::time::Instant::now());
    }

    match result {
        Ok(()) => {
            // After CreateSession, propagate the session id to the UI.
            if cmd == "CreateSession" {
                if let Some(session) = engine.active_session() {
                    shared.set_session_id(Some(session.id.0.to_string()));
                }
            }

            // After StartSession, start the local audio ASR loop.
            if cmd == "StartSession" {
                start_audio_loop(engine, audio_loop, asr_text_tx, shared, playback.gate(), loop_config);
            }

            // After StopSession, persist the completed session and reset UI.
            if is_stop {
                if let (Some(storage), Some(session_id)) = (storage, pre_session_id) {
                    persist_session(engine, &session_id, storage, shared);
                }
                shared.clear_transcript();
                shared.update_session_state("Idle", false);
                shared.set_session_id(None);
            }
        }
        Err(e) => {
            push_error(shared, &format!("Command '{cmd}' failed: {e}"));
        }
    }
}

/// Start the local audio capture + Whisper ASR loop for the active session.
fn start_audio_loop(
    engine: &prxvc_core::engine::Engine,
    audio_loop: &mut AudioProcessingLoop,
    asr_text_tx: &std::sync::mpsc::Sender<RecognizedUtterance>,
    shared: &SharedAppState,
    gate: EchoGateHandle,
    loop_config: LoopConfig,
) {
    {
        let Some(session) = engine.active_session() else {
            return;
        };
        let session_id = session.id.0;

        // Model paths — look in standard locations.
        let models_dir = app_paths()
            .map(|p| p.models_dir.clone())
            .unwrap_or_else(|| std::path::PathBuf::from("models"));

        let model_path = models_dir.join("ggml-small.bin");
        let vad_model_path = models_dir.join("ggml-silero-vad.bin");

        if !model_path.exists() || !vad_model_path.exists() {
            tracing::warn!(
                model = %model_path.display(),
                vad = %vad_model_path.display(),
                "Whisper model files not found — audio ASR disabled. \
                 Download models to the paths above."
            );
            return;
        }

        let asr_config = prxvc_audio::whisper_asr::WhisperAsrConfig {
            model_path: model_path.to_string_lossy().into(),
            vad_model_path: vad_model_path.to_string_lossy().into(),
            language: "zh".into(),
            n_threads: 4,
            ..Default::default()
        };

        if let Err(e) = audio_loop.start(asr_config, session_id, asr_text_tx.clone(), gate, loop_config) {
            tracing::warn!(error = %e, "failed to start audio loop");
            push_error(shared, &format!("Audio: {e}"));
        }
    }

}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

/// Map an [`Event`] from the engine to [`SharedAppState`] updates so the
/// egui UI can reflect the latest state.
fn handle_engine_event(event: &prxvc_protocol::Event, shared: &SharedAppState) {
    use prxvc_protocol::Event;

    // Build a concise event-type label from the variant name.
    let debug_repr = format!("{event:?}");
    let event_type = debug_repr
        .split('(')
        .next()
        .unwrap_or("Unknown")
        .split('{')
        .next()
        .unwrap_or("Unknown")
        .trim()
        .to_owned();

    shared.push_event(EventSnapshot {
        event_type,
        detail: debug_repr,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });

    match event {
        Event::SessionStateChanged { to, .. } => {
            let active = to != "Completed" && to != "Failed";
            shared.update_session_state(to, active);
        }
        Event::InterimTranscriptProduced { text, .. } => {
            shared.add_transcript_line(TranscriptLine {
                role: "user".into(),
                text: text.clone(),
                is_final: false,
            });
        }
        Event::FinalTranscriptProduced { text, .. } => {
            shared.add_transcript_line(TranscriptLine {
                role: "user".into(),
                text: text.clone(),
                is_final: true,
            });
        }
        Event::AssistantReplyTextDelta { delta, .. } => {
            shared.add_transcript_line(TranscriptLine {
                role: "assistant".into(),
                text: delta.clone(),
                is_final: true,
            });
        }
        Event::DeviceListUpdated { devices } => {
            tracing::info!("devices updated: {} devices", devices.len());
        }
        Event::ErrorOccurred {
            code,
            message,
            recoverable,
        } => {
            shared.set_error(message, *recoverable);
            if code.contains("reconnect") {
                shared.set_network_status(NetworkStatus::Reconnecting);
            }
        }
        Event::DiagnosticsExported { path } => {
            shared.push_event(EventSnapshot {
                event_type: "DiagnosticsExported".into(),
                detail: format!("Exported to: {path}"),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Initialization helpers
// ---------------------------------------------------------------------------

/// Attempt to open a SQLite-backed [`StorageManager`].
///
/// Returns `None` (with a warning log) if the platform data directory cannot
/// be resolved or the database cannot be opened, so the app can continue
/// without persistence.
fn initialize_storage() -> Option<prxvc_storage::StorageManager> {
    let db_path = app_paths()?.data_dir.join("sessions.db");
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match prxvc_storage::StorageManager::new(db_path) {
        Ok(s) => {
            tracing::info!("Storage initialized");
            Some(s)
        }
        Err(e) => {
            tracing::warn!("Storage init failed: {e}");
            None
        }
    }
}

/// Directories resolved once at start-up by [`initialize_config`].
static APP_PATHS: std::sync::OnceLock<prxvc_config::paths::AppPaths> = std::sync::OnceLock::new();

/// The resolved application directories (`None` before [`initialize_config`]
/// ran or when the platform has no home directory).
fn app_paths() -> Option<&'static prxvc_config::paths::AppPaths> {
    APP_PATHS.get()
}

/// Log what a legacy directory migration did.
fn log_migration(what: &str, report: &prxvc_config::legacy::MigrationReport) {
    if report.is_noop() {
        return;
    }
    tracing::info!(
        "{what}: copied {}, moved {}, removed {} empty folder(s)",
        report.copied.len(),
        report.moved.len(),
        report.removed_dirs.len()
    );
    for (path, reason) in &report.failures {
        tracing::warn!("{what}: could not migrate {}: {reason}", path.display());
    }
}

/// Migrate the legacy directories, load [`AppConfig`] and resolve the
/// application directories.
///
/// A settings file that cannot be read is kept under another name and
/// defaults are used (the user-facing notice is a later work package; for
/// now it is logged). Falls back to defaults if the directory cannot be
/// resolved.
fn initialize_config() -> prxvc_config::AppConfig {
    use prxvc_config::{legacy, paths::AppPaths};
    let legacy_dirs = legacy::LegacyDirs::resolve().ok();
    let default_paths = AppPaths::resolve(&prxvc_config::AppConfig::default()).ok();
    if let (Some(l), Some(p)) = (&legacy_dirs, &default_paths) {
        log_migration(
            "config and database migration",
            &legacy::migrate_legacy_config_and_database(&l.config_dir, &l.data_dir, &p.config_dir, &p.data_dir),
        );
    }
    let config = match prxvc_config::AppConfigManager::with_default_dir() {
        Ok(mgr) => mgr.load().unwrap_or_else(|e| {
            tracing::error!("Failed to load config: {e}");
            prxvc_config::AppConfig::default()
        }),
        Err(_) => prxvc_config::AppConfig::default(),
    };
    if let Ok(paths) = AppPaths::resolve(&config) {
        if let Err(e) = paths.ensure_dirs() {
            tracing::warn!("Failed to create application directories: {e}");
        }
        if let Some(l) = &legacy_dirs {
            log_migration(
                "model files migration",
                &legacy::migrate_legacy_models(&l.models_dir, &paths.models_dir),
            );
        }
        let _ = APP_PATHS.set(paths);
    }
    config
}

/// Persist UI settings to config.toml (debounced: only writes when a persisted
/// value differs from the last write).
///
/// Starts from the configuration in effect and changes only the fields the
/// popup edits, so provider names and other hidden fields are preserved.
fn save_settings_to_config(
    config: &mut prxvc_config::AppConfig,
    last_saved: &mut settings::SettingsSnapshot,
    ui: &prxvc_desktop_ui::UiSettings,
) {
    let current = settings::snapshot(ui);
    if *last_saved == current {
        return;
    }
    settings::apply_ui_settings(config, ui);
    if let Ok(mgr) = prxvc_config::AppConfigManager::with_default_dir() {
        if let Err(e) = mgr.save(config) {
            tracing::warn!("Failed to save config: {}", e);
        } else {
            tracing::debug!("Config saved to disk");
        }
    }
    *last_saved = current;
}

/// Populate the UI history list from persisted session records.
fn load_history_from_storage(storage: &prxvc_storage::StorageManager, shared: &SharedAppState) {
    match storage.recent_sessions(20) {
        Ok(records) => {
            for record in &records {
                shared.add_history_entry(HistoryEntry {
                    session_id: record.id.clone(),
                    title: record.summary_title(),
                    created_at: record.created_at.clone(),
                    duration: record.duration_display(),
                    state: record.state.clone(),
                });
            }
            tracing::info!("Loaded {} history entries from storage", records.len());
        }
        Err(e) => {
            tracing::warn!("Failed to load history: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

/// Persist a session to storage after it has been stopped.
///
/// Looks up the session by id in the session manager (it is still there
/// even though the engine cleared `active_session`).
fn persist_session(
    engine: &prxvc_core::engine::Engine,
    session_id: &prxvc_session::SessionId,
    storage: &prxvc_storage::StorageManager,
    shared: &SharedAppState,
) {
    if let Some(session) = engine.session_manager().get_session(session_id) {
        match storage.save_session(session, None) {
            Ok(()) => {
                tracing::info!("Session persisted: {}", session_id.0);
                // Also add to the UI history.
                if let Ok(record) = prxvc_storage::SessionRecord::from_session(session) {
                    shared.add_history_entry(HistoryEntry {
                        session_id: record.id.clone(),
                        title: record.summary_title(),
                        created_at: record.created_at.clone(),
                        duration: record.duration_display(),
                        state: record.state.clone(),
                    });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to persist session: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics export
// ---------------------------------------------------------------------------

/// Build and export a diagnostics JSON bundle to the system temp directory.
fn export_diagnostics(shared: &SharedAppState) {
    let bundle = prxvc_observe::DiagnosticsBundle {
        app_version: "0.1.0".into(),
        os_info: std::env::consts::OS.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        log_excerpt: Vec::new(),
        config_snapshot: None,
        error_summary: Vec::new(),
        metrics: Some(prxvc_observe::metrics::snapshot()),
        transport_state: None,
        active_session_state: None,
        system_info: Some(prxvc_observe::SystemInfo::default()),
    };

    let export_dir = std::env::temp_dir().join("prx-voice-client");
    let _ = std::fs::create_dir_all(&export_dir);
    let export_path = export_dir.join(format!(
        "diagnostics-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));

    match prxvc_observe::export_diagnostics(&bundle, &export_path) {
        Ok(path) => {
            shared.push_event(EventSnapshot {
                event_type: "DiagnosticsExported".into(),
                detail: format!("Exported to: {}", path.display()),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        Err(e) => {
            push_error(shared, &format!("Export failed: {e}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Push an error event to the shared state for display in the UI.
fn push_error(shared: &SharedAppState, msg: &str) {
    shared.push_event(EventSnapshot {
        event_type: "Error".into(),
        detail: msg.to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_inner_size([220.0, 320.0])
            .with_min_inner_size([180.0, 260.0])
            .with_title("PRX Voice"),
        ..Default::default()
    };

    eframe::run_native(
        "PRX Voice",
        options,
        Box::new(|cc| Ok(Box::new(PrxVoiceApp::new(cc)))),
    )
}
