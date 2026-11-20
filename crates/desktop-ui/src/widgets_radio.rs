//! The standard's radio list: one-of-many where each option has a description (RF 2.2).
//!
//! An outlined box (`line-strong` border, radius `md`) holding rows of a radio glyph (18 by 18,
//! on: 1.5px `primary` ring and a 9px `primary` dot; off: `input` fill and a 1.5px `control`
//! ring), a label (15) and a description (13, `secondary`). The standard has no focus state for
//! the radio; the focus ring follows the segmented control and is drawn on the row.

use egui::{Align, CornerRadius, Layout, Rect, Sense, Stroke, Ui, WidgetInfo, pos2, vec2};

use crate::theme::{Tokens, border, hairline, radius, size, space};
use crate::widgets::{paint_focus_ring, text, text_medium};

/// Diameter of the radio glyph.
const GLYPH: f32 = 18.0;
/// Diameter of the dot of a selected radio.
const DOT: f32 = 9.0;

/// A radio list. `options` are `(label, description)`. Returns the index of a
/// newly chosen option. `enabled = false` draws the list read-only: the
/// glyphs keep their state, nothing reacts.
pub fn radio_list(
    ui: &mut Ui,
    tk: &Tokens,
    id_salt: &str,
    options: &[(String, String)],
    selected: usize,
    enabled: bool,
) -> Option<usize> {
    let mut chosen = None;
    let outer = ui.available_width();
    let frame = egui::Frame::NONE
        .stroke(hairline(tk.line_strong))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(egui::Margin::symmetric(space::S12 as i8, space::S4 as i8));
    frame.show(ui, |ui| {
        let row_width = outer - 2.0 * space::S12 - 2.0;
        ui.set_width(row_width);
        ui.spacing_mut().item_spacing.y = 0.0;
        let last = options.len().saturating_sub(1);
        for (i, (label, description)) in options.iter().enumerate() {
            let sense = if enabled {
                Sense::click()
            } else {
                Sense::hover()
            };
            let id = ui.make_persistent_id((id_salt, i));
            ui.push_id(id, |ui| {
                let text_width = ui.available_width() - GLYPH - space::S12;
                let row_top = ui.cursor().top();
                ui.add_space(space::S12);
                let inner = ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S12;
                    let (glyph, _) = ui.allocate_exact_size(vec2(GLYPH, GLYPH), Sense::hover());
                    ui.allocate_ui_with_layout(
                        vec2(text_width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(text_width);
                            ui.spacing_mut().item_spacing.y = space::S4;
                            ui.add(
                                egui::Label::new(text_medium(label, size::BODY, tk.text)).wrap(),
                            );
                            ui.add(
                                egui::Label::new(text(description, size::SECONDARY, tk.secondary))
                                    .wrap(),
                            );
                        },
                    );
                    glyph
                });
                ui.add_space(space::S12);
                let glyph_rect = inner.inner;
                let row = Rect::from_min_max(
                    pos2(ui.min_rect().left(), row_top),
                    pos2(ui.min_rect().left() + row_width, ui.cursor().top()),
                );
                let response = ui.interact(row, id.with("row"), sense);
                let on = i == selected;
                response.widget_info(|| {
                    WidgetInfo::selected(egui::Role::RadioButton, enabled, on, label)
                });
                if enabled && response.clicked() && !on {
                    chosen = Some(i);
                }
                paint_glyph(ui, tk, glyph_rect, on, enabled);
                if enabled && response.has_focus() {
                    paint_focus_ring(ui, tk, row.shrink(1.0), radius::SM, false);
                }
                if enabled && response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if i != last {
                    let y = ui.cursor().top();
                    ui.painter()
                        .hline(row.left()..=row.right(), y, hairline(tk.line));
                }
            });
        }
    });
    chosen
}

fn paint_glyph(ui: &Ui, tk: &Tokens, rect: Rect, on: bool, enabled: bool) {
    let center = rect.center();
    let r = GLYPH / 2.0;
    if !enabled {
        ui.painter().circle_filled(center, r, tk.surface);
        ui.painter().circle_stroke(
            center,
            r - 0.75,
            Stroke::new(border::CONTROL, tk.line_strong),
        );
        if on {
            ui.painter().circle_filled(center, DOT / 2.0, tk.control);
        }
        return;
    }
    if on {
        ui.painter().circle_filled(center, r, tk.input);
        ui.painter()
            .circle_stroke(center, r - 0.75, Stroke::new(border::CONTROL, tk.primary));
        ui.painter().circle_filled(center, DOT / 2.0, tk.primary);
    } else {
        ui.painter().circle_filled(center, r, tk.input);
        ui.painter()
            .circle_stroke(center, r - 0.75, Stroke::new(border::CONTROL, tk.control));
    }
}
