//! Small RF-styled widgets that stock egui widgets cannot express.
//!
//! Every function takes the active [`Tokens`] explicitly and paints with the
//! standard's colours, radii and sizes. Interaction states follow the
//! standard: hover and pressed use `surface` / `selected` (the standard's
//! proposed values), focus is a 2px `focus` ring that replaces the border of
//! outlined controls and sits 2px outside the edge of filled controls, and a
//! disabled control is a dedicated variant (fill `surface`, label `control`),
//! never a dimmed copy.

use egui::{
    Align, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Layout, Margin, Pos2,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, pos2,
    vec2,
};

use crate::fonts;
use crate::theme::{Tokens, border, hairline, layout, mix, radius, shadow_overlay, size, space};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Plain text at a token size in a token colour.
///
/// Weight follows the standard's scale: `subheading` and larger (headings,
/// titles) are weight 600, everything else is weight 400. Use
/// [`text_medium`] for weight 500.
pub fn text(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    let family = if size >= size::SUBHEADING {
        fonts::semibold()
    } else {
        FontFamily::Proportional
    };
    RichText::new(s)
        .font(FontId::new(size, family))
        .color(color)
}

/// Text at weight 500 (row titles, button-like labels).
pub fn text_medium(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s)
        .font(FontId::new(size, fonts::medium()))
        .color(color)
}

/// Paint the focus ring around `rect`.
///
/// Outlined and unfilled controls: 2px ring replacing the border (inside the
/// edge). Filled controls: 2px ring drawn 2px outside the edge, so it stays
/// visible against the surrounding ground.
pub fn paint_focus_ring(ui: &Ui, tk: &Tokens, rect: Rect, corner: u8, filled: bool) {
    let stroke = Stroke::new(border::FOCUS, tk.focus);
    if filled {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(corner.saturating_add(2)),
            stroke,
            StrokeKind::Outside,
        );
    } else {
        ui.painter()
            .rect_stroke(rect, CornerRadius::same(corner), stroke, StrokeKind::Inside);
    }
}

fn pill_radius(rect: Rect) -> CornerRadius {
    CornerRadius::same((rect.height() / 2.0).min(255.0) as u8)
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// Button hierarchy of the standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one near-black button of a region.
    Primary,
    /// White outlined default button.
    Secondary,
    /// Solid red; only the confirm button of a destructive dialog.
    Danger,
    /// Quiet destructive action on a row or banner.
    DangerQuiet,
    /// No fill, text label.
    Text,
}

/// Colours of one button in one state.
struct ButtonColors {
    fill: Color32,
    stroke: Option<Color32>,
    label: Color32,
}

fn button_colors(
    tk: &Tokens,
    kind: ButtonKind,
    enabled: bool,
    hovered: bool,
    pressed: bool,
) -> ButtonColors {
    if !enabled {
        // `Button / Disabled`: fill `surface`, label `control`.
        return ButtonColors {
            fill: tk.surface,
            stroke: None,
            label: tk.control,
        };
    }
    match kind {
        ButtonKind::Primary => ButtonColors {
            fill: if pressed {
                mix(tk.primary, tk.on_primary, 0.22)
            } else if hovered {
                mix(tk.primary, tk.on_primary, 0.12)
            } else {
                tk.primary
            },
            stroke: None,
            label: tk.on_primary,
        },
        ButtonKind::Secondary => ButtonColors {
            fill: if pressed {
                tk.selected
            } else if hovered {
                tk.surface
            } else {
                tk.surface_white
            },
            stroke: Some(tk.text),
            label: tk.text,
        },
        ButtonKind::Danger => ButtonColors {
            fill: if pressed {
                mix(tk.danger, tk.on_danger, 0.22)
            } else if hovered {
                mix(tk.danger, tk.on_danger, 0.12)
            } else {
                tk.danger
            },
            stroke: None,
            label: tk.on_danger,
        },
        ButtonKind::DangerQuiet => ButtonColors {
            fill: if pressed {
                mix(tk.danger_bg, tk.danger, 0.20)
            } else if hovered {
                mix(tk.danger_bg, tk.danger, 0.10)
            } else {
                tk.danger_bg
            },
            stroke: None,
            label: tk.danger,
        },
        ButtonKind::Text => ButtonColors {
            fill: if pressed {
                tk.selected
            } else if hovered {
                tk.surface
            } else {
                Color32::TRANSPARENT
            },
            stroke: None,
            label: tk.text,
        },
    }
}

