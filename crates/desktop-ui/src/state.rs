//! UI state: the view models, `DesktopUiState` and its sync from and flush to the shared state.

use crate::shared::{NetworkStatus, SharedAppState};
use crate::theme_presets::Theme;
use crate::tool_window;

/// A single line in the transcript display.
#[derive(Debug, Clone)]
pub struct TranscriptLine {
    /// Role of the speaker: "user", "assistant", or "system".
    pub role: String,
    /// The transcript text content.
    pub text: String,
    /// Whether this transcript line is finalized.
    pub is_final: bool,
}

/// A history list entry representing a past session.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Unique identifier for the session.
    pub session_id: String,
    /// Human-readable title for the session.
    pub title: String,
    /// Timestamp when the session was created.
    pub created_at: String,
    /// Duration of the session as a human-readable string.
    pub duration: String,
    /// Final state of the session (e.g., "Completed", "Failed").
    pub state: String,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            title: String::new(),
            created_at: String::new(),
            duration: String::new(),
            state: String::new(),
        }
    }
}

/// UI-level settings (mirrors config but for display/editing).
#[derive(Debug, Clone, PartialEq)]
pub struct UiSettings {
    /// Selected audio input device (UID); empty is the system default.
    pub input_device: String,
    /// Selected audio output device (UID); empty is the system default.
    pub output_device: String,
    /// UI language code (e.g., "en", "zh-CN").
    pub language: String,
    /// Whether to auto-play assistant audio responses.
    pub auto_play: bool,
    /// Whether to persist session history locally.
    pub save_history: bool,
    /// Whether to show live transcript in the session view.
    pub show_transcript: bool,
    /// Whether the global hotkey is enabled.
    pub hotkey_enabled: bool,
    /// Server URL for prx-voice connection.
    pub server_url: String,
    /// Whether using mock mode (offline).
    pub use_mock: bool,
    // ---- fields of the settings window (design 2.2); each maps to one config key ----
    /// `app.mode`: `"assistant"` or `"call"`.
    pub mode: String,
    /// `ui.theme`: the name of the colour theme (`"rf"` is the default).
    pub theme: String,
    /// `ui.compact_mode`.
    pub compact_mode: bool,
    /// `ui.always_on_top`.
    pub always_on_top: bool,
    /// `app.launch_at_login`.
    pub launch_at_login: bool,
    /// `call.self_language`; empty follows the interface language.
    pub call_self_language: String,
    /// `call.peer_language`; empty is the other one of the two.
    pub call_peer_language: String,
    /// `call.target_app` (bundle id); empty is to choose when starting.
    pub call_target_app: String,
    /// `call.local_output_mode`: `"translation"`, `"both"` or `"original"`.
    pub call_local_output_mode: String,
    /// `call.outgoing_capture`: `"hands_free"` or `"push_to_talk"`.
    pub call_outgoing_capture: String,
    /// `call.captions_show_source`.
    pub call_captions_show_source: bool,
    /// `call.barge_in`.
    pub call_barge_in: bool,
    /// `network.language`: the assistant's session language.
    pub network_language: String,
    /// `audio.echo_mode`: `"auto"`, `"always"` or `"off"`.
    pub echo_mode: String,
    /// `audio.virtual_mic_device`: a sink UID; empty is this app's own driver.
    pub virtual_mic_device: String,
    /// `audio.sample_rate` (read-only).
    pub sample_rate: u32,
    /// `audio.channels` (read-only).
    pub channels: u16,
    /// `voice.self_profile_id` (read-only here).
    pub voice_profile_id: String,
    /// `voice.use_clone_outgoing`.
    pub voice_use_clone: bool,
    /// `voice.incoming_preset`; empty is the catalogue default.
    pub voice_incoming_preset: String,
    /// `voice.fallback_preset`; empty is the catalogue default.
    pub voice_fallback_preset: String,
    /// `providers.preset` (read-only here): `"local"`, `"hybrid"`, `"cloud"` or `"custom"`.
    pub providers_preset: String,
    /// `models.preload`.
    pub models_preload: bool,
    /// `models.warmup`.
    pub models_warmup: bool,
    /// `models.endpoint`: `"huggingface"`, `"modelscope"` or an `https://` URL.
    pub models_endpoint: String,
    /// `models.max_concurrent`, 1 to 3.
    pub models_max_concurrent: u32,
    /// `models.dir`; empty is the default location.
    pub models_dir: String,
    /// `models.catalogue_url`.
    pub models_catalogue_url: String,
    /// `models.catalogue_auto_update`.
    pub models_catalogue_auto_update: bool,
    /// `hotkeys.session_toggle` (read-only here).
    pub hotkey_session_toggle: String,
    /// `hotkeys.push_to_talk` (read-only here).
    pub hotkey_push_to_talk: String,
    /// `hotkeys.interrupt` (read-only here).
    pub hotkey_interrupt: String,
    /// `hotkeys.assistant_mode`: `"toggle"` or `"hold"`.
    pub hotkey_assistant_mode: String,
    /// `privacy.save_transcript`.
    pub save_transcript: bool,
    /// `privacy.log_sensitive`.
    pub log_sensitive: bool,
    /// `debug.enabled`.
    pub debug_enabled: bool,
    /// `debug.show_debug_panel`.
    pub debug_show_panel: bool,
    /// `debug.log_level`.
    pub debug_log_level: String,
    /// `network.asr_provider`.
    pub network_asr_provider: String,
    /// `network.agent_provider`.
    pub network_agent_provider: String,
    /// `network.tts_provider`.
    pub network_tts_provider: String,
    /// `network.connect_timeout_ms`.
    pub connect_timeout_ms: u64,
    /// `network.reconnect_attempts`.
    pub reconnect_attempts: u32,
    /// `network.reconnect_delay_ms`.
    pub reconnect_delay_ms: u64,
    /// `network_policy.offline`.
    pub offline: bool,
    /// `network_policy.allow_hosts`.
    pub allow_hosts: Vec<String>,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            input_device: String::new(),
            output_device: String::new(),
            language: "en".to_owned(),
            auto_play: true,
            save_history: true,
            show_transcript: true,
            hotkey_enabled: false,
            server_url: "http://localhost:3000".to_owned(),
            use_mock: true,
            mode: "assistant".to_owned(),
            theme: "rf".to_owned(),
            compact_mode: false,
            always_on_top: true,
            launch_at_login: false,
            call_self_language: String::new(),
            call_peer_language: String::new(),
            call_target_app: String::new(),
            call_local_output_mode: "translation".to_owned(),
            call_outgoing_capture: "hands_free".to_owned(),
            call_captions_show_source: true,
            call_barge_in: true,
            network_language: "zh-CN".to_owned(),
            echo_mode: "auto".to_owned(),
            virtual_mic_device: String::new(),
            sample_rate: 16_000,
            channels: 1,
            voice_profile_id: String::new(),
            voice_use_clone: true,
            voice_incoming_preset: String::new(),
            voice_fallback_preset: String::new(),
            providers_preset: "local".to_owned(),
            models_preload: true,
            models_warmup: true,
            models_endpoint: "huggingface".to_owned(),
            models_max_concurrent: 2,
            models_dir: String::new(),
            models_catalogue_url: String::new(),
            models_catalogue_auto_update: false,
            hotkey_session_toggle: "Ctrl+Shift+Space".to_owned(),
            hotkey_push_to_talk: String::new(),
            hotkey_interrupt: String::new(),
            hotkey_assistant_mode: "toggle".to_owned(),
            save_transcript: false,
            log_sensitive: false,
            debug_enabled: false,
            debug_show_panel: false,
            debug_log_level: "info".to_owned(),
            network_asr_provider: "mock".to_owned(),
            network_agent_provider: "mock".to_owned(),
            network_tts_provider: "mock".to_owned(),
            connect_timeout_ms: 5_000,
            reconnect_attempts: 3,
            reconnect_delay_ms: 1_000,
            offline: false,
            allow_hosts: Vec::new(),
        }
    }
}

