//! The orb's right-click context menu: stop, open a tool window, close the app.

use crate::state::DesktopUiState;
use crate::theme::{Tokens, space};
use crate::tool_window::ToolKind;
use crate::widgets;

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
            if widgets::menu_item_colored(
                ui,
                tk,
                &prxvc_i18n::t("session.stop"),
                false,
                tk.danger,
            )
            .clicked()
            {
                state.pending_commands.push("StopSession".into());
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
            if widgets::menu_item(ui, tk, label, false).clicked() {
                match *key {
                    "settings" => state.windows.request_open(ToolKind::Settings),
                    "history" => state.windows.request_open(ToolKind::History),
                    "debug" => state.windows.request_open(ToolKind::Debug),
                    _ => {}
                }
            }
        }
        ui.add_space(space::S4);
        ui.separator();
        ui.add_space(space::S4);
        if widgets::menu_item_colored(
            ui,
            tk,
            &prxvc_i18n::t("common.close"),
            false,
            tk.secondary,
        )
        .clicked()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
