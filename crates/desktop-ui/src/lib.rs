//! Desktop UI layer for PRX Voice Client using egui.
//!
//! The UI centers on a living orb that represents the AI's state. Three zones:
//! - Left sidebar (narrow, ~200px) — navigation + status
//! - Central stage (fills remaining space) — the ORB + state label + transcript
//! - Right panel (slides in) — settings or history

pub mod shared;

pub use shared::{EventSnapshot, NetworkStatus, SharedAppState, UiSnapshot};

use egui::Color32;

// ---------------------------------------------------------------------------
// Color system (warm palette)
// ---------------------------------------------------------------------------

/// Orb colors mapped to session states.
struct OrbColors {
    /// Purple for idle / default state.
    idle: Color32,
    /// Teal for listening / user speaking.
    listening: Color32,
    /// Amber for thinking / recognizing.
    thinking: Color32,
    /// Coral for assistant speaking.
    speaking: Color32,
}

const COLORS: OrbColors = OrbColors {
    idle: Color32::from_rgb(167, 139, 250),
    listening: Color32::from_rgb(94, 234, 212),
    thinking: Color32::from_rgb(251, 191, 36),
    speaking: Color32::from_rgb(248, 113, 113),
};

/// Deep background color for the central stage (kept for backward compat).
#[allow(dead_code)]
const BG_DEEP: Color32 = Color32::from_rgb(15, 14, 19);
/// Warm background for sidebar and panels (kept for backward compat).
#[allow(dead_code)]
const BG_WARM: Color32 = Color32::from_rgb(19, 17, 26);
/// Surface background for widgets (kept for backward compat).
#[allow(dead_code)]
const BG_SURFACE: Color32 = Color32::from_rgb(26, 23, 35);
/// Elevated surface for active/hover elements.
const BG_ELEVATED: Color32 = Color32::from_rgb(34, 30, 46);
/// Primary text color (light lavender).
const TEXT_PRIMARY: Color32 = Color32::from_rgb(237, 233, 246);
/// Softer text color for secondary content.
const TEXT_SOFT: Color32 = Color32::from_rgb(168, 158, 194);
/// Dim text color for hints and labels.
const TEXT_DIM: Color32 = Color32::from_rgb(88, 79, 110);

// ---------------------------------------------------------------------------
// Theme system
// ---------------------------------------------------------------------------