/// View model for the main session panel (kept for backward compatibility).
pub struct SessionViewModel {
    /// Human-readable label describing the current session state.
    pub state_label: String,
    /// Whether a session is currently active.
    pub is_session_active: bool,
    /// Whether the Start button should be enabled.
    pub can_start: bool,
    /// Whether the Stop button should be enabled.
    pub can_stop: bool,
    /// Whether the Interrupt button should be enabled.
    pub can_interrupt: bool,
    /// Transcript lines for the current session.
    pub transcript_lines: Vec<TranscriptLine>,
    /// Name of the selected input audio device.
    pub input_device: String,
    /// Name of the selected output audio device.
    pub output_device: String,
}

impl Default for SessionViewModel {
    fn default() -> Self {
        Self {
            state_label: "Idle".to_owned(),
            is_session_active: false,
            can_start: true,
            can_stop: false,
            can_interrupt: false,
            transcript_lines: Vec::new(),
            input_device: "Default".to_owned(),
            output_device: "Default".to_owned(),
        }
    }
}

/// Navigation pages available in the desktop application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Main voice session view (orb).
    Session,
    /// Session history browser.
    History,
    /// Application settings.
    Settings,
    /// Debug / diagnostics view.
    Debug,
    /// About information.
    About,
}

/// Top-level UI state for the desktop application.
pub struct DesktopUiState {
    /// The currently displayed page.
    pub current_page: Page,
    /// Current session state label (driven by shared state).
    pub session_state: String,
    /// Whether a session is currently active.
    pub session_active: bool,
    /// Current session ID, if any.
    pub session_id: Option<String>,
    /// View model for the session panel (backward compat).
    pub session_vm: SessionViewModel,
    /// Transcript lines for the active session.
    pub transcript_lines: Vec<TranscriptLine>,
    /// Collects command names when buttons are clicked, for later dispatch.
    pub pending_commands: Vec<String>,
    /// Whether the settings panel is open (right side).
    pub settings_open: bool,
    /// Whether the history panel is open (right side).
    pub history_open: bool,
    /// Session history entries.
    pub history_entries: Vec<HistoryEntry>,
    /// UI-level settings.
    pub settings: UiSettings,
    /// Debug event log entries.
    pub debug_events: Vec<String>,
    /// Current state machine info for the debug page.
    pub debug_state_info: String,
    /// Current error message to display (if any).
    pub error_message: Option<String>,
    /// Whether the current error is recoverable (shows retry button).
    pub error_recoverable: bool,
    /// Network/transport connection status.
    pub network_status: NetworkStatus,
    /// Number of reconnect attempts so far.
    pub reconnect_attempts: u32,
    /// The microphone is paused while the assistant speaks (echo gate closed).
    pub mic_paused: bool,
    /// Name of the input device being captured; empty when none.
    pub input_device: String,
    /// Optional status bar message.
    pub status_message: Option<String>,
    /// Call mode as the temporary debug control shows it.
    pub call: crate::call_view::CallPanel,
    /// Internal animation time tracker.
    pub(crate) anim_time: f64,
    /// The idle orb breathes (and the app repaints for it). Tests turn it off
    /// so a settled harness shows a still orb.
    pub idle_animation: bool,
    /// The settings, history and debug windows (native windows of their own).
    pub windows: tool_window::ToolWindows,
    /// Current color theme.
    pub theme: Theme,
    /// Pending theme name to apply (set by settings, consumed by app loop).
    pub pending_theme: Option<String>,
    /// Navigation and drafts of the settings window.
    pub settings_view: crate::settings_window::SettingsViewState,
    /// What the app probed for the settings window (devices, permissions, ...).
    pub settings_data: crate::settings_data::SettingsData,
    /// Requests of the settings window the app applies after the frame.
    pub settings_commands: Vec<crate::settings_command::SettingsCommand>,
    /// Where the app publishes [`SettingsData`](crate::settings_data::SettingsData).
    pub settings_feed: crate::settings_feed::SettingsFeed,
    /// Version of the feed last copied into `settings_data`.
    settings_feed_seen: u64,
}

