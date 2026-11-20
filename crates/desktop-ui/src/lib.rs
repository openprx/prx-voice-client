//! Desktop UI layer for PRX Voice Client using egui.
//!
//! The UI centers on a living orb that represents the AI's state. Three zones:
//! - Left sidebar (narrow, ~200px) — navigation + status
//! - Central stage (fills remaining space) — the ORB + state label + transcript
//! - Right panel (slides in) — settings or history
//!
//! The look follows the RF design standard: see [`theme`] for the tokens and
//! the egui style built from them, and [`widgets`] for the custom widgets.

pub mod fonts;
pub mod settings_view;
pub mod shared;
pub mod theme;
pub mod tool_window;
pub mod widgets;

pub use shared::{EventSnapshot, NetworkStatus, SharedAppState, UiSnapshot};
pub use theme::{Scheme, Tokens};

use egui::{Color32, CornerRadius, FontId, StrokeKind};
use tool_window::ToolKind;
use theme::{hairline, layout, mix, radius, size, space};
use widgets::{ButtonKind, Tone};

// ---------------------------------------------------------------------------
// Theme system
// ---------------------------------------------------------------------------

/// Color theme for the UI.
///
/// The RF theme ([`Theme::rf`]) is the default. Its colours come from the
/// design tokens in [`theme`] and follow the system light/dark preference;
/// the other built-in themes are dark colour variants that stay selectable.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Human-readable theme name.
    pub name: &'static str,
    /// Colour scheme in effect (always dark for the legacy colour themes).
    pub scheme: Scheme,
    /// Deep background color.
    pub bg_warm: Color32,
    /// Elevated surface color for cards and panels.
    pub bg_elevated: Color32,
    /// Primary text color.
    pub text_primary: Color32,
    /// Softer text color for secondary content.
    pub text_soft: Color32,
    /// Dim text color for hints and labels.
    pub text_dim: Color32,
    /// Accent / highlight color.
    pub accent: Color32,
    /// Orb color in idle state.
    pub orb_idle: Color32,
    /// Orb color when listening.
    pub orb_listening: Color32,
    /// Orb color when thinking.
    pub orb_thinking: Color32,
    /// Orb color when speaking.
    pub orb_speaking: Color32,
}

impl Theme {
    /// The RF theme (default) in the given scheme.
    ///
    /// The orb uses the standard's own colours: `primary` when idle, the
    /// semantic `success` when listening, `warning` when thinking and the
    /// first chart colour when speaking.
    pub fn rf(scheme: Scheme) -> Self {
        let tk = scheme.tokens();
        Self {
            name: theme::RF_THEME_NAME,
            scheme,
            bg_warm: tk.bg,
            bg_elevated: tk.surface_white,
            text_primary: tk.text,
            text_soft: tk.secondary,
            text_dim: tk.secondary,
            accent: tk.primary,
            orb_idle: tk.primary,
            orb_listening: tk.success,
            orb_thinking: tk.warning,
            orb_speaking: tk.chart_1,
        }
    }

    /// Whether this is the RF theme.
    pub fn is_rf(&self) -> bool {
        self.name == theme::RF_THEME_NAME
    }

    /// The design tokens this theme paints with.
    ///
    /// For the RF theme these are the standard's tokens of [`Self::scheme`];
    /// the legacy colour themes derive an approximate token set from their
    /// own colours so the same widgets work on every theme.
    pub fn tokens(&self) -> Tokens {
        if self.is_rf() {
            self.scheme.tokens()
        } else {
            self.legacy_tokens()
        }
    }

    fn legacy_tokens(&self) -> Tokens {
        let mut tk = theme::DARK;
        let (bg, raised, text) = (self.bg_warm, self.bg_elevated, self.text_primary);
        tk.bg = bg;
        tk.nav = mix(bg, Color32::BLACK, 0.25);
        tk.rail = mix(bg, Color32::BLACK, 0.45);
        tk.surface = raised;
        tk.surface_white = raised;
        tk.overlay = raised;
        tk.input = bg;
        tk.selected = mix(raised, text, 0.08);
        tk.overlay_border = mix(raised, text, 0.12);
        tk.line = mix(raised, text, 0.12);
        tk.line_strong = mix(raised, text, 0.22);
        tk.input_border = self.text_dim;
        tk.control = self.text_dim;
        tk.text = text;
        tk.secondary = self.text_soft;
        tk.primary = self.accent;
        tk.on_primary = bg;
        tk.focus = self.accent;
        tk.success = self.orb_listening;
        tk.success_bg = mix(raised, self.orb_listening, 0.15);
        tk.warning = self.orb_thinking;
        tk.warning_bg = mix(raised, self.orb_thinking, 0.15);
        tk.danger = self.orb_speaking;
        tk.danger_bg = mix(raised, self.orb_speaking, 0.15);
        tk.on_danger = bg;
        tk.chart_1 = self.orb_idle;
        tk
    }

