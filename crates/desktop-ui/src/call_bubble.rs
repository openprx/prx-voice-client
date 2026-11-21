//! One utterance of the captions as a bubble: the translation as the primary text, the source as a smaller second text, the stable part of a live partial in full colour and its tail dimmer, the clause being played highlighted, a clause that was not spoken struck through, and one quiet row under it with the live indicator, the notes and the copy action.
//!
//! The bubble is laid out from galleys measured first, so its size is exact before anything is
//! painted: that is what lets the timeline remember heights and lay out only what is on screen.
//! Text goes in as selectable labels over the painted bubble.

use std::hash::{Hash, Hasher};

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, FontId, Label, Rect, Sense, Stroke, WidgetInfo, pos2, vec2};

use crate::call_transcript::{Activity, Line, Side, Utterance};
use crate::call_view::{CaptionFlag, ClauseStatus, SkipView};
use crate::call_window_state::CaptionSize;
use crate::theme::{Tokens, layout, radius, size, space};

/// Seconds between two utterances of one speaker that start a new group (a new time stamp).
pub(crate) const GROUP_GAP_S: u32 = 60;

/// Height of the row of notes and the copy action under a bubble, and of the group head above it.
const ROW_HEIGHT: f32 = 14.0;

/// The words the bubbles use, resolved once per frame.
pub(crate) struct BubbleText {
    pub me: String,
    pub peer: String,
    pub speaking: String,
    pub translating: String,
    pub playing: String,
    pub not_spoken: String,
    pub retried: String,
    pub copy: String,
    pub copied: String,
    pub flags: [String; 6],
    /// "Not translated: already in <language>" for the language each side's listener hears
    /// (outgoing, incoming).
    pub in_target: [String; 2],
    /// "Own translation heard by the microphone".
    pub own_echo: String,
}

impl BubbleText {
    /// The words for `targets`, the names of the languages heard by (the remote party, the user).
    pub(crate) fn resolve(targets: [&str; 2]) -> Self {
        let t = prxvc_i18n::t;
        Self {
            me: t("callwin.caption.me"),
            peer: t("callwin.caption.peer"),
            speaking: t("callwin.activity.speaking"),
            translating: t("callwin.activity.translating"),
            playing: t("callwin.activity.playing"),
            not_spoken: t("callwin.caption.not_spoken"),
            retried: t("callwin.caption.retried"),
            copy: t("callwin.copy"),
            copied: t("callwin.copied"),
            flags: CaptionFlag::ALL.map(|f| t(f.key())),
            in_target: targets.map(|n| prxvc_i18n::t_with("callwin.caption.in_target", &[n])),
            own_echo: t("callwin.caption.own_echo"),
        }
    }
}

/// How captions are drawn this frame.
pub(crate) struct BubbleOpts<'a> {
    pub size: CaptionSize,
    pub show_source: bool,
    pub text: &'a BubbleText,
    /// The utterance whose "Copied" note is showing.
    pub copied: Option<u64>,
}

/// The colours of a bubble of one side.
struct Palette {
    fill: Color32,
    main: Color32,
    dim: Color32,
    highlight: Color32,
}

fn palette(tk: &Tokens, side: Side) -> Palette {
    match side {
        Side::Me => Palette {
            fill: tk.outgoing,
            main: tk.outgoing_text,
            dim: tk.outgoing_meta,
            highlight: tk.outgoing_quote,
        },
        Side::Peer => Palette {
            fill: tk.incoming,
            main: tk.text,
            dim: tk.incoming_meta,
            highlight: tk.incoming_quote,
        },
    }
}

fn format(px: f32, color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::proportional(px),
        color,
        line_height: Some((px * 1.45).round()),
        ..TextFormat::default()
    }
}

fn job(width: f32) -> LayoutJob {
    let mut j = LayoutJob::default();
    j.wrap.max_width = width;
    j
}

/// Whether a clause's translation is being said or made ready to be said.
fn is_current(l: &Line) -> bool {
    matches!(l.status, ClauseStatus::Playing | ClauseStatus::Synthesizing)
}