/// Color theme for the UI.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Human-readable theme name.
    pub name: &'static str,
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
    /// Midnight Purple (default warm theme).
    pub fn midnight() -> Self {
        Self {
            name: "Midnight",
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

    /// All available built-in themes.
    pub fn all() -> Vec<Theme> {
        vec![
            Self::midnight(),
            Self::ocean(),
            Self::rose(),
            Self::forest(),
            Self::ember(),
        ]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::midnight()
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
    /// Optional status bar message.
    pub status_message: Option<String>,
    /// Internal animation time tracker.
    anim_time: f64,
    /// Whether the settings popup window is open (frameless mode).
    pub show_settings_window: bool,
    /// Whether the history popup window is open (frameless mode).
    pub show_history_window: bool,
    /// Whether the debug popup window is open (frameless mode).
    pub show_debug_window: bool,
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
            reconnect_attempts: 0,
            status_message: None,
            anim_time: 0.0,
            show_settings_window: false,
            show_history_window: false,
            show_debug_window: false,
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

    // Calculate breathing/pulsing based on state
    let (orb_color, scale_anim) = match state.session_state.as_str() {
        "Listening" | "UserSpeaking" => {
            let s = 1.0 + 0.06 * (time * 2.5).sin() as f32;
            (COLORS.listening, s)
        }
        "Recognizing" | "Thinking" => {
            let s = 1.0
                + 0.03 * (time * 1.5).sin() as f32
                + 0.02 * (time * 2.3).cos() as f32;
            (COLORS.thinking, s)
        }
        "AssistantSpeaking" => {
            let s = 1.0 + 0.08 * (time * 4.0).sin() as f32;
            (COLORS.speaking, s)
        }
        _ => {
            let s = 1.0 + 0.03 * (time * 1.0).sin() as f32;
            (COLORS.idle, s)
        }
    };

    let radius = base_radius * scale_anim;

    // Allocate the clickable area first (requires &mut ui), then paint over it.
    let orb_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
    let response = ui.allocate_rect(orb_rect, egui::Sense::click());

    let painter = ui.painter();

    // Outer glow (large, soft)
    let glow_color =
        Color32::from_rgba_premultiplied(orb_color.r(), orb_color.g(), orb_color.b(), 15);
    painter.circle_filled(center, radius * 2.5, glow_color);

    // Rings
    let ring_alpha = (25.0 * (1.0 - ((time * 0.8).sin() as f32 * 0.5 + 0.5))) as u8;
    let ring_color = Color32::from_rgba_premultiplied(255, 255, 255, ring_alpha);
    painter.circle_stroke(
        center,
        radius * 1.3,
        egui::Stroke::new(1.0, ring_color),
    );
    painter.circle_stroke(
        center,
        radius * 1.6,
        egui::Stroke::new(
            0.5,
            Color32::from_rgba_premultiplied(255, 255, 255, ring_alpha / 2),
        ),
    );

    // Main orb
    painter.circle_filled(center, radius, orb_color);

    // Highlight (top-left shimmer)
    let highlight_center = egui::pos2(center.x - radius * 0.25, center.y - radius * 0.25);
    let highlight_color = Color32::from_rgba_premultiplied(255, 255, 255, 40);
    painter.circle_filled(highlight_center, radius * 0.4, highlight_color);

    // Hover effect
    if response.hovered() {
        painter.circle_stroke(
            center,
            radius + 2.0,
            egui::Stroke::new(
                1.5,
                Color32::from_rgba_premultiplied(255, 255, 255, 30),
            ),
        );
    }

    response
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

/// Render the left sidebar with brand, navigation, and status indicator.
pub fn render_sidebar(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    ui.vertical(|ui| {
        ui.add_space(12.0);

        // Brand
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("PRX")
                    .size(16.0)
                    .color(TEXT_PRIMARY)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("voice")
                    .size(14.0)
                    .color(Color32::from_rgb(196, 181, 253))
                    .italics(),
            );
        });

        ui.add_space(20.0);

        // Nav items
        let items = [
            ("Session", Page::Session),
            ("History", Page::History),
            ("Settings", Page::Settings),
            ("Debug", Page::Debug),
        ];

        for (label, page) in &items {
            let is_active =
                std::mem::discriminant(&state.current_page) == std::mem::discriminant(page);
            let text_color = if is_active { TEXT_PRIMARY } else { TEXT_SOFT };

            let resp = ui.add(
                egui::Button::new(egui::RichText::new(*label).size(13.0).color(text_color))
                    .fill(if is_active {
                        BG_ELEVATED
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(6.0)
                    .min_size(egui::vec2(ui.available_width(), 32.0)),
            );

            if resp.clicked() {
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

        // Status indicator
        ui.horizontal(|ui| {
            let dot_color = match state.network_status {
                NetworkStatus::Connected => Color32::from_rgb(45, 212, 191),
                NetworkStatus::Disconnected => Color32::from_rgb(158, 158, 158),
                NetworkStatus::Reconnecting => Color32::from_rgb(255, 152, 0),
                NetworkStatus::Failed => Color32::from_rgb(244, 67, 54),
            };
            let (_, dot_rect) = ui.allocate_space(egui::vec2(6.0, 6.0));
            ui.painter()
                .circle_filled(dot_rect.center(), 3.0, dot_color);

            let status_text = match state.network_status {
                NetworkStatus::Connected => "Ready",
                NetworkStatus::Disconnected => "Offline",
                NetworkStatus::Reconnecting => "Reconnecting",
                NetworkStatus::Failed => "Failed",
            };
            ui.label(egui::RichText::new(status_text).size(11.0).color(TEXT_DIM));
        });
    });
}

// ---------------------------------------------------------------------------
// State info (below orb)
// ---------------------------------------------------------------------------

/// Render the state label and hint text below the orb.
pub fn render_state_info(ui: &mut egui::Ui, state: &DesktopUiState) {
    ui.vertical_centered(|ui| {
        // State label — large, italic
        let label = translate_state_human(&state.session_state);
        ui.label(egui::RichText::new(label).size(22.0).color(TEXT_SOFT).italics());

        ui.add_space(4.0);

        // Hint
        let hint = match state.session_state.as_str() {
            "" | "Idle" | "Completed" => prxvc_i18n::t("session.hint.idle"),
            "Listening" => prxvc_i18n::t("session.hint.listening"),
            "AssistantSpeaking" => prxvc_i18n::t("session.hint.speaking"),
            _ => String::new(),
        };
        if !hint.is_empty() {
            ui.label(egui::RichText::new(hint).size(12.0).color(TEXT_DIM));
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

/// Render the scrollable transcript at the bottom of the central stage.
pub fn render_transcript(ui: &mut egui::Ui, lines: &[TranscriptLine]) {
    if lines.is_empty() {
        return;
    }

    egui::ScrollArea::vertical()
        .max_height(200.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in lines {
                ui.horizontal(|ui| {
                    let role_color = if line.role == "user" {
                        Color32::from_rgb(94, 234, 212) // teal
                    } else {
                        Color32::from_rgb(252, 165, 165) // coral light
                    };
                    let role_text = if line.role == "user" { "YOU" } else { "AI" };
                    ui.label(
                        egui::RichText::new(role_text)
                            .size(10.0)
                            .color(role_color)
                            .monospace(),
                    );

                    let text_color = if line.is_final {
                        TEXT_PRIMARY
                    } else {
                        TEXT_SOFT
                    };
                    ui.label(egui::RichText::new(&line.text).size(14.0).color(text_color));
                });
                ui.add_space(4.0);
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

    ui.horizontal_centered(|ui| {
        // Interrupt button
        if state.session_state == "AssistantSpeaking"
            && ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Interrupt")
                            .size(12.0)
                            .color(Color32::from_rgb(251, 191, 36)),
                    )
                    .fill(BG_ELEVATED)
                    .corner_radius(16.0),
                )
                .clicked()
        {
            state.pending_commands.push("InterruptPlayback".into());
        }

        // Stop button
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("End")
                        .size(12.0)
                        .color(Color32::from_rgb(248, 113, 113)),
                )
                .fill(BG_ELEVATED)
                .corner_radius(16.0),
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
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new("Settings")
                .size(18.0)
                .color(TEXT_PRIMARY)
                .italics(),
        );
        ui.add_space(16.0);

        // Language section
        ui.label(egui::RichText::new("LANGUAGE").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .selectable_label(settings.language == "en", "English")
                .clicked()
            {
                settings.language = "en".into();
                prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
            }
            if ui
                .selectable_label(settings.language == "zh-CN", "\u{4e2d}\u{6587}")
                .clicked()
            {
                settings.language = "zh-CN".into();
                prxvc_i18n::set_locale(prxvc_i18n::Locale::ZhCn);
            }
        });

        ui.add_space(16.0);
        ui.label(egui::RichText::new("AUDIO").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.checkbox(
            &mut settings.auto_play,
            egui::RichText::new("Auto-play replies").size(13.0).color(TEXT_SOFT),
        );

        ui.add_space(16.0);
        ui.label(egui::RichText::new("PRIVACY").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.checkbox(
            &mut settings.save_history,
            egui::RichText::new("Save history").size(13.0).color(TEXT_SOFT),
        );
        ui.checkbox(
            &mut settings.show_transcript,
            egui::RichText::new("Show transcript").size(13.0).color(TEXT_SOFT),
        );

        ui.add_space(16.0);
        ui.label(egui::RichText::new("SYSTEM").size(10.0).color(TEXT_DIM));
        ui.add_space(4.0);
        ui.checkbox(
            &mut settings.hotkey_enabled,
            egui::RichText::new("Global hotkey").size(13.0).color(TEXT_SOFT),
        );
    });
}

// ---------------------------------------------------------------------------
// History panel (right side)
// ---------------------------------------------------------------------------

/// Render the history panel in the right sidebar.
pub fn render_history_panel(ui: &mut egui::Ui, entries: &[HistoryEntry]) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new("History")
                .size(18.0)
                .color(TEXT_PRIMARY)
                .italics(),
        );
        ui.add_space(16.0);

        if entries.is_empty() {
            ui.label(egui::RichText::new("No sessions yet").size(13.0).color(TEXT_DIM));
            return;
        }

        for entry in entries {
            let _resp = ui.add(
                egui::Button::new(egui::RichText::new(&entry.title).size(13.0).color(TEXT_SOFT))
                    .fill(Color32::TRANSPARENT)
                    .corner_radius(6.0)
                    .min_size(egui::vec2(ui.available_width(), 48.0)),
            );
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&entry.created_at)
                        .size(10.0)
                        .color(TEXT_DIM),
                );
                ui.label(
                    egui::RichText::new(&entry.duration)
                        .size(10.0)
                        .color(TEXT_DIM),
                );
            });
            ui.add_space(2.0);
        }
    });
}

