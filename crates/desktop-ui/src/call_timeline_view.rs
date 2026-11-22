//! The captions of the Call translation window: one scrolling timeline of bubbles, me on one side and the other party on the other.
//!
//! * Follows the newest line while the user is at the end and lets go when the user scrolls up;
//!   a "jump to latest" pill (with the number of lines that began meanwhile) brings it back.
//! * Lays out only the utterances on screen: heights are measured once per layout and the rest
//!   is skipped as empty space, so a conversation of the full bound
//!   ([`Transcript::MAX_UTTERANCES`](crate::call_transcript::Transcript::MAX_UTTERANCES)) costs
//!   what a screenful costs.
//! * An empty timeline says what is going on and what to do next.

use egui::{Align, CornerRadius, FontId, Sense, Stroke, StrokeKind, WidgetInfo, pos2, vec2};

use crate::call_bubble::{self, BubbleOpts, BubbleText, GROUP_GAP_S};
use crate::call_transcript::Side;
use crate::call_window_state::{CallWindowState, CaptionSize, Measured};
use crate::theme::{Tokens, hairline, radius, size, space};
use crate::widgets;

/// Screen height above and below the visible part that is still laid out.
const CULL_MARGIN: f32 = 240.0;

/// How long the "Copied" note shows, in seconds.
const COPIED_FOR: f64 = 1.6;

/// What the empty timeline says: a title and what happens next.
pub(crate) struct EmptyState {
    pub title: String,
    pub body: String,
}

/// Everything the timeline needs besides the window's memory.
pub(crate) struct TimelineArgs<'a> {
    pub size: CaptionSize,
    pub show_source: bool,
    /// The names of the languages heard by the remote party and by the user.
    pub targets: [&'a str; 2],
    pub empty: EmptyState,
}

/// Draw the timeline in the height that is left in `ui`, which the caller bounds.
pub(crate) fn timeline(
    ui: &mut egui::Ui,
    tk: &Tokens,
    w: &mut CallWindowState,
    a: &TimelineArgs<'_>,
) {
    if w.transcript.is_empty() {
        empty_state(ui, tk, &a.empty);
        return;
    }
    let now = ui.ctx().input(|i| i.time);
    let copied = match w.copied {
        Some((id, at)) if now - at < COPIED_FOR => id,
        _ => None,
    };
    let texts = BubbleText::resolve(a.targets);
    let opts = BubbleOpts {
        size: a.size,
        show_source: a.show_source,
        text: &texts,
        copied,
    };
    let want_jump = std::mem::take(&mut w.jump);
    let mut copy_request = None;
    let CallWindowState {
        transcript,
        heights,
        ..
    } = &mut *w;
    // Exactly the height the caller gave: the area neither grows past it nor, as an area does by
    // default, keeps a minimum of its own that would reach into what is below.
    let height = ui.available_height();
    let out = egui::ScrollArea::vertical()
        .id_salt("call_captions")
        .auto_shrink([false, false])
        .max_height(height)
        .min_scrolled_height(height)
        .stick_to_bottom(true)
        .show_viewport(ui, |ui, viewport| {
            ui.spacing_mut().item_spacing.y = space::S4;
            let width = ui.available_width();
            let origin = ui.cursor().top();
            if transcript.forgotten > 0 {
                ui.label(widgets::text(
                    prxvc_i18n::t_with("callwin.forgotten", &[&transcript.forgotten.to_string()]),
                    size::CAPTION,
                    tk.secondary,
                ));
            }
            let mut previous: Option<(Side, u32)> = None;
            for u in transcript.items() {
                let head = previous.is_none_or(|(side, at)| {
                    side != u.side || u.at_s.saturating_sub(at) >= GROUP_GAP_S
                });
                previous = Some((u.side, u.at_s));
                let key = call_bubble::layout_key(u, width, &opts, head);
                let y = ui.cursor().top() - origin;
                if let Some(m) = heights.get(&u.id).filter(|m| m.key == key)
                    && (y + m.height < viewport.min.y - CULL_MARGIN
                        || y > viewport.max.y + CULL_MARGIN)
                {
                    ui.allocate_space(vec2(width, m.height));
                    continue;
                }
                let drawn = call_bubble::utterance(ui, tk, u, &opts, head);
                heights.insert(
                    u.id,
                    Measured {
                        height: drawn.height,
                        key,
                    },
                );
                if let Some(text) = drawn.copy {
                    copy_request = Some((u.id, text));
                }
            }
            if want_jump {
                ui.scroll_to_cursor(Some(Align::BOTTOM));
            }
        });
    let max_offset = (out.content_size.y - out.inner_rect.height()).max(0.0);
    w.at_bottom = out.state.clamped_offset().y >= max_offset - 2.0;
    let newest = w.transcript.items().back().map_or(0, |u| u.id);
    if w.at_bottom || w.unseen_from.is_none() {
        w.unseen_from = Some(newest);
    }
    if let Some((id, text)) = copy_request {
        ui.ctx().copy_text(text);
        w.copied = Some((Some(id), now));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(COPIED_FOR));
    }
    w.unseen = if w.at_bottom {
        0
    } else {
        w.transcript
            .items()
            .iter()
            .filter(|u| Some(u.id) > w.unseen_from)
            .count()
    };
    if !w.at_bottom && jump_pill(ui, tk, out.inner_rect, w.unseen) {
        w.jump = true;
        ui.ctx().request_repaint();
    }
}