/// A standard 32px button.
pub fn button(ui: &mut Ui, tk: &Tokens, kind: ButtonKind, label: &str) -> Response {
    button_with(ui, tk, kind, label, layout::CONTROL_HEIGHT, true)
}

/// A button with an explicit height (24 for an inline action inside a dense
/// row) and enabled flag. A disabled button is the dedicated disabled variant
/// and does not react to the pointer.
pub fn button_with(
    ui: &mut Ui,
    tk: &Tokens,
    kind: ButtonKind,
    label: &str,
    height: f32,
    enabled: bool,
) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(size::BUTTON, fonts::medium()),
        Color32::PLACEHOLDER,
    );
    let width = galley.size().x + 2.0 * layout::BUTTON_PAD_X;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, label));

    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let pressed = enabled && response.is_pointer_button_down_on();
        let c = button_colors(tk, kind, enabled, hovered, pressed);
        let corner = CornerRadius::same(radius::BUTTON);
        ui.painter().rect_filled(rect, corner, c.fill);
        if let Some(stroke) = c.stroke {
            ui.painter()
                .rect_stroke(rect, corner, hairline(stroke), StrokeKind::Inside);
        }
        let text_pos = pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(text_pos, galley, c.label);
        if enabled && response.has_focus() {
            let filled = matches!(kind, ButtonKind::Primary | ButtonKind::Danger);
            paint_focus_ring(ui, tk, rect, radius::BUTTON, filled);
        }
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A 32 by 32 icon button showing a text glyph (for example the close cross).
///
/// `label` is the accessible name and the hover text.
pub fn icon_button(ui: &mut Ui, tk: &Tokens, glyph: &str, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(layout::ICON_BUTTON), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let fill = if response.is_pointer_button_down_on() {
            tk.selected
        } else if response.hovered() {
            tk.surface
        } else {
            Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::BUTTON), fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(size::BODY),
            tk.secondary,
        );
        if response.has_focus() {
            paint_focus_ring(ui, tk, rect, radius::BUTTON, false);
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(label)
}

// ---------------------------------------------------------------------------
// Switch
// ---------------------------------------------------------------------------

/// The standard's switch: 36 by 20 track, 16px knob. For a setting that
/// applies immediately. Returns the response; `changed()` is set on toggle.
pub fn switch(ui: &mut Ui, tk: &Tokens, value: &mut bool) -> Response {
    switch_with(ui, tk, value, true)
}