// ---------------------------------------------------------------------------
// Error banner
// ---------------------------------------------------------------------------

/// Render an error banner at the top of the screen if there is an active error.
///
/// Recoverable errors show a retry button. Both include a close button.
pub fn render_error_banner(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    if let Some(ref error) = state.error_message {
        let error_text = error.clone();

        ui.horizontal(|ui| {
            let text_color = Color32::WHITE;
            ui.colored_label(text_color, &error_text);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if state.error_recoverable
                    && ui.button(prxvc_i18n::t("common.retry")).clicked()
                {
                    state.pending_commands.push("RetryConnection".into());
                    state.error_message = None;
                }
                if ui.button(prxvc_i18n::t("common.close")).clicked() {
                    state.error_message = None;
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Debug page
// ---------------------------------------------------------------------------

/// Render the debug / diagnostics page.
pub fn render_debug_page(ui: &mut egui::Ui, state: &DesktopUiState) {
    ui.label(
        egui::RichText::new("Debug")
            .size(18.0)
            .color(TEXT_PRIMARY)
            .italics(),
    );
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("State Machine:")
                .size(12.0)
                .color(TEXT_DIM),
        );
        ui.label(
            egui::RichText::new(&state.debug_state_info)
                .size(12.0)
                .color(TEXT_PRIMARY)
                .strong(),
        );
    });

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Pending Commands:")
                .size(12.0)
                .color(TEXT_DIM),
        );
        ui.label(
            egui::RichText::new(format!("{}", state.pending_commands.len()))
                .size(12.0)
                .color(TEXT_PRIMARY),
        );
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Recent Events:").size(12.0).color(TEXT_DIM));

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if state.debug_events.is_empty() {
                ui.label(
                    egui::RichText::new("No events recorded")
                        .size(11.0)
                        .color(TEXT_DIM),
                );
            } else {
                for event in state.debug_events.iter().rev() {
                    ui.label(egui::RichText::new(event).size(11.0).color(TEXT_SOFT));
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
    ui.horizontal(|ui| {
        let pages = [
            (Page::Session, prxvc_i18n::t("nav.session")),
            (Page::History, prxvc_i18n::t("nav.history")),
            (Page::Settings, prxvc_i18n::t("nav.settings")),
            (Page::Debug, prxvc_i18n::t("nav.debug")),
            (Page::About, prxvc_i18n::t("nav.about")),
        ];

        for (page, label) in &pages {
            let is_current = state.current_page == *page;
            let text = if is_current {
                egui::RichText::new(label.as_str()).strong().underline()
            } else {
                egui::RichText::new(label.as_str())
            };
            if ui.selectable_label(is_current, text).clicked() {
                state.current_page = *page;
            }
        }

        // Network status indicator on the right
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            render_network_indicator(ui, state);
        });
    });
}

/// Render a small network status indicator (legacy).
pub fn render_network_indicator(ui: &mut egui::Ui, state: &DesktopUiState) {
    let (color, label) = match state.network_status {
        NetworkStatus::Connected => (Color32::from_rgb(76, 175, 80), "\u{25cf}"),
        NetworkStatus::Disconnected => (Color32::from_rgb(158, 158, 158), "\u{25cb}"),
        NetworkStatus::Reconnecting => (Color32::from_rgb(255, 152, 0), "\u{25d0}"),
        NetworkStatus::Failed => (Color32::from_rgb(244, 67, 54), "\u{2715}"),
    };

    ui.horizontal(|ui| {
        ui.colored_label(color, label);
        let status_text = match state.network_status {
            NetworkStatus::Connected => prxvc_i18n::t("network.connected"),
            NetworkStatus::Disconnected => prxvc_i18n::t("network.disconnected"),
            NetworkStatus::Reconnecting => prxvc_i18n::t_with(
                "network.reconnecting",
                &[&state.reconnect_attempts.to_string()],
            ),
            NetworkStatus::Failed => prxvc_i18n::t("network.failed"),
        };
        ui.small(status_text);
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
    ui.label(
        egui::RichText::new(prxvc_i18n::t("app.name"))
            .size(18.0)
            .color(TEXT_PRIMARY),
    );
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!("Version: {}", prxvc_i18n::t("app.version")))
            .size(13.0)
            .color(TEXT_SOFT),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(prxvc_i18n::t("app.description"))
            .size(13.0)
            .color(TEXT_SOFT),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("License: {}", prxvc_i18n::t("app.license")))
            .size(13.0)
            .color(TEXT_DIM),
    );
}

