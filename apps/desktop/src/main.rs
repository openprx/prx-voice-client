//! PRX Voice Client desktop application.
//!
//! Runs the egui UI on the main thread and a tokio async runtime on a
//! background thread. The two communicate through [`SharedAppState`] which
//! uses `parking_lot::Mutex` for minimal-contention synchronization.

mod hotkey;
mod tray;

use std::thread::JoinHandle;

use eframe::egui;
use prxvc_audio::audio_loop::{AudioProcessingLoop, RecognizedUtterance};
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
}

impl PrxVoiceApp {
    /// Bootstrap the application: create shared state and spawn the runtime.
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load CJK font at runtime for Chinese text support
        let mut fonts = egui::FontDefinitions::default();
        let cjk_paths = [
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "C:\\Windows\\Fonts\\msyh.ttc",
        ];
        let mut cjk_loaded = false;
        for path in &cjk_paths {
            if let Ok(data) = std::fs::read(path) {
                fonts.font_data.insert(
                    "cjk".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(data)),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("cjk".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("cjk".to_owned());
                cjk_loaded = true;
                tracing::info!("CJK font loaded from {}", path);
                break;
            }
        }
        if !cjk_loaded {
            tracing::warn!("No CJK font found — Chinese text may not render correctly");
        }
        cc.egui_ctx.set_fonts(fonts);

        // Set dark visuals with transparent panel for frameless orb mode
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::TRANSPARENT;
        visuals.window_fill = egui::Color32::from_rgb(19, 17, 26); // BG_WARM for popup windows
        cc.egui_ctx.set_visuals(visuals);

        // Initialize i18n with default locale
        prxvc_i18n::set_locale(prxvc_i18n::Locale::En);

        let shared = SharedAppState::new();
        let shared_clone = shared.clone();

        let handle = std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("Failed to create tokio runtime: {}", e);
                    return;
                }
            };
            rt.block_on(run_engine_loop(shared_clone));
        });

        let hotkey_manager = match hotkey::HotkeyManager::new() {
            Ok(m) => {
                tracing::info!("Global hotkey registered: Ctrl+Shift+Space");
                Some(m)
            }
            Err(e) => {
                tracing::warn!("Failed to register hotkey: {}", e);
                None
            }
        };

        let tray_manager = match tray::TrayManager::new() {
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
            ui_state: prxvc_desktop_ui::DesktopUiState::new(),
            shared,
            _runtime_handle: Some(handle),
            hotkey_manager,
            tray_manager,
            last_locale: "en".to_owned(),
        }
    }
}

impl eframe::App for PrxVoiceApp {
    /// Tell the glow renderer to clear with fully transparent color.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Sync state from async runtime
        self.ui_state.sync_from_shared(&self.shared);