    /// Midnight Purple (formerly the default theme).
    pub fn midnight() -> Self {
        Self {
            name: "Midnight",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(19, 17, 26),
            bg_elevated: Color32::from_rgb(34, 30, 46),
            text_primary: Color32::from_rgb(237, 233, 246),
            text_soft: Color32::from_rgb(168, 158, 194),
            text_dim: Color32::from_rgb(88, 79, 110),
            accent: Color32::from_rgb(167, 139, 250),
            orb_idle: Color32::from_rgb(167, 139, 250),
            orb_listening: Color32::from_rgb(94, 234, 212),
            orb_thinking: Color32::from_rgb(251, 191, 36),
            orb_speaking: Color32::from_rgb(248, 113, 113),
        }
    }

    /// Ocean Blue.
    pub fn ocean() -> Self {
        Self {
            name: "Ocean",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(13, 17, 28),
            bg_elevated: Color32::from_rgb(22, 30, 48),
            text_primary: Color32::from_rgb(220, 233, 250),
            text_soft: Color32::from_rgb(140, 165, 200),
            text_dim: Color32::from_rgb(70, 90, 120),
            accent: Color32::from_rgb(96, 165, 250),
            orb_idle: Color32::from_rgb(96, 165, 250),
            orb_listening: Color32::from_rgb(52, 211, 153),
            orb_thinking: Color32::from_rgb(251, 191, 36),
            orb_speaking: Color32::from_rgb(251, 146, 60),
        }
    }

    /// Rose Gold.
    pub fn rose() -> Self {
        Self {
            name: "Rose",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(24, 16, 20),
            bg_elevated: Color32::from_rgb(42, 28, 35),
            text_primary: Color32::from_rgb(248, 232, 238),
            text_soft: Color32::from_rgb(200, 160, 180),
            text_dim: Color32::from_rgb(120, 80, 100),
            accent: Color32::from_rgb(244, 114, 182),
            orb_idle: Color32::from_rgb(244, 114, 182),
            orb_listening: Color32::from_rgb(168, 230, 207),
            orb_thinking: Color32::from_rgb(252, 211, 77),
            orb_speaking: Color32::from_rgb(239, 68, 68),
        }
    }

    /// Forest Green.
    pub fn forest() -> Self {
        Self {
            name: "Forest",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(12, 20, 16),
            bg_elevated: Color32::from_rgb(24, 38, 30),
            text_primary: Color32::from_rgb(220, 243, 230),
            text_soft: Color32::from_rgb(150, 195, 170),
            text_dim: Color32::from_rgb(75, 110, 90),
            accent: Color32::from_rgb(74, 222, 128),
            orb_idle: Color32::from_rgb(74, 222, 128),
            orb_listening: Color32::from_rgb(147, 197, 253),
            orb_thinking: Color32::from_rgb(253, 224, 71),
            orb_speaking: Color32::from_rgb(248, 113, 113),
        }
    }

    /// Amber Ember.
    pub fn ember() -> Self {
        Self {
            name: "Ember",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(22, 16, 10),
            bg_elevated: Color32::from_rgb(40, 30, 20),
            text_primary: Color32::from_rgb(250, 240, 225),
            text_soft: Color32::from_rgb(200, 175, 145),
            text_dim: Color32::from_rgb(120, 95, 70),
            accent: Color32::from_rgb(251, 146, 60),
            orb_idle: Color32::from_rgb(251, 146, 60),
            orb_listening: Color32::from_rgb(94, 234, 212),
            orb_thinking: Color32::from_rgb(253, 224, 71),
            orb_speaking: Color32::from_rgb(239, 68, 68),
        }
    }

    /// All available built-in themes. The RF theme comes first.
    pub fn all() -> Vec<Theme> {
        vec![
            Self::rf(Scheme::Light),
            Self::midnight(),
            Self::ocean(),
            Self::rose(),
            Self::forest(),
            Self::ember(),
        ]
    }
}