// ---------------------------------------------------------------------------
// Main layout assembly — the new entry point
// ---------------------------------------------------------------------------

/// Render the floating orb UI (frameless transparent mode).
///
/// The entire window is transparent. Only the orb, state label, transcript,
/// and context-menu popups are visible. The orb is draggable to move the window.
pub fn render_app(ctx: &egui::Context, state: &mut DesktopUiState) {
    let time = ctx.input(|i| i.time);
    state.anim_time = time;

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
            let ring_alpha = (20.0 * (1.0 + (time * 0.8).sin() as f32) / 2.0) as u8;
            let label_pos = egui::pos2(center.x, center.y + radius + 16.0);
            let label_text = translate_state_human(&state.session_state);

            let hint = match state.session_state.as_str() {
                "" | "Idle" | "Completed" => prxvc_i18n::t("session.hint.idle"),
                "Listening" => prxvc_i18n::t("session.hint.listening"),
                "AssistantSpeaking" => prxvc_i18n::t("session.hint.speaking"),
                _ => String::new(),
            };

            // === ALLOCATE INTERACTIVE AREAS (requires &mut ui) ===
            let orb_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.5, radius * 2.5));
            let orb_response = ui.allocate_rect(orb_rect, egui::Sense::click_and_drag());

            // Transcript sub-UI (must be created before painting)
            let has_transcript = !state.transcript_lines.is_empty();
            if has_transcript {
                let theme = &state.theme;
                let transcript_top = label_pos.y + 40.0;
                let transcript_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 20.0, transcript_top),
                    egui::pos2(rect.right() - 20.0, rect.bottom() - 10.0),
                );

                let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(transcript_rect).layout(egui::Layout::top_down(egui::Align::LEFT)));
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(transcript_rect.height())
                    .show(&mut child_ui, |ui| {
                        for line in &state.transcript_lines {
                            ui.horizontal(|ui| {
                                let role_color = if line.role == "user" {
                                    theme.orb_listening
                                } else {
                                    theme.orb_speaking
                                };
                                let role = if line.role == "user" { "YOU" } else { "AI" };
                                ui.label(egui::RichText::new(role).size(8.0).color(role_color).monospace());
                                let tc = if line.is_final { theme.text_primary } else { theme.text_soft };
                                ui.label(egui::RichText::new(&line.text).size(11.0).color(tc));
                            });
                            ui.add_space(3.0);
                        }
                    });
            }

            // End button (only when session active)
            let btn_resp = if state.session_active {
                let ctrl_y = center.y + radius + 55.0
                    + if has_transcript { 10.0 } else { 0.0 };
                let ctrl_center = egui::pos2(center.x, ctrl_y.min(label_pos.y + 38.0));
                let btn_rect = egui::Rect::from_center_size(ctrl_center, egui::vec2(48.0, 20.0));
                Some((btn_rect, ui.allocate_rect(btn_rect, egui::Sense::click())))
            } else {
                None
            };

            // === PAINT EVERYTHING (only needs &ui / painter, no more mutations) ===
            let painter = ui.painter();

            // Glow
            let glow = Color32::from_rgba_unmultiplied(orb_color.r(), orb_color.g(), orb_color.b(), 20);
            painter.circle_filled(center, radius * 2.2, glow);

            // Animated rings
            painter.circle_stroke(
                center,
                radius * 1.35,
                egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, ring_alpha)),
            );
            painter.circle_stroke(
                center,
                radius * 1.6,
                egui::Stroke::new(0.5, Color32::from_rgba_unmultiplied(255, 255, 255, ring_alpha / 2)),
            );

            // Main orb body
            painter.circle_filled(center, radius, orb_color);

            // Highlight shimmer (top-left)
            let hl = egui::pos2(center.x - radius * 0.2, center.y - radius * 0.2);
            painter.circle_filled(hl, radius * 0.35, Color32::from_rgba_unmultiplied(255, 255, 255, 35));

            // Hover glow
            if orb_response.hovered() {
                painter.circle_stroke(
                    center,
                    radius + 3.0,
                    egui::Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            // State label
            painter.text(
                label_pos,
                egui::Align2::CENTER_TOP,
                &label_text,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
                state.theme.text_soft,
            );

            // Hint text
            if !hint.is_empty() {
                painter.text(
                    egui::pos2(label_pos.x, label_pos.y + 20.0),
                    egui::Align2::CENTER_TOP,
                    &hint,
                    egui::FontId::new(9.0, egui::FontFamily::Proportional),
                    state.theme.text_dim,
                );
            }

            // End button painting
            if let Some((btn_rect, ref resp)) = btn_resp {
                let sp = state.theme.orb_speaking;
                let btn_fill = if resp.hovered() {
                    Color32::from_rgba_unmultiplied(sp.r(), sp.g(), sp.b(), 30)
                } else {
                    Color32::from_rgba_unmultiplied(
                        state.theme.bg_elevated.r(),
                        state.theme.bg_elevated.g(),
                        state.theme.bg_elevated.b(),
                        200,
                    )
                };
                painter.rect_filled(btn_rect, 12.0, btn_fill);
                painter.text(
                    btn_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "End",
                    egui::FontId::new(9.0, egui::FontFamily::Proportional),
                    state.theme.orb_speaking,
                );
            }

            // === HANDLE INTERACTIONS (no borrow conflicts here) ===

            // Click to start/stop/interrupt
            if orb_response.clicked() {
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

            // Right-click context menu (themed)
            orb_response.context_menu(|ui| {
                let theme = &state.theme;
                ui.set_min_width(140.0);
                ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.hovered.bg_fill = theme.bg_elevated;

                if state.session_active {
                    if ui.add(egui::Button::new(
                        egui::RichText::new(prxvc_i18n::t("session.stop")).size(12.0).color(theme.orb_speaking)
                    ).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() {
                        state.pending_commands.push("StopSession".into());
                        ui.close_menu();
                    }
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);
                }

                let menu_items = [
                    (prxvc_i18n::t("settings.title"), "settings"),
                    (prxvc_i18n::t("history.title"), "history"),
                    (prxvc_i18n::t("debug.title"), "debug"),
                ];
                for (label, key) in &menu_items {
                    if ui.add(egui::Button::new(
                        egui::RichText::new(label).size(12.0).color(theme.text_soft)
                    ).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() {
                        match *key {
                            "settings" => state.show_settings_window = true,
                            "history" => state.show_history_window = true,
                            "debug" => state.show_debug_window = true,
                            _ => {}
                        }
                        ui.close_menu();
                    }
                }
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(2.0);
                if ui.add(egui::Button::new(
                    egui::RichText::new(prxvc_i18n::t("common.close")).size(12.0).color(theme.text_dim)
                ).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            // End button click
            if let Some((_, resp)) = btn_resp {
                if resp.clicked() {
                    state.pending_commands.push("StopSession".into());
                }
            }
        });

    // === POPUP WINDOWS (Settings, History, Debug) ===
    // Themed popup windows with custom title bars, rounded corners, and shadow.

    let popup_frame = |theme: &Theme| {
        egui::Frame::NONE
            .fill(theme.bg_warm)
            .corner_radius(10.0)
            .inner_margin(3.0)
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 12,
                spread: 0,
                color: Color32::from_rgba_unmultiplied(0, 0, 0, 100),
            })
    };

    if state.show_settings_window {
        let theme = state.theme.clone();
        egui::Window::new("##settings")
            .default_size([240.0, 340.0])
            .frame(popup_frame(&theme))
            .title_bar(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(prxvc_i18n::t("settings.title")).size(13.0).color(theme.text_primary).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("\u{2715}").size(12.0).color(theme.text_dim)).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE).frame(false)).clicked() {
                            state.show_settings_window = false;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                render_popup_settings(ui, &mut state.settings, &theme, &mut state.pending_theme);
            });
    }

    if state.show_history_window {
        let theme = state.theme.clone();
        let history_entries = state.history_entries.clone();
        egui::Window::new("##history")
            .default_size([280.0, 360.0])
            .frame(popup_frame(&theme))
            .title_bar(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(prxvc_i18n::t("history.title")).size(13.0).color(theme.text_primary).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("\u{2715}").size(12.0).color(theme.text_dim)).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE).frame(false)).clicked() {
                            state.show_history_window = false;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                render_popup_history(ui, &history_entries, &theme);
            });
    }

    if state.show_debug_window {
        let theme = state.theme.clone();
        let debug_state_info = state.debug_state_info.clone();
        let debug_events = state.debug_events.clone();
        egui::Window::new("##debug")
            .default_size([320.0, 380.0])
            .frame(popup_frame(&theme))
            .title_bar(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(prxvc_i18n::t("debug.title")).size(13.0).color(theme.text_primary).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("\u{2715}").size(12.0).color(theme.text_dim)).fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE).frame(false)).clicked() {
                            state.show_debug_window = false;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                render_popup_debug(ui, &debug_state_info, &debug_events, &theme);
            });
    }

    // Request continuous repaint for orb animation
    ctx.request_repaint();
}