        // Check hotkey
        if let Some(ref hk) = self.hotkey_manager {
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
                        self.ui_state.show_settings_window = true;
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

        // Render the entire UI
        prxvc_desktop_ui::render_app(ctx, &mut self.ui_state);

        // Apply pending theme change (if settings changed the theme)
        if let Some(theme_name) = self.ui_state.pending_theme.take() {
            if let Some(new_theme) = Theme::all()
                .into_iter()
                .find(|t| t.name == theme_name)
            {
                self.ui_state.theme = new_theme;
                let mut visuals = ctx.style().visuals.clone();
                visuals.window_fill = self.ui_state.theme.bg_warm;
                ctx.set_visuals(visuals);
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
        save_settings_to_config(&self.ui_state.settings);

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
async fn run_engine_loop(shared: SharedAppState) {
    // Initialize storage (graceful degradation if unavailable).
    let storage = initialize_storage();

    // Load persisted history into the UI.
    if let Some(ref storage) = storage {
        load_history_from_storage(storage, &shared);
    }

    // Initialize config (used for transport selection and future settings).
    let config = initialize_config();

    // Create Engine with transport selected by config.
    let (event_tx, _event_rx) = prxvc_core::bus::event_channel(256);

    let transport: Box<dyn prxvc_protocol::transport::Transport> = if config.network.use_mock {
        tracing::info!("Using MockGateway (offline mode)");
        Box::new(prxvc_protocol::mock::MockGateway::new())
    } else {
        tracing::info!("Connecting to prx-voice server: {}", config.network.server_url);
        let prx_config = prxvc_protocol::prx_voice_transport::PrxVoiceConfig {
            base_url: config.network.server_url.clone(),
            language: "zh-CN".into(),
            asr_provider: "mock".into(),
            agent_provider: "mock".into(),
            tts_provider: "mock".into(),
            channel: "app_sdk".into(),
            client_id: uuid::Uuid::new_v4().to_string(),
        };
        Box::new(prxvc_protocol::prx_voice_transport::PrxVoiceTransport::new(prx_config))
    };

    let transport_label = if config.network.use_mock { "MockGateway" } else { "PrxVoiceTransport" };
    let mut engine = prxvc_core::engine::Engine::new(transport, event_tx);

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

    // Main polling loop.
    loop {
        // 1. Drain UI commands and process them through the engine.
        let commands = shared.take_commands();
        for cmd in &commands {
            process_command(&mut engine, cmd, &shared, &storage, &mut audio_loop, &asr_text_tx);
        }

        // 2. Check for locally recognized speech and send to server.
        while let Ok(utterance) = asr_text_rx.try_recv() {
            if let Some(session) = engine.active_session() {
                let sid = session.id.0;
                tracing::info!(text = %utterance.text, "sending local ASR text to server");
                shared.add_transcript_line(TranscriptLine {
                    role: "user".into(),
                    text: utterance.text.clone(),
                    is_final: true,
                });
                let cmd = prxvc_protocol::Command::SendText {
                    session_id: sid,
                    text: utterance.text,
                };
                if let Err(e) = engine.process_command(cmd) {
                    tracing::warn!("SendText failed: {e}");
                }
            }
        }

        // 3. Poll transport events from the engine and update the UI.
        match engine.poll_transport_events() {
            Ok(events) => {
                for event in &events {
                    handle_engine_event(event, &shared);
                }
            }
            Err(e) => {
                tracing::warn!("transport poll error: {e}");
            }
        }

        // 4. Update debug state info from the active session.
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
fn process_command(
    engine: &mut prxvc_core::engine::Engine,
    cmd: &str,
    shared: &SharedAppState,
    storage: &Option<prxvc_storage::StorageManager>,
    audio_loop: &mut AudioProcessingLoop,
    asr_text_tx: &std::sync::mpsc::Sender<RecognizedUtterance>,
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
                tracing::debug!("InterruptPlayback ignored: no active session");
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

    match engine.process_command(command) {
        Ok(()) => {
            // After CreateSession, propagate the session id to the UI.
            if cmd == "CreateSession" {
                if let Some(session) = engine.active_session() {
                    shared.set_session_id(Some(session.id.0.to_string()));
                }
            }

            // After StartSession, start the local audio ASR loop.
            if cmd == "StartSession" {
                start_audio_loop(engine, audio_loop, asr_text_tx, shared);
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
) {
    {
        let Some(session) = engine.active_session() else {
            return;
        };
        let session_id = session.id.0;

        // Model paths — look in standard locations.
        let models_dir = directories::ProjectDirs::from("com", "prx", "voice-client")
            .map(|d| d.data_dir().join("models"))
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

        if let Err(e) = audio_loop.start(asr_config, session_id, asr_text_tx.clone()) {
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
    let dirs = directories::ProjectDirs::from("com", "prx", "prx-voice-client")?;
    let db_path = dirs.data_dir().join("sessions.db");
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

/// Load [`AppConfig`] from the platform-specific config directory.
///
/// Falls back to defaults if the directory or file cannot be accessed.
fn initialize_config() -> prxvc_config::AppConfig {
    match prxvc_config::AppConfigManager::with_default_dir("prx-voice-client") {
        Ok(mgr) => mgr.load().unwrap_or_default(),
        Err(_) => prxvc_config::AppConfig::default(),
    }
}

/// Persist UI settings to config.toml (debounced — only writes when values differ).
fn save_settings_to_config(settings: &prxvc_desktop_ui::UiSettings) {
    use std::sync::LazyLock;
    use parking_lot::Mutex;

    // Cache the last saved snapshot to avoid writing every frame
    static LAST: LazyLock<Mutex<(bool, String, String)>> = LazyLock::new(|| {
        Mutex::new((true, "http://localhost:3000".to_owned(), "en".to_owned()))
    });

    let current = (settings.use_mock, settings.server_url.clone(), settings.language.clone());
    {
        let last = LAST.lock();
        if *last == current {
            return;
        }
    }

    // Values changed — write to disk
    if let Ok(mgr) = prxvc_config::AppConfigManager::with_default_dir("prx-voice-client") {
        let mut config = mgr.load().unwrap_or_default();
        config.network.use_mock = settings.use_mock;
        config.network.server_url = settings.server_url.clone();
        config.ui.language = settings.language.clone();
        config.audio.auto_play_reply = settings.auto_play;
        config.privacy.save_history = settings.save_history;
        if let Err(e) = mgr.save(&config) {
            tracing::warn!("Failed to save config: {}", e);
        } else {
            tracing::debug!("Config saved to disk");
        }
    }

    // Update cache
    *LAST.lock() = current;
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
