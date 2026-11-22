//! The outgoing-playback indicator of the Call translation window: whether the user's translated speech is still pending or playing into the virtual microphone, so that the record button of the other application is not released too early.
//!
//! The user cannot hear their own translation, so this row is the only sign that it is still going
//! out. While the call translates it is always there, at one fixed height, in one of three looks:
//! *sending* (warning tone, animated dots) from the first detected speech until the outgoing lane
//! has drained; *done* (success tone, a check mark) once it has; and, after a few quiet seconds,
//! *idle* (neutral, no emphasis) with the same words. The state comes from
//! [`LaneView::in_flight`](crate::call_view::LaneView::in_flight), which the engine derives from
//! the same test the graceful stop uses, so "done" is shown only when playback has ended.

use std::time::Duration;

use egui::{Align2, CornerRadius, Pos2, Sense, Stroke, WidgetInfo, pos2, vec2};

use crate::call_view::{CallPanel, CallPhase};
use crate::repaint::IDLE_REPAINT_INTERVAL;
use crate::theme::{Tokens, border, layout, radius, size, space};

/// How long "done" keeps its success look before it settles to the neutral idle look.
pub(crate) const DONE_FADE: Duration = Duration::from_secs(4);

/// What the row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SendState {
    /// The call does not translate: no row.
    Hidden,
    /// Speech is pending or playing.
    Sending,
    /// Nothing is pending and the lane has drained.
    Done,
    /// Done for a while: the same words, neutral look.
    Idle,
}

impl SendState {
    /// The i18n key of the words.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Sending => "callwin.send.sending",
            Self::Hidden | Self::Done | Self::Idle => "callwin.send.done",
        }
    }
}

/// The state for a panel, given how long the outgoing lane has been drained.
pub(crate) fn derive(panel: &CallPanel, drained_for: Duration) -> SendState {
    if panel.phase != CallPhase::Translating {
        SendState::Hidden
    } else if panel.outgoing.in_flight {
        SendState::Sending
    } else if drained_for >= DONE_FADE {
        SendState::Idle
    } else {
        SendState::Done
    }
}

/// Draw the row (nothing when hidden) and ask for the repaints its look needs.
pub(crate) fn indicator(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, drained_for: Duration) {
    let state = derive(panel, drained_for);
    if state == SendState::Hidden {
        return;
    }
    ui.add_space(space::S8);
    let font = egui::FontId::proportional(size::BUTTON);
    let width = ui.available_width();
    let text_width = (width - 2.0 * space::S12 - MARK - space::S8).max(0.0);
    let lay = |key: &str| {
        ui.painter().layout(
            prxvc_i18n::t(key),
            font.clone(),
            egui::Color32::PLACEHOLDER,
            text_width,
        )
    };
    // One height for every state: the tallest of the words, so a wrapped line never shifts the layout.
    let text_h = lay(SendState::Sending.key())
        .size()
        .y
        .max(lay(SendState::Done.key()).size().y);
    let height = (text_h + 2.0 * space::S8).max(layout::CONTROL_HEIGHT);
    let galley = lay(state.key());
    let label = prxvc_i18n::t(state.key());
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Label, true, &label));
    let (fill, strong) = match state {
        SendState::Sending => (tk.warning_bg, tk.warning),
        SendState::Done => (tk.success_bg, tk.success),
        _ => (tk.surface, tk.secondary),
    };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(radius::CARD), fill);
        let centre = pos2(rect.left() + space::S12 + MARK / 2.0, rect.center().y);
        match state {
            SendState::Sending => dots(painter, centre, strong, ui.ctx().input(|i| i.time)),
            SendState::Done => check(painter, centre, strong),
            _ => {
                painter.circle_stroke(centre, MARK / 4.0, Stroke::new(border::CONTROL, strong));
            }
        }
        let at = pos2(rect.left() + space::S12 + MARK + space::S8, rect.center().y);
        let text_rect = Align2::LEFT_CENTER.anchor_size(at, galley.size());
        painter.galley(text_rect.min, galley, strong);
    }
    match state {
        SendState::Sending => ui.ctx().request_repaint_after(IDLE_REPAINT_INTERVAL),
        SendState::Done => ui
            .ctx()
            .request_repaint_after(DONE_FADE.saturating_sub(drained_for)),
        _ => {}
    }
}