// ---------------------------------------------------------------------------
// Themed popup rendering helpers
// ---------------------------------------------------------------------------

/// Render a section header label for popup panels.
fn render_section_header(ui: &mut egui::Ui, title: &str, theme: &Theme) {
    ui.label(
        egui::RichText::new(title)
            .size(9.0)
            .color(theme.text_dim)
            .strong(),
    );
}

/// Render a labeled toggle row (label on left, checkbox on right).
fn render_toggle_row(ui: &mut egui::Ui, label: &str, value: &mut bool, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(theme.text_soft));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(value, "");
        });
    });
}

/// Render the redesigned settings popup content.
fn render_popup_settings(
    ui: &mut egui::Ui,
    settings: &mut UiSettings,
    theme: &Theme,
    pending_theme: &mut Option<String>,
) {
    ui.add_space(8.0);

    // Section: Theme / Appearance
    render_section_header(ui, "APPEARANCE", theme);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for available_theme in Theme::all() {
            let is_selected = theme.name == available_theme.name;
            let resp = ui.add(
                egui::Button::new(
                    egui::RichText::new(available_theme.name)
                        .size(10.0)
                        .color(if is_selected {
                            available_theme.accent
                        } else {
                            theme.text_soft
                        }),
                )
                .fill(if is_selected {
                    available_theme.bg_elevated
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(if is_selected {
                    egui::Stroke::new(1.0, available_theme.accent)
                } else {
                    egui::Stroke::new(0.5, theme.text_dim)
                })
                .corner_radius(12.0)
                .min_size(egui::vec2(0.0, 24.0)),
            );
            if resp.clicked() {
                *pending_theme = Some(available_theme.name.to_owned());
            }
        }
    });

    ui.add_space(12.0);

    // Section: Language
    render_section_header(ui, "LANGUAGE", theme);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let en_selected = settings.language == "en";
        if ui
            .add(egui::SelectableLabel::new(
                en_selected,
                egui::RichText::new("English")
                    .size(12.0)
                    .color(if en_selected {
                        theme.accent
                    } else {
                        theme.text_soft
                    }),
            ))
            .clicked()
        {
            settings.language = "en".into();
            prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
        }
        ui.add_space(4.0);
        let zh_selected = settings.language == "zh-CN";
        if ui
            .add(egui::SelectableLabel::new(
                zh_selected,
                egui::RichText::new("\u{4e2d}\u{6587}")
                    .size(12.0)
                    .color(if zh_selected {
                        theme.accent
                    } else {
                        theme.text_soft
                    }),
            ))
            .clicked()
        {
            settings.language = "zh-CN".into();
            prxvc_i18n::set_locale(prxvc_i18n::Locale::ZhCn);
        }
    });

    ui.add_space(12.0);

    // Section: Audio
    render_section_header(ui, "AUDIO", theme);
    ui.add_space(4.0);
    render_toggle_row(ui, "Auto-play replies", &mut settings.auto_play, theme);

    ui.add_space(12.0);

    // Section: Privacy
    render_section_header(ui, "PRIVACY", theme);
    ui.add_space(4.0);
    render_toggle_row(ui, "Save history", &mut settings.save_history, theme);
    render_toggle_row(ui, "Show transcript", &mut settings.show_transcript, theme);

    ui.add_space(12.0);

    // Section: System
    render_section_header(ui, "SYSTEM", theme);
    ui.add_space(4.0);
    render_toggle_row(ui, "Global hotkey", &mut settings.hotkey_enabled, theme);

    ui.add_space(16.0);

    // Version info
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("PRX Voice v0.1.0")
                .size(10.0)
                .color(theme.text_dim),
        );
    });
}