/// The primary text of `u`, `None` when there is nothing to show yet.
fn primary(u: &Utterance, p: &Palette, px: f32, width: f32, show_source: bool) -> Option<LayoutJob> {
    let mut j = job(width);
    let mut any = false;
    let gap = |j: &mut LayoutJob, any: bool, text: &str| {
        if any && needs_space(&j.text, text) {
            j.append(" ", 0.0, format(px, p.main));
        }
    };
    for l in &u.lines {
        let t = l.translation.trim();
        if !t.is_empty() {
            gap(&mut j, any, t);
            let mut f = format(px, p.main);
            if is_current(l) {
                f.background = p.highlight;
                f.expand_bg = 1.0;
            }
            j.append(t, 0.0, f);
            any = true;
        } else if l.skip == SkipView::NotSpoken {
            // Not spoken: what was said, struck through.
            gap(&mut j, any, l.source.trim());
            let mut f = format(px, p.dim);
            f.strikethrough = Stroke::new(1.0, p.dim);
            j.append(l.source.trim(), 0.0, f);
            j.append(" ", 0.0, format(px, p.main));
            any = true;
        } else if l.skip == SkipView::AlreadyInTarget {
            gap(&mut j, any, l.source.trim());
            j.append(l.source.trim(), 0.0, format(px, p.main));
            any = true;
        }
    }
    // Nothing translated yet: without the original line, show what was heard, dimmer.
    if !any && !show_source {
        any = append_heard(&mut j, u, p, px);
    }
    if !any && u.lines.iter().any(|l| !l.source.trim().is_empty()) && u.partial.is_none() {
        // Clauses are being translated and the original line is shown below: wait quietly.
        return None;
    }
    any.then_some(j)
}

/// Whether a space belongs between the text so far and `next`: where both sides are Latin text,
/// or after a struck-out piece; Chinese runs on without one.
fn needs_space(so_far: &str, next: &str) -> bool {
    match (so_far.chars().next_back(), next.chars().next()) {
        (Some(a), Some(b)) => a.is_ascii() && b.is_ascii() && !a.is_whitespace(),
        _ => false,
    }
}

/// Append the committed sources of `u` and its live partial: the stable part in full colour, the
/// tail dimmer. Returns whether anything was appended.
fn append_heard(j: &mut LayoutJob, u: &Utterance, p: &Palette, px: f32) -> bool {
    let mut any = false;
    for l in &u.lines {
        let s = l.source.trim();
        // A clause that was skipped is shown struck or as it was in the primary text already.
        if s.is_empty() || l.skip != SkipView::None {
            continue;
        }
        if any && needs_space(&j.text, s) {
            j.append(" ", 0.0, format(px, p.dim));
        }
        j.append(s, 0.0, format(px, p.dim));
        any = true;
    }
    if let Some(part) = &u.partial {
        if any && needs_space(&j.text, &part.text) {
            j.append(" ", 0.0, format(px, p.dim));
        }
        let cut = (0..=part.stable_len.min(part.text.len()))
            .rev()
            .find(|i| part.text.is_char_boundary(*i))
            .unwrap_or(0);
        let (stable, tail) = part.text.split_at(cut);
        if !stable.is_empty() {
            j.append(stable, 0.0, format(px, p.main));
            any = true;
        }
        if !tail.is_empty() {
            j.append(tail, 0.0, format(px, p.dim));
            any = true;
        }
    }
    any
}

/// The original line under the translation: the sources of the clauses and the live partial.
fn secondary(u: &Utterance, p: &Palette, px: f32, width: f32) -> Option<LayoutJob> {
    let mut j = job(width);
    let small = (px - 2.0).max(size::CAPTION);
    append_heard(&mut j, u, p, small).then_some(j)
}

