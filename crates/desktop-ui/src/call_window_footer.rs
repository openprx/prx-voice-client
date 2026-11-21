//! The slim footer of the Call translation window: the two levels and the delay as small quiet indicators, and the switch for the details part, where counters and the rest live.

use egui::{Align, CornerRadius, Layout, Sense, vec2};

use crate::call_view::{CallPanel, CallPhase, LaneView};
use crate::call_window_state::CallWindowState;
use crate::theme::{Tokens, layout, size, space};
use crate::widgets::{self, ButtonKind};

/// Width of a level meter.
const METER_WIDTH: f32 = 48.0;
/// Height of a level meter.
const METER_HEIGHT: f32 = 4.0;
/// The level shown as an empty meter, in dBFS.
const FLOOR_DB: f32 = -60.0;

/// A linear RMS as a position on a meter, 0 to 1 (dBFS from -60).
pub(crate) fn meter_position(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

fn meter(ui: &mut egui::Ui, tk: &Tokens, label: &str, lane: &LaneView) {
    ui.label(widgets::text(label, size::CAPTION, tk.secondary));
    let (rect, _) = ui.allocate_exact_size(vec2(METER_WIDTH, METER_HEIGHT), Sense::hover());
    let round = CornerRadius::same(2);
    ui.painter().rect_filled(rect, round, tk.line);
    let fill = rect.shrink2(vec2(0.0, 0.0));
    let width = fill.width() * meter_position(lane.rms);
    if width > 0.0 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(fill.min, vec2(width, METER_HEIGHT)),
            round,
            tk.secondary,
        );
    }
}

/// The delay of the last finished clause in seconds with one decimal, or a dash.
pub(crate) fn delay_text(panel: &CallPanel) -> String {
    let ms = [&panel.outgoing, &panel.incoming]
        .into_iter()
        .filter_map(|l| l.latency.as_ref())
        .filter_map(|l| l.speech_start_to_first_audio_ms)
        .fold(None::<f64>, |a, v| Some(a.map_or(v, |a| a.max(v))));
    match ms {
        Some(ms) => prxvc_i18n::t_with("callwin.footer.delay", &[&format!("{:.1}", ms / 1000.0)]),
        None => prxvc_i18n::t("callwin.footer.delay_none"),
    }
}

/// The slim row: levels, delay, and the Details switch.
pub(crate) fn footer_row(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, w: &mut CallWindowState) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S8;
        let running = !matches!(panel.phase, CallPhase::Idle | CallPhase::Stopping);
        if running {
            meter(ui, tk, &prxvc_i18n::t("callwin.caption.me"), &panel.outgoing);
            meter(ui, tk, &prxvc_i18n::t("callwin.caption.peer"), &panel.incoming);
            ui.label(widgets::text(delay_text(panel), size::CAPTION, tk.secondary));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let key = if w.details_open { "callwin.details_hide" } else { "callwin.details" };
            if widgets::button_with(
                ui,
                tk,
                ButtonKind::Text,
                &prxvc_i18n::t(key),
                layout::CONTROL_HEIGHT_SMALL,
                true,
            )
            .clicked()
            {
                w.details_open = !w.details_open;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_view::LatencyView;

    #[test]
    fn the_meter_runs_from_minus_sixty_dbfs_to_full_scale() {
        assert_eq!(meter_position(0.0), 0.0);
        assert_eq!(meter_position(0.000_5), 0.0);
        assert!((meter_position(0.1) - (40.0 / 60.0)).abs() < 1e-3);
        assert_eq!(meter_position(1.0), 1.0);
        assert_eq!(meter_position(4.0), 1.0);
    }

    #[test]
    fn the_delay_is_the_slower_direction_and_a_dash_before_any_clause() {
        let _ = delay_text(&CallPanel::default());
        let lane = |ms| LaneView {
            latency: Some(LatencyView {
                speech_start_to_first_audio_ms: Some(ms),
                ..LatencyView::default()
            }),
            ..LaneView::default()
        };
        let panel = CallPanel {
            outgoing: lane(2400.0),
            incoming: lane(3100.0),
            ..CallPanel::default()
        };
        assert!(delay_text(&panel).contains("3.1"));
    }
}
