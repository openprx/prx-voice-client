//! Transcript view: how transcript lines are painted, as a scrolling list and as the card on the stage.

use egui::{CornerRadius, StrokeKind};

use crate::state::{DesktopUiState, TranscriptLine};
use crate::theme::{self, Tokens, hairline, radius, size, space};
use crate::widgets;

/// Paint one transcript line: a role word and the text. People and the
/// assistant are told apart by the word, not by hue.
pub(crate) fn transcript_line_ui(
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

/// The transcript card on the stage: a bordered surface from `top` to the
/// bottom of `rect`, holding the scrolling lines. Nothing is drawn when the
/// space left is too small.
pub(crate) fn transcript_card(
    ui: &mut egui::Ui,
    state: &DesktopUiState,
    tk: &Tokens,
    rect: egui::Rect,
    top: f32,
) {
    let transcript_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 20.0, top),
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
                    transcript_line_ui(ui, tk, line, size::META, size::CAPTION);
                    ui.add_space(space::S4 - 1.0);
                }
            });
    }
}