impl DesktopUiState {
    /// Create a new UI state with defaults.
    pub fn new() -> Self {
        Self {
            current_page: Page::Session,
            session_state: String::new(),
            session_active: false,
            session_id: None,
            session_vm: SessionViewModel::default(),
            transcript_lines: Vec::new(),
            pending_commands: Vec::new(),
            settings_open: false,
            history_open: false,
            history_entries: Vec::new(),
            settings: UiSettings::default(),
            debug_events: Vec::new(),
            debug_state_info: "Idle".to_owned(),
            error_message: None,
            error_recoverable: false,
            network_status: NetworkStatus::default(),
            mic_paused: false,
            input_device: String::new(),
            reconnect_attempts: 0,
            status_message: None,
            call: crate::call_view::CallPanel::default(),
            anim_time: 0.0,
            idle_animation: true,
            windows: tool_window::ToolWindows::default(),
            theme: Theme::default(),
            pending_theme: None,
            settings_view: crate::settings_window::SettingsViewState::default(),
            settings_data: crate::settings_data::SettingsData::default(),
            settings_commands: Vec::new(),
            settings_feed: crate::settings_feed::SettingsFeed::default(),
            settings_feed_seen: 0,
        }
    }

    /// Open the settings window, or focus it when it is open, at `section`
    /// (the page it last showed when `None`).
    pub fn open_settings(&mut self, section: Option<crate::settings_section::SettingsSection>) {
        if let Some(section) = section {
            self.settings_view.section = section;
        }
        self.windows.request_open(tool_window::ToolKind::Settings);
        self.settings_commands.push(crate::settings_command::SettingsCommand::Refresh);
    }

