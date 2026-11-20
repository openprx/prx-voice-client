//! Debug view: the diagnostics page and the body of the debug tool window.

use egui::CornerRadius;

use crate::call_view::CallPanel;
use crate::state::DesktopUiState;
use crate::theme::{Tokens, radius, size, space};
use crate::widgets;

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

/// Debug dialog body: the state machine in a `surface` block, then events.
pub(crate) fn debug_body(
    ui: &mut egui::Ui,
    tk: &Tokens,
    state_info: &str,
    events: &[String],
    call: &CallPanel,
    call_translate: &mut bool,
    commands: &mut Vec<String>,
) {
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

    crate::call_debug_view::call_section(ui, tk, call, call_translate, commands);

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