impl Default for Theme {
    /// The RF theme. Its scheme is resolved against the system preference on
    /// the first rendered frame.
    fn default() -> Self {
        Self::rf(Scheme::Light)
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// DesktopUiState
// ---------------------------------------------------------------------------

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
    anim_time: f64,
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


// ---------------------------------------------------------------------------
// Orb rendering
// ---------------------------------------------------------------------------

/// Render the animated orb in the center of the stage area.
///
/// The orb changes color and animation based on session state. Returns a
/// clickable response so callers can handle click-to-start/interrupt.
pub fn render_orb(ui: &mut egui::Ui, state: &DesktopUiState, time: f64) -> egui::Response {
    let available = ui.available_size();
    let center = egui::pos2(available.x / 2.0, available.y * 0.4);
    let base_radius: f32 = 80.0;
    let tk = state.theme.tokens();

    let (orb_color, scale_anim) = orb_animation(state, time);
    let radius = base_radius * scale_anim;

    // Allocate the clickable area first (requires &mut ui), then paint over it.
    let orb_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
    let response = ui.allocate_rect(orb_rect, egui::Sense::click());

    let painter = ui.painter();
    paint_orb(painter, &tk, center, radius, orb_color, time, response.hovered());
    response
}

/// Paint the orb: glow, two breathing rings, body, shimmer and hover ring.
///
/// Rings, shimmer and hover ring use the `text` token with a low alpha so
/// they read on both schemes.
fn paint_orb(
    painter: &egui::Painter,
    tk: &Tokens,
    center: egui::Pos2,
    radius: f32,
    orb_color: Color32,
    time: f64,
    hovered: bool,
) {
    let with_alpha = |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
    let ring_alpha = (20.0 * (1.0 + (time * 0.8).sin() as f32) / 2.0) as u8;

    painter.circle_filled(center, radius * 2.2, with_alpha(orb_color, 20));
    painter.circle_stroke(
        center,
        radius * 1.35,
        egui::Stroke::new(1.0_f32, with_alpha(tk.text, ring_alpha)),
    );
    painter.circle_stroke(
        center,
        radius * 1.6,
        egui::Stroke::new(0.5_f32, with_alpha(tk.text, ring_alpha / 2)),
    );
    painter.circle_filled(center, radius, orb_color);

    // Highlight shimmer (top-left).
    let hl = egui::pos2(center.x - radius * 0.2, center.y - radius * 0.2);
    painter.circle_filled(hl, radius * 0.35, with_alpha(Color32::WHITE, 35));

    if hovered {
        painter.circle_stroke(
            center,
            radius + 3.0,
            egui::Stroke::new(1.5_f32, with_alpha(tk.text, 60)),
        );
    }
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

/// Colour of the connection status dot for a network status.
fn status_color(tk: &Tokens, status: &NetworkStatus) -> Color32 {
    match status {
        NetworkStatus::Connected => tk.success,
        NetworkStatus::Disconnected => tk.control,
        NetworkStatus::Connecting => tk.secondary,
        NetworkStatus::Reconnecting => tk.warning,
        NetworkStatus::Failed => tk.danger,
    }
}

/// Render the left sidebar with brand, navigation, and status indicator.
pub fn render_sidebar(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    let tk = theme::tokens_for(ui.ctx());
    ui.vertical(|ui| {
        ui.add_space(space::S12);

        // Brand
        ui.horizontal(|ui| {
            ui.label(widgets::text("PRX", size::SUBHEADING, tk.text));
            ui.label(widgets::text("voice", size::BUTTON, tk.secondary));
        });

        ui.add_space(space::S20);

        // Nav items
        let items = [
            (prxvc_i18n::t("nav.session"), Page::Session),
            (prxvc_i18n::t("nav.history"), Page::History),
            (prxvc_i18n::t("nav.settings"), Page::Settings),
            (prxvc_i18n::t("nav.debug"), Page::Debug),
        ];

        for (label, page) in &items {
            let is_active =
                std::mem::discriminant(&state.current_page) == std::mem::discriminant(page);
            if widgets::menu_item(ui, &tk, label, is_active).clicked() {
                state.current_page = *page;
                // Toggle panels
                state.settings_open = matches!(page, Page::Settings);
                state.history_open = matches!(page, Page::History);
            }
        }

        // Spacer — push status to the bottom
        let remaining = ui.available_height() - 40.0;
        if remaining > 0.0 {
            ui.add_space(remaining);
        }

        // Status indicator: a dot and words (status is words first).
        ui.horizontal(|ui| {
            let (_, dot_rect) = ui.allocate_space(egui::vec2(8.0, 8.0));
            ui.painter().circle_filled(
                dot_rect.center(),
                4.0,
                status_color(&tk, &state.network_status),
            );

            let status_text = match state.network_status {
                NetworkStatus::Connected => prxvc_i18n::t("status.ready"),
                NetworkStatus::Disconnected => prxvc_i18n::t("status.offline"),
                NetworkStatus::Connecting => prxvc_i18n::t("status.connecting"),
                NetworkStatus::Reconnecting => prxvc_i18n::t("status.reconnecting"),
                NetworkStatus::Failed => prxvc_i18n::t("status.failed"),
            };
            ui.label(widgets::text(status_text, size::META, tk.secondary));
        });
    });
}

// ---------------------------------------------------------------------------
// State info (below orb)
// ---------------------------------------------------------------------------

/// The hint line shown under the state label for a session state.
fn state_hint(session_state: &str) -> String {
    match session_state {
        "" | "Idle" | "Completed" => prxvc_i18n::t("session.hint.idle"),
        "Listening" => prxvc_i18n::t("session.hint.listening"),
        "AssistantSpeaking" => prxvc_i18n::t("session.hint.speaking"),
        _ => String::new(),
    }
}

/// Render the state label and hint text below the orb.
pub fn render_state_info(ui: &mut egui::Ui, state: &DesktopUiState) {
    let tk = state.theme.tokens();
    ui.vertical_centered(|ui| {
        let label = translate_state_human(&state.session_state);
        ui.label(widgets::text(label, size::TITLE, tk.text));

        ui.add_space(space::S4);

        let hint = if state.mic_paused && state.session_state == "AssistantSpeaking" {
            prxvc_i18n::t("session.hint.mic_paused")
        } else {
            state_hint(&state.session_state)
        };
        if !hint.is_empty() {
            ui.label(widgets::text(hint, size::META, tk.secondary));
        }
    });
}

/// Translate a raw session state to a human-friendly i18n label for the orb UI.
fn translate_state_human(state: &str) -> String {
    match state {
        "" | "Idle" => prxvc_i18n::t("orb.ready"),
        "Listening" => prxvc_i18n::t("orb.listening"),
        "UserSpeaking" => prxvc_i18n::t("orb.hearing"),
        "Recognizing" => prxvc_i18n::t("orb.processing"),
        "Thinking" => prxvc_i18n::t("orb.thinking"),
        "AssistantSpeaking" => prxvc_i18n::t("orb.speaking"),
        "Interrupted" => prxvc_i18n::t("orb.interrupted"),
        "Completed" => prxvc_i18n::t("orb.ended"),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Transcript panel
// ---------------------------------------------------------------------------

/// Paint one transcript line: a role word and the text. People and the
/// assistant are told apart by the word, not by hue.
fn transcript_line_ui(
    ui: &mut egui::Ui,
    tk: &Tokens,
    line: &TranscriptLine,
    text_size: f32,
    role_size: f32,
) {
    ui.horizontal_wrapped(|ui| {
        let (role_text, role_color) = if line.role == "user" {
            (prxvc_i18n::t("transcript.you"), tk.text)
        } else {
            (prxvc_i18n::t("transcript.ai"), tk.secondary)
        };
        ui.label(
            egui::RichText::new(role_text)
                .size(role_size)
                .color(role_color)
                .monospace(),
        );
        let text_color = if line.is_final { tk.text } else { tk.secondary };
        ui.label(widgets::text(&line.text, text_size, text_color));
    });
}

/// Render the scrollable transcript at the bottom of the central stage.
pub fn render_transcript(ui: &mut egui::Ui, lines: &[TranscriptLine]) {
    if lines.is_empty() {
        return;
    }
    let tk = theme::tokens_for(ui.ctx());

    egui::ScrollArea::vertical()
        .max_height(200.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in lines {
                transcript_line_ui(ui, &tk, line, size::BUTTON, size::CAPTION);
                ui.add_space(space::S4);
            }
        });
}

// ---------------------------------------------------------------------------
// Controls (floating at bottom when session active)
// ---------------------------------------------------------------------------

/// Render floating session controls (interrupt / end) when a session is active.
pub fn render_controls(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    if !state.session_active {
        return;
    }
    let tk = state.theme.tokens();

    ui.horizontal_centered(|ui| {
        // Interrupt button
        if state.session_state == "AssistantSpeaking"
            && widgets::button(
                ui,
                &tk,
                ButtonKind::Secondary,
                &prxvc_i18n::t("session.interrupt"),
            )
            .clicked()
        {
            state.pending_commands.push("InterruptPlayback".into());
        }

        // Stop button: a quiet destructive action.
        if widgets::button(
            ui,
            &tk,
            ButtonKind::DangerQuiet,
            &prxvc_i18n::t("session.stop"),
        )
        .clicked()
        {
            state.pending_commands.push("StopSession".into());
        }
    });
}

// ---------------------------------------------------------------------------
// Settings panel (right side)
// ---------------------------------------------------------------------------

/// Render the settings panel in the right sidebar.
pub fn render_settings_panel(ui: &mut egui::Ui, settings: &mut UiSettings) {
    let tk = theme::tokens_for(ui.ctx());
    ui.vertical(|ui| {
        ui.label(widgets::text(
            prxvc_i18n::t("settings.title"),
            size::TITLE,
            tk.text,
        ));
        ui.add_space(space::S16);
        // The legacy panel has no theme state; a theme chosen here is dropped.
        let mut ignored = None;
        settings_view::render_settings_content(
            ui,
            &tk,
            settings,
            theme::RF_THEME_NAME,
            &mut ignored,
        );
    });
}

// ---------------------------------------------------------------------------
// History panel (right side)
// ---------------------------------------------------------------------------

/// History rows: title, a meta line, and the state as a chip.
fn history_rows(ui: &mut egui::Ui, tk: &Tokens, entries: &[HistoryEntry]) {
    if entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(space::S32);
            ui.label(widgets::text(
                prxvc_i18n::t("history.empty"),
                size::SECONDARY,
                tk.secondary,
            ));
        });
        return;
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let count = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let meta = format!("{} \u{00b7} {}", entry.created_at, entry.duration);
        widgets::setting_row(ui, tk, &entry.title, Some(&meta), 96.0, i + 1 == count, |ui| {
            let tone = if entry.state == "Failed" {
                Tone::Danger
            } else {
                Tone::Neutral
            };
            widgets::chip(ui, tk, tone, &entry.state);
        });
    }
}

/// Render the history panel in the right sidebar.
pub fn render_history_panel(ui: &mut egui::Ui, entries: &[HistoryEntry]) {
    let tk = theme::tokens_for(ui.ctx());
    ui.vertical(|ui| {
        ui.label(widgets::text(
            prxvc_i18n::t("history.title"),
            size::TITLE,
            tk.text,
        ));
        ui.add_space(space::S16);
        history_rows(ui, &tk, entries);
    });
}

// ---------------------------------------------------------------------------
// Error banner
// ---------------------------------------------------------------------------

/// Render an error banner at the top of the screen if there is an active error.
///
/// Shown as a danger tip bar. Recoverable errors show a retry button. Both
/// include a close button.
pub fn render_error_banner(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    if let Some(ref error) = state.error_message {
        let error_text = error.clone();
        let tk = state.theme.tokens();

        widgets::tip_bar(ui, &tk, Tone::Danger, &error_text, |ui| {
            if state.error_recoverable
                && widgets::button_with(
                    ui,
                    &tk,
                    ButtonKind::Secondary,
                    &prxvc_i18n::t("common.retry"),
                    layout::CONTROL_HEIGHT_SMALL,
                    true,
                )
                .clicked()
            {
                state.pending_commands.push("RetryConnection".into());
                state.error_message = None;
            }
            if widgets::button_with(
                ui,
                &tk,
                ButtonKind::Text,
                &prxvc_i18n::t("common.close"),
                layout::CONTROL_HEIGHT_SMALL,
                true,
            )
            .clicked()
            {
                state.error_message = None;
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Debug page
// ---------------------------------------------------------------------------

/// Render the debug / diagnostics page.
pub fn render_debug_page(ui: &mut egui::Ui, state: &DesktopUiState) {
    let tk = state.theme.tokens();
    ui.label(widgets::text(
        prxvc_i18n::t("debug.title"),
        size::TITLE,
        tk.text,
    ));
    ui.add_space(space::S8);

    ui.horizontal(|ui| {
        ui.label(widgets::text(
            prxvc_i18n::t("debug.state_machine_label"),
            size::META,
            tk.secondary,
        ));
        ui.label(widgets::text(&state.debug_state_info, size::META, tk.text));
    });

    ui.add_space(space::S4);

    ui.horizontal(|ui| {
        ui.label(widgets::text(
            prxvc_i18n::t("debug.pending_commands_label"),
            size::META,
            tk.secondary,
        ));
        ui.label(widgets::text(
            format!("{}", state.pending_commands.len()),
            size::META,
            tk.text,
        ));
    });

    ui.add_space(space::S8);
    ui.label(widgets::text(
        prxvc_i18n::t("debug.recent_events"),
        size::META,
        tk.secondary,
    ));

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if state.debug_events.is_empty() {
                ui.label(widgets::text(
                    prxvc_i18n::t("debug.no_events_recorded"),
                    size::META,
                    tk.secondary,
                ));
            } else {
                for event in state.debug_events.iter().rev() {
                    ui.label(widgets::text(event, size::META, tk.secondary));
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Legacy render functions (backward compatibility for apps/desktop)
// ---------------------------------------------------------------------------

/// Render the main session panel (legacy — delegates to orb-centric layout).
pub fn render_session_panel(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    let time = ui.input(|i| i.time);
    let orb_response = render_orb(ui, state, time);

    if orb_response.clicked() {
        if !state.session_active {
            state.pending_commands.push("CreateSession".into());
            state.pending_commands.push("StartSession".into());
        } else if state.session_state == "AssistantSpeaking" {
            state.pending_commands.push("InterruptPlayback".into());
        }
    }

    ui.add_space(24.0);
    render_state_info(ui, state);
    ui.add_space(16.0);

    ui.vertical_centered(|ui| {
        render_controls(ui, state);
    });

    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        render_transcript(ui, &state.transcript_lines);
    });
}

/// Render the horizontal navigation bar (legacy — for callers not using render_app).
pub fn render_nav_bar(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    let tk = state.theme.tokens();
    ui.horizontal(|ui| {
        let pages = [
            (Page::Session, prxvc_i18n::t("nav.session")),
            (Page::History, prxvc_i18n::t("nav.history")),
            (Page::Settings, prxvc_i18n::t("nav.settings")),
            (Page::Debug, prxvc_i18n::t("nav.debug")),
            (Page::About, prxvc_i18n::t("nav.about")),
        ];

        // Page tabs are a segmented control: the selected page is raised.
        let labels: Vec<&str> = pages.iter().map(|(_, l)| l.as_str()).collect();
        let selected = pages
            .iter()
            .position(|(p, _)| *p == state.current_page)
            .unwrap_or(0);
        if let Some(i) = widgets::segmented(ui, &tk, selected, &labels) {
            state.current_page = pages[i].0;
        }

        // Network status indicator on the right
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            render_network_indicator(ui, state);
        });
    });
}

/// Render a small network status indicator (legacy).
pub fn render_network_indicator(ui: &mut egui::Ui, state: &DesktopUiState) {
    let tk = state.theme.tokens();
    let color = status_color(&tk, &state.network_status);

    ui.horizontal(|ui| {
        let (_, dot_rect) = ui.allocate_space(egui::vec2(8.0, 8.0));
        ui.painter().circle_filled(dot_rect.center(), 4.0, color);
        let status_text = match state.network_status {
            NetworkStatus::Connected => prxvc_i18n::t("network.connected"),
            NetworkStatus::Disconnected => prxvc_i18n::t("network.disconnected"),
            NetworkStatus::Connecting => prxvc_i18n::t("network.connecting"),
            NetworkStatus::Reconnecting => prxvc_i18n::t_with(
                "network.reconnecting",
                &[&state.reconnect_attempts.to_string()],
            ),
            NetworkStatus::Failed => prxvc_i18n::t("network.failed"),
        };
        ui.label(widgets::text(status_text, size::META, tk.secondary));
    });
}

/// Render the session history page (legacy — for callers not using render_app).
pub fn render_history_page(ui: &mut egui::Ui, state: &DesktopUiState) {
    render_history_panel(ui, &state.history_entries);
}

/// Render the settings page (legacy — for callers not using render_app).
pub fn render_settings_page(ui: &mut egui::Ui, settings: &mut UiSettings) {
    render_settings_panel(ui, settings);
}

/// Render the about page with project information.
pub fn render_about_page(ui: &mut egui::Ui) {
    let tk = theme::tokens_for(ui.ctx());
    ui.label(widgets::text(
        prxvc_i18n::t("app.name"),
        size::TITLE,
        tk.text,
    ));
    ui.add_space(space::S8);
    ui.label(widgets::text(
        format!(
            "{}: {}",
            prxvc_i18n::t("app.version_label"),
            prxvc_i18n::t("app.version")
        ),
        size::SECONDARY,
        tk.secondary,
    ));
    ui.add_space(space::S4);
    ui.label(widgets::text(
        prxvc_i18n::t("app.description"),
        size::SECONDARY,
        tk.secondary,
    ));
    ui.add_space(space::S4);
    ui.label(widgets::text(
        format!(
            "{}: {}",
            prxvc_i18n::t("app.license_label"),
            prxvc_i18n::t("app.license")
        ),
        size::SECONDARY,
        tk.secondary,
    ));
}

// ---------------------------------------------------------------------------
// Main layout assembly — the new entry point
// ---------------------------------------------------------------------------

/// The state pill under the orb: the state label (14) and the hint (12) on a
/// floating-layer surface so the text reads on any desktop behind the
/// transparent window.
struct StatusPill {
    rect: egui::Rect,
    label: std::sync::Arc<egui::Galley>,
    hint: Option<std::sync::Arc<egui::Galley>>,
}

impl StatusPill {
    const PAD_X: f32 = 12.0;
    const PAD_Y: f32 = 8.0;

    fn layout(
        painter: &egui::Painter,
        tk: &Tokens,
        top_center: egui::Pos2,
        label: &str,
        hint: &str,
        max_width: f32,
    ) -> Self {
        let wrap = (max_width - 2.0 * Self::PAD_X).max(60.0);
        let label = painter.layout(
            label.to_owned(),
            FontId::new(size::BUTTON, fonts::medium()),
            tk.text,
            wrap,
        );
        let hint = (!hint.is_empty()).then(|| {
            painter.layout(
                hint.to_owned(),
                FontId::proportional(size::META),
                tk.secondary,
                wrap,
            )
        });
        let inner_w = hint
            .as_ref()
            .map_or(label.size().x, |h| h.size().x.max(label.size().x));
        let inner_h = label.size().y + hint.as_ref().map_or(0.0, |h| h.size().y + space::S4);
        let rect = egui::Rect::from_min_size(
            egui::pos2(top_center.x - inner_w / 2.0 - Self::PAD_X, top_center.y),
            egui::vec2(inner_w + 2.0 * Self::PAD_X, inner_h + 2.0 * Self::PAD_Y),
        );
        Self { rect, label, hint }
    }

    fn paint(self, painter: &egui::Painter, tk: &Tokens) {
        painter.rect(
            self.rect,
            CornerRadius::same(radius::LG),
            tk.overlay,
            hairline(tk.overlay_border),
            StrokeKind::Inside,
        );
        let cx = self.rect.center().x;
        let mut y = self.rect.top() + Self::PAD_Y;
        let label_h = self.label.size().y;
        let label_w = self.label.size().x;
        painter.galley(egui::pos2(cx - label_w / 2.0, y), self.label, tk.text);
        y += label_h + space::S4;
        if let Some(hint) = self.hint {
            painter.galley(egui::pos2(cx - hint.size().x / 2.0, y), hint, tk.secondary);
        }
    }
}

/// Render the floating orb UI (frameless transparent mode).
///
/// The entire window is transparent. Only the orb, state label, transcript,
/// and context-menu popups are visible. The orb is draggable to move the window.
pub fn render_app(ctx: &egui::Context, state: &mut DesktopUiState) {
    let time = ctx.input(|i| i.time);
    state.anim_time = time;

    // Style first, then resolve the RF scheme against what egui now uses
    // (the system light/dark preference for the RF theme).
    theme::ensure_installed(ctx, &state.theme);
    if state.theme.is_rf() {
        let scheme = Scheme::from_egui(ctx.theme());
        if state.theme.scheme != scheme {
            state.theme = Theme::rf(scheme);
        }
    }
    let tk = state.theme.tokens();

    // Transparent background for central panel
    let frame = egui::Frame::NONE.fill(Color32::TRANSPARENT);

    egui::CentralPanel::default()
        .frame(frame)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let center = egui::pos2(rect.center().x, rect.top() + 90.0);

            // === ORB ANIMATION STATE ===
            let base_radius = 40.0;
            let (orb_color, scale) = orb_animation(state, time);
            let radius = base_radius * scale;
            let label_pos = egui::pos2(center.x, center.y + radius + 16.0);
            let label_text = translate_state_human(&state.session_state);
            let hint = state_hint(&state.session_state);

            let pill = StatusPill::layout(
                ui.painter(),
                &tk,
                label_pos,
                &label_text,
                &hint,
                rect.width() - 2.0 * space::S8,
            );
            let below_pill = pill.rect.bottom() + space::S8;

            // === ALLOCATE INTERACTIVE AREAS (requires &mut ui) ===
            let orb_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.5, radius * 2.5));
            let orb_response = ui.allocate_rect(orb_rect, egui::Sense::click_and_drag());

            // End button (only when session active): a quiet destructive action.
            let mut stop_clicked = false;
            let controls_h = if state.session_active {
                layout::CONTROL_HEIGHT_SMALL + space::S8
            } else {
                0.0
            };
            if state.session_active {
                let btn_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.center().x - 80.0, below_pill),
                    egui::vec2(160.0, layout::CONTROL_HEIGHT_SMALL),
                );
                ui.allocate_new_ui(
                    egui::UiBuilder::new()
                        .max_rect(btn_rect)
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                    |ui| {
                        stop_clicked = widgets::button_with(
                            ui,
                            &tk,
                            ButtonKind::DangerQuiet,
                            &prxvc_i18n::t("session.stop"),
                            layout::CONTROL_HEIGHT_SMALL,
                            true,
                        )
                        .clicked();
                    },
                );
            }

            // Transcript card (must be created before painting)
            if !state.transcript_lines.is_empty() {
                let transcript_top = below_pill + controls_h;
                let transcript_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 20.0, transcript_top),
                    egui::pos2(rect.right() - 20.0, rect.bottom() - 10.0),
                );
                if transcript_rect.height() > 24.0 && transcript_rect.width() > 24.0 {
                    ui.painter().rect(
                        transcript_rect,
                        CornerRadius::same(radius::CARD),
                        tk.surface_white,
                        hairline(tk.line),
                        StrokeKind::Inside,
                    );
                    let inner = transcript_rect.shrink(space::S8);
                    let mut child_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(inner)
                            .layout(egui::Layout::top_down(egui::Align::LEFT)),
                    );
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .max_height(inner.height())
                        .show(&mut child_ui, |ui| {
                            for line in &state.transcript_lines {
                                transcript_line_ui(ui, &tk, line, size::META, size::CAPTION);
                                ui.add_space(space::S4 - 1.0);
                            }
                        });
                }
            }

            // === PAINT EVERYTHING (only needs &ui / painter, no more mutations) ===
            let painter = ui.painter();
            paint_orb(painter, &tk, center, radius, orb_color, time, orb_response.hovered());
            if orb_response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            pill.paint(painter, &tk);

            // === HANDLE INTERACTIONS (no borrow conflicts here) ===

            // Click to start/stop/interrupt
            if orb_response.clicked() {
                tracing::info!(
                    session_active = state.session_active,
                    session_state = %state.session_state,
                    "orb clicked"
                );
                if !state.session_active {
                    state.pending_commands.push("CreateSession".into());
                    state.pending_commands.push("StartSession".into());
                } else if state.session_state == "AssistantSpeaking" {
                    state.pending_commands.push("InterruptPlayback".into());
                }
            }

            // Drag to move the frameless window
            if orb_response.dragged() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            // Right-click context menu: a floating menu of 38px items.
            orb_response.context_menu(|ui| {
                ui.set_min_width(180.0);
                ui.spacing_mut().item_spacing.y = 0.0;

                if state.session_active {
                    if widgets::menu_item_colored(
                        ui,
                        &tk,
                        &prxvc_i18n::t("session.stop"),
                        false,
                        tk.danger,
                    )
                    .clicked()
                    {
                        state.pending_commands.push("StopSession".into());
                        ui.close_menu();
                    }
                    ui.add_space(space::S4);
                    ui.separator();
                    ui.add_space(space::S4);
                }

                let menu_items = [
                    (prxvc_i18n::t("settings.title"), "settings"),
                    (prxvc_i18n::t("history.title"), "history"),
                    (prxvc_i18n::t("debug.title"), "debug"),
                ];
                for (label, key) in &menu_items {
                    if widgets::menu_item(ui, &tk, label, false).clicked() {
                        match *key {
                            "settings" => state.windows.request_open(ToolKind::Settings),
                            "history" => state.windows.request_open(ToolKind::History),
                            "debug" => state.windows.request_open(ToolKind::Debug),
                            _ => {}
                        }
                        ui.close_menu();
                    }
                }
                ui.add_space(space::S4);
                ui.separator();
                ui.add_space(space::S4);
                if widgets::menu_item_colored(
                    ui,
                    &tk,
                    &prxvc_i18n::t("common.close"),
                    false,
                    tk.secondary,
                )
                .clicked()
                {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            // End button click
            if stop_clicked {
                state.pending_commands.push("StopSession".into());
            }
        });

    // === TOOL WINDOWS (Settings, History, Debug) ===
    // Each is a native window of its own (see `tool_window`); the orb window
    // is not resized or moved by them.
    let placement = tool_window::Placement::from_root(ctx);

    let current_theme = state.theme.name;
    {
        let DesktopUiState { windows, settings, pending_theme, .. } = &mut *state;
        tool_window::show(ctx, &tk, ToolKind::Settings, &mut windows.settings, placement, |ui| {
            settings_view::render_settings_content(ui, &tk, settings, current_theme, pending_theme);
        });
    }

    let history_entries = state.windows.history.is_open().then(|| state.history_entries.clone());
    if let Some(entries) = history_entries {
        tool_window::show(ctx, &tk, ToolKind::History, &mut state.windows.history, placement, |ui| {
            history_rows(ui, &tk, &entries);
        });
    }

    let debug_snapshot = state
        .windows
        .debug
        .is_open()
        .then(|| (state.debug_state_info.clone(), state.debug_events.clone()));
    if let Some((info, events)) = debug_snapshot {
        tool_window::show(ctx, &tk, ToolKind::Debug, &mut state.windows.debug, placement, |ui| {
            debug_body(ui, &tk, &info, &events);
        });
    }

    // Request continuous repaint for orb animation
    ctx.request_repaint();
}

