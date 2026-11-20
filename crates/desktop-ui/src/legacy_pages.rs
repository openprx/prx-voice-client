//! Legacy page renderers (sidebar, nav bar, session panel, controls, about page and the like) that `render_app` does not call.
//!
//! Kept public for the callers and tests that still import them; RF-7 lists
//! them for deletion once the lead confirms which ones go.

use crate::history_view::render_history_panel;
use crate::orb_view::render_orb;
use crate::settings_view;
use crate::shared::NetworkStatus;
use crate::state::{DesktopUiState, Page, UiSettings};
use crate::status_hint::{state_hint, translate_state_human};
use crate::theme::{self, Tokens, size, space};
use crate::transcript_view::render_transcript;
use crate::widgets::{self, ButtonKind};
use egui::Color32;

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