/// Switch with an explicit enabled flag. The disabled look is derived (track
/// `surface` with a `line-strong` outline, knob `control`); the standard does
/// not draw a disabled switch.
pub fn switch_with(ui: &mut Ui, tk: &Tokens, value: &mut bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(layout::SWITCH_WIDTH, layout::SWITCH_HEIGHT), sense);
    if enabled && response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let on = *value;
    response.widget_info(|| WidgetInfo::selected(egui::Role::Switch, enabled, on, ""));

    if ui.is_rect_visible(rect) {
        let (track, knob) = if !enabled {
            (tk.surface, tk.control)
        } else if on {
            (tk.primary, tk.on_primary)
        } else {
            (tk.line_strong, tk.surface_white)
        };
        ui.painter().rect_filled(rect, pill_radius(rect), track);
        if !enabled {
            ui.painter().rect_stroke(
                rect,
                pill_radius(rect),
                hairline(tk.line_strong),
                StrokeKind::Inside,
            );
        }
        let inset = (layout::SWITCH_HEIGHT - layout::SWITCH_KNOB) / 2.0;
        let r = layout::SWITCH_KNOB / 2.0;
        let cx = if on {
            rect.right() - inset - r
        } else {
            rect.left() + inset + r
        };
        ui.painter()
            .circle_filled(pos2(cx, rect.center().y), r, knob);
        if enabled && response.has_focus() {
            // A checked switch is a filled control: ring outside the edge.
            let corner = (layout::SWITCH_HEIGHT / 2.0) as u8;
            paint_focus_ring(ui, tk, rect, corner, on);
        }
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Opacity of a switch that shows a value which cannot be changed right now.
const LOCKED_OPACITY: f32 = 0.5;

/// A switch that shows its real state but cannot be changed now: the normal on or off colours at
/// reduced opacity, with `name` as its accessible name and `why` as the hover text. Unlike [`switch_with`] with `enabled = false`
/// the state stays readable.
pub fn switch_locked(ui: &mut Ui, tk: &Tokens, on: bool, name: &str, why: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(layout::SWITCH_WIDTH, layout::SWITCH_HEIGHT),
        Sense::hover(),
    );
    response.widget_info(|| WidgetInfo::selected(egui::Role::Switch, false, on, name));
    if ui.is_rect_visible(rect) {
        let (track, knob) = if on {
            (tk.primary, tk.on_primary)
        } else {
            (tk.line_strong, tk.surface_white)
        };
        let fade = |c: Color32| c.gamma_multiply(LOCKED_OPACITY);
        ui.painter()
            .rect_filled(rect, pill_radius(rect), fade(track));
        let inset = (layout::SWITCH_HEIGHT - layout::SWITCH_KNOB) / 2.0;
        let r = layout::SWITCH_KNOB / 2.0;
        let cx = if on {
            rect.right() - inset - r
        } else {
            rect.left() + inset + r
        };
        ui.painter()
            .circle_filled(pos2(cx, rect.center().y), r, fade(knob));
    }
    response.on_hover_text(why)
}

// ---------------------------------------------------------------------------
// Segmented control
// ---------------------------------------------------------------------------

/// The standard's segmented control: `surface` track (radius `sm`, padding 2,
/// gap 2) with the selected segment raised on `surface-white` (radius `xs`).
///
/// Returns the index of a newly clicked segment.
pub fn segmented(ui: &mut Ui, tk: &Tokens, selected: usize, labels: &[&str]) -> Option<usize> {
    let font = FontId::proportional(size::SECONDARY);
    let galleys: Vec<_> = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), Color32::PLACEHOLDER)
        })
        .collect();
    let pad = space::S2;
    let seg_h = layout::CONTROL_HEIGHT - 2.0 * pad;
    let seg_widths: Vec<f32> = galleys
        .iter()
        .map(|g| g.size().x + 2.0 * space::S12)
        .collect();
    let total_w = seg_widths.iter().sum::<f32>()
        + pad * 2.0
        + space::S2 * labels.len().saturating_sub(1) as f32;
    let (track, _) = ui.allocate_exact_size(vec2(total_w, layout::CONTROL_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(track, CornerRadius::same(radius::SM), tk.surface);

    let mut clicked = None;
    let mut x = track.left() + pad;
    for (i, galley) in galleys.into_iter().enumerate() {
        let seg = Rect::from_min_size(pos2(x, track.top() + pad), vec2(seg_widths[i], seg_h));
        x += seg_widths[i] + space::S2;
        let id = ui.scope_id().with(("segment", i));
        let response = ui.interact(seg, id, Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(egui::Role::RadioButton, true, i == selected, labels[i])
        });
        let is_selected = i == selected;
        if is_selected {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(radius::XS), tk.surface_white);
        } else if response.is_pointer_button_down_on() {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(radius::XS), tk.selected);
        }
        let color = if is_selected { tk.text } else { tk.secondary };
        let pos = pos2(
            seg.center().x - galley.size().x / 2.0,
            seg.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(pos, galley, color);
        if response.has_focus() {
            paint_focus_ring(ui, tk, seg, radius::XS, false);
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if response.clicked() && !is_selected {
            clicked = Some(i);
        }
    }
    clicked
}