/// Render the redesigned history popup content.
fn render_popup_history(ui: &mut egui::Ui, entries: &[HistoryEntry], theme: &Theme) {
    ui.add_space(8.0);

    if entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new("No sessions yet")
                    .size(13.0)
                    .color(theme.text_dim)
                    .italics(),
            );
        });
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for entry in entries {
            ui.add_space(2.0);
            egui::Frame::NONE
                .fill(theme.bg_elevated)
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&entry.title)
                            .size(12.0)
                            .color(theme.text_primary),
                    );
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&entry.created_at)
                                .size(9.0)
                                .color(theme.text_dim),
                        );
                        ui.label(
                            egui::RichText::new("\u{00b7}")
                                .size(9.0)
                                .color(theme.text_dim),
                        );
                        ui.label(
                            egui::RichText::new(&entry.duration)
                                .size(9.0)
                                .color(theme.text_dim),
                        );
                        ui.label(
                            egui::RichText::new("\u{00b7}")
                                .size(9.0)
                                .color(theme.text_dim),
                        );
                        let state_color = match entry.state.as_str() {
                            "Completed" => theme.orb_idle,
                            "Failed" => theme.orb_speaking,
                            _ => theme.text_dim,
                        };
                        ui.label(
                            egui::RichText::new(&entry.state)
                                .size(9.0)
                                .color(state_color),
                        );
                    });
                });
        }
    });
}