/// The notes row: the live indicator, what was not spoken or retried, flags, and the notice of a
/// clause already in the listener's language.
fn notes(u: &Utterance, o: &BubbleOpts<'_>, p: &Palette, tk: &Tokens, width: f32) -> Option<LayoutJob> {
    let t = o.text;
    let mut j = job(width);
    let mut parts: Vec<(String, Color32)> = Vec::new();
    match u.activity() {
        Activity::Speaking => parts.push((format!("\u{2022} {}", t.speaking), p.dim)),
        Activity::Translating => parts.push((format!("\u{2022} {}", t.translating), p.dim)),
        Activity::Playing => parts.push((format!("\u{2022} {}", t.playing), p.dim)),
        Activity::Done => {}
    }
    if u.lines.iter().any(|l| l.skip == SkipView::NotSpoken) {
        parts.push((t.not_spoken.clone(), p.dim));
    }
    if u.lines.iter().any(|l| l.skip == SkipView::AlreadyInTarget) {
        let side = usize::from(u.side == Side::Peer);
        parts.push((t.in_target[side].clone(), p.dim));
    }
    if u.lines.iter().any(|l| l.skip == SkipView::OwnEcho) {
        parts.push((t.own_echo.clone(), p.dim));
    }
    if u.lines.iter().any(|l| l.retried) {
        parts.push((t.retried.clone(), p.dim));
    }
    for (i, flag) in CaptionFlag::ALL.iter().enumerate() {
        if u.lines.iter().any(|l| l.flags.contains(flag)) {
            parts.push((t.flags[i].clone(), tk.warning));
        }
    }
    for (i, (text, color)) in parts.iter().enumerate() {
        if i > 0 {
            j.append("  ", 0.0, format(size::CAPTION, p.dim));
        }
        j.append(text, 0.0, format(size::CAPTION, *color));
    }
    (!parts.is_empty()).then_some(j)
}

/// What drawing one utterance reports.
pub(crate) struct Drawn {
    /// Total height of the item including its head and notes row.
    pub height: f32,
    /// The user pressed Copy: the text to put on the clipboard.
    pub copy: Option<String>,
}

/// The cache key of an utterance's layout: anything that changes its height changes the key.
pub(crate) fn layout_key(u: &Utterance, width: f32, o: &BubbleOpts<'_>, head: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (u.id, u.rev, width.round() as i32, o.size.px().to_bits(), o.show_source, head).hash(&mut h);
    (prxvc_i18n::current_locale() as u8).hash(&mut h);
    h.finish()
}

