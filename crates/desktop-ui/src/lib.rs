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
//! - `legacy_pages`: page renderers `render_app` does not call.
//! - [`theme`], [`widgets`], [`settings_view`], [`tool_window`], [`fonts`],
//!   [`shared`], [`repaint`]: tokens, widgets, settings content, tool windows,
//!   fonts, the engine bridge and the repaint policy.

mod app;
mod context_menu;
mod debug_view;
mod error_banner;
pub mod fonts;
mod history_view;
mod legacy_pages;
mod orb_view;
pub mod repaint;
pub mod settings_view;
pub mod shared;
mod state;
mod status_hint;
mod status_pill;
pub mod theme;
mod theme_presets;
pub mod tool_window;
mod transcript_view;
pub mod widgets;

pub use app::render_app;
pub use debug_view::render_debug_page;
pub use error_banner::render_error_banner;
pub use history_view::render_history_panel;
pub use legacy_pages::{
    render_about_page, render_controls, render_history_page, render_nav_bar,
    render_network_indicator, render_session_panel, render_settings_page, render_settings_panel,
    render_sidebar, render_state_info,
};
pub use orb_view::render_orb;
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
        TranscriptLine, UiSettings, render_about_page, render_app, render_controls,
        render_debug_page, render_error_banner, render_history_page, render_history_panel,
        render_nav_bar, render_network_indicator, render_orb, render_session_panel,
        render_settings_page, render_settings_panel, render_sidebar, render_state_info,
        render_transcript,
    };
}
