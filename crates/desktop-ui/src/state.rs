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
pub struct UiSettings {
    /// Selected audio input device name.
    pub input_device: String,
    /// Selected audio output device name.
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
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            input_device: "Default".to_owned(),
            output_device: "Default".to_owned(),
            language: "en".to_owned(),
            auto_play: true,
            save_history: true,
            show_transcript: true,
            hotkey_enabled: false,
            server_url: "http://localhost:3000".to_owned(),
            use_mock: true,
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
    /// Optional status bar message.
    pub status_message: Option<String>,
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
            reconnect_attempts: 0,
            status_message: None,
            anim_time: 0.0,
            idle_animation: true,
            windows: tool_window::ToolWindows::default(),
            theme: Theme::default(),
            pending_theme: None,
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

        // Status
        self.status_message = snap.status_message;

        // Error and network
        self.error_message = snap.error_message;
        self.error_recoverable = snap.error_recoverable;
        self.network_status = snap.network_status;
        self.reconnect_attempts = snap.reconnect_attempts;
        self.mic_paused = snap.mic_paused;
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