/// Draw one utterance in the full width of `ui` and return its height. `head` shows who spoke
/// and when above it (the first of a group).
pub(crate) fn utterance(
    ui: &mut egui::Ui,
    tk: &Tokens,
    u: &Utterance,
    o: &BubbleOpts<'_>,
    head: bool,
) -> Drawn {
    let width = ui.available_width();
    let p = palette(tk, u.side);
    let px = o.size.px();
    let max_bubble = layout::BUBBLE_MAX.min(width * 0.92).max(140.0);
    let inner_max = max_bubble - 2.0 * f32::from(layout::BUBBLE_PAD_X);

    let galley = |j: LayoutJob| ui.painter().layout_job(j);
    let primary_g = primary(u, &p, px, inner_max, o.show_source).map(galley);
    let secondary_g = o
        .show_source
        .then(|| secondary(u, &p, px, inner_max))
        .flatten()
        .map(galley);
    let copy_w = 52.0;
    let notes_g = notes(u, o, &p, tk, (width * 0.8 - copy_w).max(120.0)).map(galley);

    let content_w = [primary_g.as_ref(), secondary_g.as_ref()]
        .into_iter()
        .flatten()
        .map(|g| g.size().x)
        .fold(0.0_f32, f32::max);
    let bubble_w = (content_w + 2.0 * f32::from(layout::BUBBLE_PAD_X)).min(max_bubble);
    let gap = space::S4;
    let text_h: f32 = primary_g.iter().chain(secondary_g.iter()).map(|g| g.size().y).sum::<f32>()
        + if primary_g.is_some() && secondary_g.is_some() { gap } else { 0.0 };
    let has_bubble = text_h > 0.0;
    let bubble_h = if has_bubble {
        text_h + 2.0 * f32::from(layout::BUBBLE_PAD_Y)
    } else {
        0.0
    };
    let notes_h = notes_g.as_ref().map_or(0.0, |g| g.size().y).max(ROW_HEIGHT);
    let head_h = if head { ROW_HEIGHT + space::S2 } else { 0.0 };
    let total = head_h + bubble_h + if has_bubble { gap } else { 0.0 } + notes_h;

    let (rect, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
    let mut copy = None;
    // Labels are put in a child of their own: a `put` at an earlier place would otherwise pull the
    // parent's cursor back up.
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    if !ui.is_rect_visible(rect) {
        return Drawn { height: total, copy };
    }
    let me = u.side == Side::Me;
    let x_of = |w: f32| if me { rect.right() - w } else { rect.left() };
    let mut y = rect.top();

    if head {
        let who = if me { &o.text.me } else { &o.text.peer };
        let label = format!("{who}  {}", crate::call_transcript::mmss(u.at_s));
        let g = ui.painter().layout_no_wrap(label, FontId::proportional(size::CAPTION), tk.secondary);
        let at = pos2(x_of(g.size().x), y);
        cui.put(Rect::from_min_size(at, g.size()), Label::new(g).selectable(false));
        y += head_h;
    }
    let mut bubble_rect = Rect::NOTHING;
    if has_bubble {
        bubble_rect = Rect::from_min_size(pos2(x_of(bubble_w), y), vec2(bubble_w, bubble_h));
        ui.painter()
            .rect_filled(bubble_rect, CornerRadius::same(radius::BUBBLE), p.fill);
        let mut ty = bubble_rect.top() + f32::from(layout::BUBBLE_PAD_Y);
        let tx = bubble_rect.left() + f32::from(layout::BUBBLE_PAD_X);
        for (i, g) in primary_g.iter().chain(secondary_g.iter()).enumerate() {
            if i > 0 {
                ty += gap;
            }
            let r = Rect::from_min_size(pos2(tx, ty), g.size());
            cui.put(r, Label::new(g.clone()));
            ty += g.size().y;
        }
        y = bubble_rect.bottom() + gap;
    }

    // The notes row: notes and the copy action, flush to the side of the speaker.
    let notes_w = notes_g.as_ref().map_or(0.0, |g| g.size().x);
    let row_w = notes_w + if notes_w > 0.0 { space::S8 } else { 0.0 } + copy_w;
    let row_left = x_of(row_w);
    if let Some(g) = &notes_g {
        let at = pos2(row_left, y);
        cui.put(Rect::from_min_size(at, g.size()), Label::new(g.clone()).selectable(false));
    }
    let copy_rect = Rect::from_min_size(pos2(row_left + row_w - copy_w, y), vec2(copy_w, ROW_HEIGHT));
    let id = ui.make_persistent_id(("call_copy", u.id));
    let response = ui.interact(copy_rect, id, Sense::click());
    let label = o.text.copy.as_str();
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
    let row_hot = ui.rect_contains_pointer(rect) || response.has_focus();
    if row_hot {
        let copied = o.copied == Some(u.id);
        let color = if response.hovered() || copied { tk.text } else { tk.secondary };
        let shown = if copied { &o.text.copied } else { &o.text.copy };
        ui.painter().text(
            copy_rect.right_center(),
            Align2::RIGHT_CENTER,
            shown,
            FontId::proportional(size::CAPTION),
            color,
        );
        if response.has_focus() {
            crate::widgets::paint_focus_ring(ui, tk, copy_rect.expand(2.0), radius::XS, false);
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        copy = Some(copy_text(u, o.show_source));
    }
    let _ = bubble_rect;
    Drawn { height: total, copy }
}

/// What the Copy action puts on the clipboard: the translation, and the original under it when
/// it is shown.
pub(crate) fn copy_text(u: &Utterance, with_source: bool) -> String {
    let translation = u.translation_text();
    let source = u.source_text();
    match (translation.is_empty(), with_source && !source.is_empty()) {
        (true, _) => source,
        (false, true) => format!("{translation}\n{source}"),
        (false, false) => translation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_transcript::Transcript;
    use crate::call_view::{CaptionView, LaneView, PartialView};

    fn one(lines: Vec<CaptionView>, partial: Option<PartialView>) -> Utterance {
        let mut t = Transcript::default();
        t.absorb(
            Side::Me,
            &LaneView { clauses: lines, partial, translating: true, ..LaneView::default() },
            0,
        );
        t.items()[0].clone()
    }

    fn section_text<'a>(j: &'a LayoutJob, s: &egui::text::LayoutSection) -> &'a str {
        &j.text[s.byte_range.start.0..s.byte_range.end.0]
    }

    fn cv(index: u16, src: &str, tr: &str, status: ClauseStatus) -> CaptionView {
        CaptionView { utterance: 1, index, source: src.into(), translation: tr.into(), status, ..CaptionView::default() }
    }

    #[test]
    fn copy_gives_the_translation_and_the_original_when_it_is_shown() {
        let u = one(vec![cv(0, "你好", "Hello.", ClauseStatus::Played)], None);
        assert_eq!(copy_text(&u, false), "Hello.");
        assert_eq!(copy_text(&u, true), "Hello.\n你好");
        let heard_only = one(vec![cv(0, "你好", "", ClauseStatus::Translating)], None);
        assert_eq!(copy_text(&heard_only, false), "你好");
    }

    #[test]
    fn the_playing_clause_is_highlighted_and_a_skipped_one_is_struck() {
        let tk = crate::theme::LIGHT;
        let p = palette(&tk, Side::Me);
        let mut skipped = cv(1, "来不及的一句", "", ClauseStatus::Skipped);
        skipped.skip = SkipView::NotSpoken;
        let u = one(
            vec![cv(0, "你好", "Hello.", ClauseStatus::Playing), skipped],
            None,
        );
        let j = primary(&u, &p, 15.0, 300.0, true).expect("text");
        let hello = j.sections.iter().find(|s| section_text(&j, s) == "Hello.").unwrap();
        assert_eq!(hello.format.background, p.highlight);
        let struck = j
            .sections
            .iter()
            .find(|s| section_text(&j, s) == "来不及的一句")
            .unwrap();
        assert_ne!(struck.format.strikethrough, Stroke::NONE);
    }

    #[test]
    fn a_live_partial_has_a_full_colour_stable_part_and_a_dim_tail() {
        let tk = crate::theme::LIGHT;
        let p = palette(&tk, Side::Peer);
        let part = PartialView { utterance: 1, text: "see you at thr".into(), stable_len: 11 };
        let u = one(vec![], Some(part));
        let j = primary(&u, &p, 15.0, 300.0, false).expect("text");
        assert_eq!(j.sections.len(), 2);
        assert_eq!(j.sections[0].format.color, p.main);
        assert_eq!(j.sections[1].format.color, p.dim);
        assert_eq!(section_text(&j, &j.sections[1]), "thr");
        // With the original shown, a partial without translation shows only there.
        assert!(primary(&u, &p, 15.0, 300.0, true).is_none());
        assert!(secondary(&u, &p, 15.0, 300.0).is_some());
    }

    #[test]
    fn a_utterance_with_flags_and_a_retry_has_a_notes_row() {
        let tk = crate::theme::LIGHT;
        let text = BubbleText::resolve(["English", "中文"]);
        let o = BubbleOpts { size: CaptionSize::Medium, show_source: true, text: &text, copied: None };
        let mut c = cv(0, "a", "b", ClauseStatus::Played);
        c.flags = vec![CaptionFlag::Refusal];
        c.retried = true;
        let u = one(vec![c], None);
        let j = notes(&u, &o, &palette(&tk, Side::Me), &tk, 300.0).expect("notes");
        assert!(j.text.contains(&text.flags[3]));
        assert!(j.text.contains(&text.retried));
        let quiet = one(vec![cv(0, "a", "b", ClauseStatus::Played)], None);
        assert!(notes(&quiet, &o, &palette(&tk, Side::Me), &tk, 300.0).is_none());
    }
}
