//! History view: the list of past sessions as a panel and as the body of the history tool window.

use crate::state::HistoryEntry;
use crate::theme::{self, Tokens, size, space};
use crate::widgets::{self, Tone};

/// History rows: title, a meta line, and the state as a chip.
pub(crate) fn history_rows(ui: &mut egui::Ui, tk: &Tokens, entries: &[HistoryEntry]) {
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