/// The "jump to latest" pill over the bottom of the captions: an outlined 32 px pill with an arrow
/// and the number of lines that began while the user was looking elsewhere.
fn jump_pill(ui: &mut egui::Ui, tk: &Tokens, over: egui::Rect, unseen: usize) -> bool {
    let label = prxvc_i18n::t("callwin.jump");
    let text = if unseen > 0 {
        prxvc_i18n::t_with("callwin.jump_n", &[&label, &unseen.to_string()])
    } else {
        label.clone()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(size::META), tk.text);
    let height = crate::theme::layout::CONTROL_HEIGHT;
    let width = galley.size().x + 2.0 * space::S12 + 16.0 + space::S4;
    let rect = egui::Rect::from_center_size(
        pos2(over.center().x, over.bottom() - height / 2.0 - space::S8),
        vec2(width, height),
    );
    let response = ui.interact(
        rect,
        ui.make_persistent_id("call_jump_pill"),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, &label));
    let fill = if response.hovered() {
        tk.surface
    } else {
        tk.surface_white
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(radius::FULL),
        fill,
        hairline(tk.line_strong),
        StrokeKind::Inside,
    );
    // The arrow: a down chevron.
    let c = pos2(rect.left() + space::S12 + 5.0, rect.center().y);
    let stroke = Stroke::new(1.5, tk.text);
    painter.line_segment([c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0)], stroke);
    painter.line_segment([c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)], stroke);
    painter.galley(
        pos2(
            rect.left() + space::S12 + 16.0 + space::S4,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        tk.text,
    );
    if response.has_focus() {
        widgets::paint_focus_ring(ui, tk, rect, radius::FULL, false);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

/// The empty timeline: what is going on and what to do, centred and quiet.
fn empty_state(ui: &mut egui::Ui, tk: &Tokens, e: &EmptyState) {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let width = (rect.width() - 2.0 * space::S24).clamp(120.0, 420.0);
    let title = ui.painter().layout(
        e.title.clone(),
        FontId::new(size::BODY, crate::fonts::medium()),
        tk.text,
        width,
    );
    let body = ui.painter().layout(
        e.body.clone(),
        FontId::proportional(size::SECONDARY),
        tk.secondary,
        width,
    );
    let total = title.size().y + space::S8 + body.size().y;
    let top = rect.center().y - total / 2.0;
    let center_x = rect.center().x;
    let title_pos = pos2(
        center_x - title.size().x / 2.0,
        top.max(rect.top() + space::S16),
    );
    let body_pos = pos2(
        center_x - body.size().x / 2.0,
        title_pos.y + title.size().y + space::S8,
    );
    // Labels, so that the words are in the accessibility tree.
    let (title_size, body_size) = (title.size(), body.size());
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    cui.put(
        egui::Rect::from_min_size(title_pos, title_size),
        egui::Label::new(title).selectable(false),
    );
    cui.put(
        egui::Rect::from_min_size(body_pos, body_size),
        egui::Label::new(body).selectable(false),
    );
}