// ---------------------------------------------------------------------------
// Themed popup rendering helpers
// ---------------------------------------------------------------------------

/// Debug dialog body: the state machine in a `surface` block, then events.
fn debug_body(ui: &mut egui::Ui, tk: &Tokens, state_info: &str, events: &[String]) {
    widgets::section_header(ui, tk, &prxvc_i18n::t("debug.state_machine"), true);
    egui::Frame::NONE
        .fill(tk.surface)
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    egui::RichText::new(state_info)
                        .size(size::SECONDARY)
                        .color(tk.text)
                        .monospace(),
                )
                .wrap(),
            );
        });

    widgets::section_header(
        ui,
        tk,
        &format!("{} ({})", prxvc_i18n::t("debug.events"), events.len()),
        false,
    );
    if events.is_empty() {
        ui.label(widgets::text(
            prxvc_i18n::t("debug.no_events_short"),
            size::SECONDARY,
            tk.secondary,
        ));
    } else {
        ui.spacing_mut().item_spacing.y = space::S4;
        for event in events.iter().rev().take(30) {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(event)
                        .size(size::META)
                        .color(tk.secondary)
                        .monospace(),
                )
                .wrap(),
            );
        }
    }
}

/// Calculate orb color and breathing scale for the current session state.
fn orb_animation(state: &DesktopUiState, time: f64) -> (Color32, f32) {
    let theme = &state.theme;
    match state.session_state.as_str() {
        "Listening" | "UserSpeaking" => {
            let s = 1.0 + 0.06 * (time * 2.5).sin() as f32;
            (theme.orb_listening, s)
        }
        "Recognizing" | "Thinking" => {
            let s = 1.0 + 0.03 * (time * 1.5).sin() as f32 + 0.02 * (time * 2.3).cos() as f32;
            (theme.orb_thinking, s)
        }
        "AssistantSpeaking" => {
            let s = 1.0 + 0.08 * (time * 4.0).sin() as f32;
            (theme.orb_speaking, s)
        }
        _ => {
            let s = 1.0 + 0.03 * (time * 1.0).sin() as f32;
            (theme.orb_idle, s)
        }
    }
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Prelude module re-exporting key types and functions.
pub mod prelude {
    pub use crate::shared::{EventSnapshot, NetworkStatus, SharedAppState, UiSnapshot};
    pub use crate::{
        DesktopUiState, HistoryEntry, Page, Scheme, SessionViewModel, Theme, Tokens,
        TranscriptLine, UiSettings, render_about_page, render_app, render_controls,
        render_debug_page, render_error_banner, render_history_page, render_history_panel,
        render_nav_bar, render_network_indicator, render_orb, render_session_panel,
        render_settings_page, render_settings_panel, render_sidebar, render_state_info,
        render_transcript,
    };
}