    /// Copy newly published settings data (devices, permissions, ...) into
    /// this state. Called once per frame by the app.
    pub fn sync_settings_feed(&mut self) {
        if let Some(data) = self.settings_feed.take_if_changed(&mut self.settings_feed_seen) {
            self.settings_data = data;
        }
    }

    /// Pull the latest display data from the shared state into this UI state.
    ///
    /// Called once per frame before rendering. Also consumes any pending events
    /// from the runtime.
    pub fn sync_from_shared(&mut self, shared: &SharedAppState) {
        // Consume pending events (drives the debug log, already stored in shared)
        let _events = shared.take_events();

        let snap = shared.snapshot_for_ui();

        // Session state (new orb-driven fields)
        self.session_state = snap.session_state.clone();
        self.session_active = snap.session_active;
        self.session_id = snap.session_id.clone();

        // Map session state to view-model fields (backward compat)
        let state = snap.session_state.as_str();
        self.session_vm.state_label = snap.session_state.clone();
        self.session_vm.is_session_active = snap.session_active;
        self.session_vm.can_start = matches!(state, "Idle" | "Completed" | "");
        self.session_vm.can_stop = snap.session_active;
        self.session_vm.can_interrupt = state == "AssistantSpeaking";

        // Transcript (both new and legacy)
        self.transcript_lines = snap.transcript_lines.clone();
        self.session_vm.transcript_lines = snap.transcript_lines;

        // History
        self.history_entries = snap.history_entries;

        // Debug
        self.debug_events = snap.debug_events;
        self.debug_state_info = if snap.session_active {
            format!(
                "{} (session: {})",
                snap.session_state,
                snap.session_id.as_deref().unwrap_or("unknown")
            )
        } else {
            format!("{} (no active session)", snap.session_state)
        };
        if !snap.input_device.is_empty() {
            self.debug_state_info = format!(
                "{}\n{}: {}",
                self.debug_state_info,
                prxvc_i18n::t("debug.input_device"),
                snap.input_device
            );
        }

        // Call mode
        self.call = snap.call;

        // Status
        self.status_message = snap.status_message;

        // Error and network
        self.error_message = snap.error_message;
        self.error_recoverable = snap.error_recoverable;
        self.network_status = snap.network_status;
        self.reconnect_attempts = snap.reconnect_attempts;
        self.mic_paused = snap.mic_paused;
        self.input_device = snap.input_device;
    }

    /// Flush any pending UI commands into the shared state for the runtime.
    ///
    /// Called once per frame after rendering (buttons push into
    /// `pending_commands` during the render pass).
    pub fn flush_commands_to_shared(&mut self, shared: &SharedAppState) {
        for cmd in self.pending_commands.drain(..) {
            shared.push_command(&cmd);
        }
    }
}

impl Default for DesktopUiState {
    fn default() -> Self {
        Self::new()
    }
}