/// Render the redesigned debug popup content.
fn render_popup_debug(ui: &mut egui::Ui, state_info: &str, events: &[String], theme: &Theme) {
    ui.add_space(8.0);

    // State machine card
    render_section_header(ui, "STATE MACHINE", theme);
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(theme.bg_elevated)
        .corner_radius(6.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(state_info)
                    .size(11.0)
                    .color(theme.accent)
                    .monospace(),
            );
        });

    ui.add_space(12.0);

    // Events
    render_section_header(ui, &format!("EVENTS ({})", events.len()), theme);
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .show(ui, |ui| {
            if events.is_empty() {
                ui.label(
                    egui::RichText::new("No events")
                        .size(10.0)
                        .color(theme.text_dim)
                        .italics(),
                );
            } else {
                for event in events.iter().rev().take(30) {
                    ui.label(
                        egui::RichText::new(event)
                            .size(9.0)
                            .color(theme.text_dim)
                            .monospace(),
                    );
                    ui.add_space(1.0);
                }
            }
        });
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
        DesktopUiState, HistoryEntry, Page, SessionViewModel, Theme, TranscriptLine, UiSettings,
        render_about_page, render_app, render_controls, render_debug_page, render_error_banner,
        render_history_page, render_history_panel, render_nav_bar, render_network_indicator,
        render_orb, render_session_panel, render_settings_page, render_settings_panel,
        render_sidebar, render_state_info, render_transcript,
    };
}