// ---------------------------------------------------------------------------
// Text field and select
// ---------------------------------------------------------------------------

/// Text field: label 13 `secondary` above (gap 6), a 36px box with `input`
/// fill, 1px `input-border`, radius `input`, value 15, optional helper line
/// 12. Focus replaces the border with the 2px focus ring. An invalid field
/// gets the `danger-bg` fill, a 1px `danger` border and the helper as the
/// error message.
pub fn text_field(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    value: &mut String,
    helper: Option<&str>,
    invalid: bool,
) -> Response {
    ui.label(text(label, size::SECONDARY, tk.secondary));
    ui.add_space(space::S6 - ui.spacing().item_spacing.y.min(space::S6));
    let name = label;
    let border_color = if invalid { tk.danger } else { tk.input_border };
    let fill = if invalid { tk.danger_bg } else { tk.input };
    let response = ui
        .scope(|ui| {
            let w = &mut ui.style_mut().visuals.widgets;
            for state in [
                &mut w.inactive,
                &mut w.hovered,
                &mut w.active,
                &mut w.noninteractive,
            ] {
                state.bg_stroke = hairline(border_color);
                state.corner_radius = CornerRadius::same(radius::INPUT);
            }
            ui.add(
                egui::TextEdit::singleline(value)
                    .font(FontId::proportional(size::BODY))
                    .text_color(tk.text)
                    .background_color(fill)
                    .margin(Margin::symmetric(12, 0))
                    .align(egui::Align2::LEFT_CENTER)
                    .desired_width(ui.available_width())
                    .min_size(vec2(0.0, layout::FIELD_HEIGHT)),
            )
            .accessible_name(name)
        })
        .inner;
    if let Some(helper) = helper {
        let color = if invalid { tk.danger } else { tk.secondary };
        ui.add_space(space::S6 - ui.spacing().item_spacing.y.min(space::S6));
        ui.label(text(helper, size::META, color));
    }
    response
}

/// A select: a field-like trigger (36 high, `input` fill, 1px `input-border`,
/// chevron) that opens a menu. The menu is a floating layer (`overlay` fill,
/// 1px `overlay-border`, radius `menu`, shadow); items are 38 high with
/// radius `sm`, hover fill `surface`, and the selected item carries the
/// leading 2px `primary` bar.
///
/// Returns the index of a newly chosen option.
pub fn select(
    ui: &mut Ui,
    tk: &Tokens,
    id_salt: impl egui::AsIdSalt,
    width: f32,
    current: &str,
    options: &[String],
    selected: usize,
) -> Option<usize> {
    select_named(ui, tk, id_salt, width, current, current, options, selected)
}

