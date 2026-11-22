//! The orb's right-click context menu: stop, open a tool window, close the app.

use crate::settings_section::SettingsSection;
use crate::state::DesktopUiState;
use crate::theme::{Tokens, space};
use crate::tool_window::ToolKind;
use crate::widgets;

/// What a menu item opens.
enum MenuTarget {
    /// The settings window at a page.
    Settings(SettingsSection),
    /// Another tool window.
    Tool(ToolKind),
}

/// Attach the context menu to the orb's response. Items push commands or
/// window requests onto `state`.
pub(crate) fn orb_context_menu(
    orb_response: &egui::Response,
    state: &mut DesktopUiState,
    tk: &Tokens,
) {
    // Right-click context menu: a floating menu of 38px items.
    orb_response.context_menu(|ui| {
        ui.set_min_width(180.0);
        ui.spacing_mut().item_spacing.y = 0.0;

        if state.session_active {
            if widgets::menu_item_colored(ui, tk, &prxvc_i18n::t("session.stop"), false, tk.danger)
                .clicked()
            {
                state.pending_commands.push("StopSession".into());
            }
            ui.add_space(space::S4);
            ui.separator();
            ui.add_space(space::S4);
        }

        // The settings entries open the one settings window at a named page.
        let menu_items: [(String, MenuTarget); 6] = [
            // The path to a translated call comes first.
            (
                prxvc_i18n::t("callwin.title"),
                MenuTarget::Tool(ToolKind::Call),
            ),
            (
                prxvc_i18n::t("settings.title"),
                MenuTarget::Settings(SettingsSection::General),
            ),
            (
                prxvc_i18n::t("settings.menu.pipeline"),
                MenuTarget::Settings(SettingsSection::SpeechPipeline),
            ),
            (
                prxvc_i18n::t("settings.menu.permissions"),
                MenuTarget::Settings(SettingsSection::Permissions),
            ),
            (
                prxvc_i18n::t("history.title"),
                MenuTarget::Tool(ToolKind::History),
            ),
            (
                prxvc_i18n::t("debug.title"),
                MenuTarget::Tool(ToolKind::Debug),
            ),
        ];
        for (label, target) in &menu_items {
            if widgets::menu_item(ui, tk, label, false).clicked() {
                match target {
                    MenuTarget::Settings(section) => state.open_settings(Some(*section)),
                    MenuTarget::Tool(kind) => state.windows.request_open(*kind),
                }
            }
        }
        ui.add_space(space::S4);
        ui.separator();
        ui.add_space(space::S4);
        if widgets::menu_item_colored(ui, tk, &prxvc_i18n::t("common.close"), false, tk.secondary)
            .clicked()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