/// The width and height of the mark's slot.
const MARK: f32 = space::S16;
/// Dots in the activity mark.
const DOTS: usize = 3;
/// Seconds one pulse takes to run along the dots.
const PULSE_PERIOD: f64 = 1.0;
/// The dimmest alpha of a dot at rest.
const DOT_REST: f32 = 0.3;

/// Three dots that light up one after another.
fn dots(painter: &egui::Painter, centre: Pos2, colour: egui::Color32, time: f64) {
    let step = MARK / DOTS as f32;
    for i in 0..DOTS {
        let phase = (time / PULSE_PERIOD - i as f64 / DOTS as f64).rem_euclid(1.0);
        let wave = (0.5 + 0.5 * (phase * std::f64::consts::TAU).cos()) as f32;
        let alpha = DOT_REST + (1.0 - DOT_REST) * wave;
        let x = centre.x + (i as f32 - (DOTS as f32 - 1.0) / 2.0) * step;
        painter.circle_filled(pos2(x, centre.y), space::S2, colour.gamma_multiply(alpha));
    }
}

/// A check mark in the mark's slot.
fn check(painter: &egui::Painter, centre: Pos2, colour: egui::Color32) {
    let u = MARK / 8.0;
    let stroke = Stroke::new(border::CONTROL, colour);
    let a = centre + vec2(-3.0 * u, 0.0);
    let b = centre + vec2(-u, 2.0 * u);
    let c = centre + vec2(3.0 * u, -2.0 * u);
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, c], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_view::LaneView;

    fn panel(phase: CallPhase, in_flight: bool) -> CallPanel {
        CallPanel {
            phase,
            outgoing: LaneView {
                in_flight,
                ..LaneView::default()
            },
            ..CallPanel::default()
        }
    }

    const NOW: Duration = Duration::ZERO;

    #[test]
    fn pending_or_playing_speech_is_sending() {
        // The engine sets the flag for open speech, a queued clause, playing audio or a queued sink.
        assert_eq!(
            derive(&panel(CallPhase::Translating, true), NOW),
            SendState::Sending
        );
        // Still sending however long the lane was quiet before.
        assert_eq!(
            derive(&panel(CallPhase::Translating, true), DONE_FADE * 5),
            SendState::Sending
        );
    }

    #[test]
    fn a_drained_lane_is_done_and_settles_to_idle_after_the_fade() {
        let drained = panel(CallPhase::Translating, false);
        assert_eq!(derive(&drained, NOW), SendState::Done);
        assert_eq!(
            derive(&drained, DONE_FADE - Duration::from_millis(1)),
            SendState::Done
        );
        assert_eq!(derive(&drained, DONE_FADE), SendState::Idle);
    }

    #[test]
    fn nothing_is_shown_unless_the_call_is_translating() {
        for phase in [
            CallPhase::Idle,
            CallPhase::Initializing,
            CallPhase::AudioReady,
            CallPhase::Paused,
            CallPhase::Degraded,
            CallPhase::Recovering,
            CallPhase::Stopping,
        ] {
            assert_eq!(
                derive(&panel(phase, true), NOW),
                SendState::Hidden,
                "{phase:?}"
            );
            assert_eq!(
                derive(&panel(phase, false), NOW),
                SendState::Hidden,
                "{phase:?}"
            );
        }
    }

    #[test]
    fn each_state_has_words_and_done_and_idle_share_them() {
        let sending = prxvc_i18n::t(SendState::Sending.key());
        let done = prxvc_i18n::t(SendState::Done.key());
        assert_ne!(sending, SendState::Sending.key());
        assert_ne!(done, SendState::Done.key());
        assert_ne!(sending, done);
        assert_eq!(SendState::Idle.key(), SendState::Done.key());
    }
}