/// [`select`] with an accessible name that differs from the shown value (the
/// name of the setting it belongs to), so that two selects showing the same
/// value can be told apart.
#[allow(clippy::too_many_arguments)]
pub fn select_named(
    ui: &mut Ui,
    tk: &Tokens,
    id_salt: impl egui::AsIdSalt,
    width: f32,
    name: &str,
    current: &str,
    options: &[String],
    selected: usize,
) -> Option<usize> {
    let id = ui.make_persistent_id(id_salt);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, layout::FIELD_HEIGHT), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(egui::Role::ComboBox, true, name));
    let _ = id;
    let popup_id = egui::Popup::default_response_id(&response);
    // `Popup::menu` toggles on the click when it is shown below; account for
    // that here so the trigger paints its open state in the same frame.
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id) != response.clicked();

    if ui.is_rect_visible(rect) {
        let fill = if response.is_pointer_button_down_on() {
            tk.selected
        } else if response.hovered() || open {
            tk.surface
        } else {
            tk.input
        };
        let corner = CornerRadius::same(radius::INPUT);
        ui.painter().rect_filled(rect, corner, fill);
        ui.painter()
            .rect_stroke(rect, corner, hairline(tk.input_border), StrokeKind::Inside);
        // A value wider than the box ends in an ellipsis instead of running over the edge.
        let mut job = egui::text::LayoutJob::simple_singleline(
            current.to_owned(),
            FontId::proportional(size::BODY),
            Color32::PLACEHOLDER,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 12.0 - 32.0);
        let galley = ui.painter().layout_job(job);
        let tpos = pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(tpos, galley, tk.text);
        paint_chevron_down(ui, pos2(rect.right() - 16.0, rect.center().y), tk.control);
        if response.has_focus() {
            paint_focus_ring(ui, tk, rect, radius::INPUT, false);
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let mut chosen = None;
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .width(width)
        .show(|ui| {
            ui.set_min_width(width - 8.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, option) in options.iter().enumerate() {
                if menu_item(ui, tk, option, i == selected).clicked() {
                    chosen = Some(i);
                }
            }
        });
    chosen
}

fn paint_chevron_down(ui: &Ui, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    let a = pos2(center.x - 4.0, center.y - 2.0);
    let b = pos2(center.x, center.y + 2.0);
    let c = pos2(center.x + 4.0, center.y - 2.0);
    ui.painter().line_segment([a, b], stroke);
    ui.painter().line_segment([b, c], stroke);
}

/// One menu item (38 high, radius `sm`, label 14). Hover fill is `surface`; a
/// selected item has the leading 2px `primary` bar.
pub fn menu_item(ui: &mut Ui, tk: &Tokens, label: &str, selected: bool) -> Response {
    menu_item_colored(ui, tk, label, selected, tk.text)
}

/// A menu item with an explicit label colour (`danger` for a destructive
/// item, `secondary` for a quiet one).
pub fn menu_item_colored(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    selected: bool,
    label_color: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), layout::MENU_ITEM_HEIGHT),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::selected(egui::Role::MenuItem, true, selected, label));
    if ui.is_rect_visible(rect) {
        let corner = CornerRadius::same(radius::SM);
        if response.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, corner, tk.selected);
        } else if response.hovered() || selected {
            ui.painter().rect_filled(rect, corner, tk.surface);
        }
        if selected {
            let bar_h = (rect.height() / 2.0).max(16.0);
            let bar =
                Rect::from_center_size(pos2(rect.left() + 5.0, rect.center().y), vec2(2.0, bar_h));
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), tk.primary);
        }
        let galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            FontId::new(size::BUTTON, fonts::medium()),
            Color32::PLACEHOLDER,
        );
        let tpos = pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(tpos, galley, label_color);
        if response.has_focus() {
            paint_focus_ring(ui, tk, rect, radius::SM, false);
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

// ---------------------------------------------------------------------------
// Settings anatomy: section header, setting row
// ---------------------------------------------------------------------------

/// Section heading (17, `text`) with the standard's 20px space above (pass
/// `first = true` to omit it for the first section of a page).
pub fn section_header(ui: &mut Ui, tk: &Tokens, title: &str, first: bool) {
    if !first {
        ui.add_space(space::S20);
    }
    ui.label(text(title, size::SUBHEADING, tk.text));
    ui.add_space(space::S4);
}

/// One setting per row: label (15) with an optional description (13,
/// `secondary`) on the left and the control on the right. Padding 12 above
/// and below, one 1px `line` divider under the row unless `last`.
///
/// `control_width` is the width reserved for the control; the left block
/// takes the rest and wraps. The control is vertically centred on the left
/// block.
pub fn setting_row(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: Option<&str>,
    control_width: f32,
    last: bool,
    add_control: impl FnOnce(&mut Ui),
) {
    let full_width = ui.available_width();
    ui.add_space(space::S12);

    let left_width = (full_width - control_width - space::S16).max(60.0);
    let left = ui
        .allocate_ui_with_layout(vec2(left_width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(left_width);
            ui.set_min_height(layout::CONTROL_HEIGHT);
            ui.spacing_mut().item_spacing.y = space::S4;
            ui.add(egui::Label::new(text_medium(label, size::BODY, tk.text)).wrap());
            if let Some(description) = description {
                ui.add(egui::Label::new(text(description, size::SECONDARY, tk.secondary)).wrap());
            }
        })
        .response
        .rect;

    let control_rect = Rect::from_min_max(
        pos2(left.right() + space::S16, left.top()),
        pos2(left.left() + full_width, left.bottom()),
    );
    let mut control_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(control_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    control_ui.name_inputs(label, add_control);

    ui.add_space(space::S12);
    if !last {
        let y = ui.cursor().top();
        ui.painter()
            .hline(left.left()..=left.left() + full_width, y, hairline(tk.line));
    }
}

// ---------------------------------------------------------------------------
// Dialog chrome
// ---------------------------------------------------------------------------

/// Frame of a floating dialog: `overlay` fill, 1px `overlay-border`, radius
/// `dialog`, `shadow-overlay`, padding 24.
pub fn dialog_frame(tk: &Tokens) -> Frame {
    Frame::NONE
        .fill(tk.overlay)
        .stroke(hairline(tk.overlay_border))
        .corner_radius(CornerRadius::same(radius::DIALOG))
        .inner_margin(Margin::same(layout::DIALOG_PADDING as i8))
        .shadow(shadow_overlay(tk))
}

/// Dialog header: title (17) on the left and the icon-button close on the
/// right. Returns `true` when close was clicked. `close_label` is the
/// accessible name of the close button.
pub fn dialog_header(ui: &mut Ui, tk: &Tokens, title: &str, close_label: &str) -> bool {
    let mut closed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(layout::ICON_BUTTON);
        ui.label(text(title, size::SUBHEADING, tk.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, tk, "\u{2715}", close_label).clicked() {
                closed = true;
            }
        });
    });
    closed
}

