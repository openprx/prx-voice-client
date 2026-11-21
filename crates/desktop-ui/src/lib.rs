//! Desktop UI layer for PRX Voice Client using egui.
//!
//! The UI centers on a living orb that represents the AI's state: the orb, the
//! state pill and the transcript card on a transparent stage, with settings,
//! history and debug in windows of their own.
//!
//! The look follows the RF design standard: see [`theme`] for the tokens and
//! the egui style built from them, and [`widgets`] for the custom widgets.
//!
//! Module map:
//! - [`state`]: view models and `DesktopUiState` with its sync and flush.
//! - [`theme_presets`]: the `Theme` colour presets.
//! - [`app`]: `render_app`, the per-frame composition.
//! - `orb_view`, `status_pill`, `status_hint`, `context_menu`: the orb and its surroundings.
//! - `transcript_view`, `history_view`, `debug_view`, `error_banner`: the content views.
//! - [`call_view`], `call_window_view` and `call_debug_view`: the call-mode view model, the Call
//!   translation window that runs a call, and the raw diagnostics inside the debug window.
//! - [`theme`], [`widgets`] (and `widgets_radio`), [`tool_window`], [`fonts`], [`shared`],
//!   [`repaint`]: tokens, widgets, tool windows, fonts, the engine bridge and the repaint policy.
//! - The settings window: [`settings_window`] (layout and navigation),
//!   [`settings_section`] (the sections), [`settings_view`] (page dispatch) and one
//!   `settings_<section>` file per page, with `settings_rows` as their row vocabulary;
//!   [`settings_data`] (what the app probes), [`settings_feed`] (how it arrives) and
//!   [`settings_command`] (what the window asks of the app).

mod app;
mod call_bubble;
mod call_captions_bar;
mod call_captions_view;
mod call_debug_view;
mod call_timeline_view;
pub mod call_transcript;
pub mod call_view;
mod call_window_details;
mod call_window_footer;
mod call_window_header;
mod call_window_issues;
pub mod call_window_state;
mod call_window_view;
mod context_menu;
mod debug_view;
mod error_banner;
pub mod fonts;
mod history_view;
mod orb_view;
pub mod repaint;
mod settings_about;
mod settings_advanced;
mod settings_assistant;
mod settings_audio;
mod settings_call;
pub mod settings_command;
pub mod settings_data;
pub mod settings_feed;
mod settings_general;
mod settings_permissions;
mod settings_pipeline;
mod settings_privacy;
mod settings_rows;
pub mod settings_section;
mod settings_shortcuts;
pub mod settings_view;
mod settings_voice;
pub mod settings_window;
pub mod shared;
mod state;
mod status_hint;
mod status_pill;
pub mod theme;
mod theme_presets;
pub mod tool_window;
mod transcript_view;
pub mod widgets;
mod widgets_radio;

pub use app::{render_app, render_call_window};
pub use call_view::{ActivityView, CallPanel, CallPhase, Heard, IssueSeverity, IssueView, LaneView};
pub use debug_view::render_debug_page;
pub use error_banner::render_error_banner;
pub use history_view::render_history_panel;
pub use orb_view::render_orb;
pub use settings_command::{RevealTarget, SettingsCommand, SystemPane};
pub use settings_data::SettingsData;
pub use settings_feed::SettingsFeed;
pub use settings_section::SettingsSection;
pub use settings_window::SettingsViewState;
pub use shared::{EventSnapshot, NetworkStatus, SharedAppState, UiSnapshot};
pub use state::{
    DesktopUiState, HistoryEntry, Page, SessionViewModel, TranscriptLine, UiSettings,
};
pub use theme::{Scheme, Tokens};
pub use theme_presets::Theme;
pub use transcript_view::render_transcript;

/// Prelude module re-exporting key types and functions.
pub mod prelude {
    pub use crate::shared::{EventSnapshot, NetworkStatus, SharedAppState, UiSnapshot};
    pub use crate::{
        DesktopUiState, HistoryEntry, Page, Scheme, SessionViewModel, Theme, Tokens,
        TranscriptLine, UiSettings, render_app, render_debug_page, render_error_banner,
        render_history_panel, render_orb, render_transcript,
    };
}