// ---------------------------------------------------------------------------
// Tip bar and chip
// ---------------------------------------------------------------------------

/// Tone of a tip bar or chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Neutral `surface`.
    Neutral,
    /// Success soft fill.
    Success,
    /// Warning soft fill.
    Warning,
    /// Danger soft fill.
    Danger,
}

fn tone_colors(tk: &Tokens, tone: Tone) -> (Color32, Color32) {
    match tone {
        Tone::Neutral => (tk.surface, tk.secondary),
        Tone::Success => (tk.success_bg, tk.success),
        Tone::Warning => (tk.warning_bg, tk.warning),
        Tone::Danger => (tk.danger_bg, tk.danger),
    }
}

/// Tip bar: soft fill, radius `card`, padding 8 / 12, a 16px glyph in the
/// tone colour and text 13 in `text`. `add_actions` fills the right side
/// (right-to-left order).
pub fn tip_bar(
    ui: &mut Ui,
    tk: &Tokens,
    tone: Tone,
    message: &str,
    add_actions: impl FnOnce(&mut Ui),
) {
    let (fill, glyph) = tone_colors(tk, tone);
    Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                ui.painter()
                    .circle_stroke(rect.center(), 7.0, Stroke::new(1.5_f32, glyph));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    if tone == Tone::Neutral { "i" } else { "!" },
                    FontId::proportional(size::CAPTION),
                    glyph,
                );
                ui.label(text(message, size::SECONDARY, tk.text));
                ui.with_layout(Layout::right_to_left(Align::Center), add_actions);
            });
        });
}

/// Status chip: padding 4 / 8, label 12, radius `chip`. Neutral chips say the
/// state in `text` on `surface`; tinted chips use the soft fill with the
/// strong colour for the label.
pub fn chip(ui: &mut Ui, tk: &Tokens, tone: Tone, label: &str) -> Response {
    let (fill, strong) = tone_colors(tk, tone);
    let label_color = if tone == Tone::Neutral {
        tk.text
    } else {
        strong
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(size::META),
        Color32::PLACEHOLDER,
    );
    let desired = galley.size() + vec2(16.0, 8.0);
    let (rect, response) = ui.allocate_exact_size(desired, Sense::hover());
    // The status is read out as text: a chip is the only place some rows say it.
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Label, true, label));
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::CHIP), fill);
        ui.painter().galley(
            pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            label_color,
        );
    }
    response
}
