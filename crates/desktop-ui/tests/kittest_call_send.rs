//! The outgoing-playback indicator of the Call translation window (egui_kittest): both visual
//! states in both languages and themes, no overlap and no layout shift between the states, the
//! row only while the call translates, and the repaints its animation and its flip need.

mod call_fixtures;
mod common;

use common::layout_guard;
use egui::{Rect, Role, Theme, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use prxvc_desktop_ui::{CallPanel, CallPhase, DesktopUiState};
use prxvc_i18n::Locale;

const SIZES: [(f32, f32); 3] = [(320.0, 480.0), (430.0, 630.0), (760.0, 900.0)];

fn harness(
    panel: CallPanel,
    width: f32,
    height: f32,
    scheme: Theme,
) -> egui_kittest::Harness<'static, DesktopUiState> {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.call = panel;
    let seen = st.call.clone();
    st.call_window.sync(&seen);
    st.call_window.began_ago(60);
    let mut h = common::builder(vec2(width, height), scheme).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
        st,
    );
    h.input_mut().system_theme = Some(scheme);
    h.run_steps(4);
    h
}

fn bounds(h: &egui_kittest::Harness<'static, DesktopUiState>, key: &str) -> Option<Rect> {
    let label = prxvc_i18n::t(key);
    let node = h.query_by_role_and_label(Role::Label, &label)?;
    let b = node.accesskit_node().raw_bounds()?;
    Some(Rect::from_min_max(
        egui::pos2(b.x0 as f32, b.y0 as f32),
        egui::pos2(b.x1 as f32, b.y1 as f32),
    ))
}

#[test]
fn sending_and_done_are_each_shown_with_their_words_in_both_languages_and_themes() {
    let _guard = common::lock_locale();
    for scheme in [Theme::Light, Theme::Dark] {
        for locale in [Locale::En, Locale::ZhCn] {
            prxvc_i18n::set_locale(locale);
            let sending = harness(call_fixtures::live_panel(), 430.0, 630.0, scheme);
            sending.check_accessibility();
            assert!(bounds(&sending, "callwin.send.sending").is_some());
            assert!(bounds(&sending, "callwin.send.done").is_none());
            let done = harness(call_fixtures::drained_panel(), 430.0, 630.0, scheme);
            done.check_accessibility();
            assert!(bounds(&done, "callwin.send.done").is_some());
            assert!(bounds(&done, "callwin.send.sending").is_none());
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn the_words_are_the_agreed_ones() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    assert_eq!(
        prxvc_i18n::t("callwin.send.sending"),
        "Sending translation…"
    );
    assert_eq!(
        prxvc_i18n::t("callwin.send.done"),
        "Translation sent — safe to release"
    );
    prxvc_i18n::set_locale(Locale::ZhCn);
    assert_eq!(prxvc_i18n::t("callwin.send.sending"), "正在播放译文…");
    assert_eq!(prxvc_i18n::t("callwin.send.done"), "译文已播完，可以松手");
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn flipping_between_the_states_moves_nothing_and_nothing_overlaps() {
    let _guard = common::lock_locale();
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale);
        for (width, height) in SIZES {
            let mut sending = harness(call_fixtures::live_panel(), width, height, Theme::Light);
            let mut done = harness(call_fixtures::drained_panel(), width, height, Theme::Light);
            let window = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, height));
            layout_guard::assert_clean(&sending, window, "sending");
            layout_guard::assert_clean(&done, window, "done");
            // The indicator's text sits at the same place and the Stop button below the head
            // does not move.
            let a = bounds(&sending, "callwin.send.sending").expect("sending row");
            let b = bounds(&done, "callwin.send.done").expect("done row");
            assert_eq!(a.min.y, b.min.y, "{width}x{height}: the row moved");
            assert_eq!(a.min.x, b.min.x, "{width}x{height}: the row moved");
            let stop = |h: &mut egui_kittest::Harness<'static, DesktopUiState>| {
                let label = prxvc_i18n::t("callwin.stop");
                let node = h.get_by_role_and_label(Role::Button, &label);
                let r = node.accesskit_node().raw_bounds().expect("bounds");
                (r.x0, r.y0, r.x1, r.y1)
            };
            assert_eq!(
                stop(&mut sending),
                stop(&mut done),
                "{width}x{height}: Stop moved"
            );
            // What sits below the row (the captions bar) is at the same height in both.
            let below = |h: &egui_kittest::Harness<'static, DesktopUiState>| {
                layout_guard::items(h)
                    .into_iter()
                    .filter(|i| i.rect.min.y > b.max.y)
                    .map(|i| i.rect.min.y.to_bits())
                    .min()
            };
            assert_eq!(
                below(&sending),
                below(&done),
                "{width}x{height}: content below moved"
            );
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn the_row_is_there_only_while_the_call_translates() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    for phase in [
        CallPhase::Idle,
        CallPhase::Initializing,
        CallPhase::AudioReady,
        CallPhase::Paused,
        CallPhase::Stopping,
    ] {
        let panel = CallPanel {
            phase,
            ..call_fixtures::live_panel()
        };
        let h = harness(panel, 430.0, 630.0, Theme::Light);
        assert!(bounds(&h, "callwin.send.sending").is_none(), "{phase:?}");
        assert!(bounds(&h, "callwin.send.done").is_none(), "{phase:?}");
    }
}

/// The delay until the next repaint egui was asked for after one frame of the window.
fn repaint_delay(panel: CallPanel) -> std::time::Duration {
    use egui::{Event, RawInput, ViewportId};
    let ctx = egui::Context::default();
    prxvc_desktop_ui::fonts::install(&ctx, None);
    prxvc_desktop_ui::theme::install(&ctx);
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.call = panel;
    let seen = st.call.clone();
    st.call_window.sync(&seen);
    let input = |time: f64| RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(430.0, 630.0))),
        time: Some(time),
        events: Vec::<Event>::new(),
        ..Default::default()
    };
    // The first passes size the window and settle its widgets; the last one's request counts.
    let mut delay = std::time::Duration::MAX;
    for time in [0.0, 0.1, 0.2, 0.3] {
        let out = ctx.run_ui(input(time), |ui| {
            prxvc_desktop_ui::render_call_window(ui, &mut st)
        });
        delay = out.viewport_output[&ViewportId::ROOT].repaint_delay;
        out.drop_without_applying_deltas();
    }
    delay
}

#[test]
fn sending_asks_for_frequent_repaints_and_done_for_one_at_the_fade() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let sending = repaint_delay(call_fixtures::live_panel());
    assert!(
        sending <= prxvc_desktop_ui::repaint::IDLE_REPAINT_INTERVAL,
        "sending repaints after {sending:?}"
    );
    let done = repaint_delay(call_fixtures::drained_panel());
    assert!(
        done > prxvc_desktop_ui::repaint::IDLE_REPAINT_INTERVAL && done.as_secs() <= 4,
        "done repaints after {done:?}"
    );
    let idle = repaint_delay(CallPanel::default());
    assert!(
        idle > std::time::Duration::from_secs(4) || idle == std::time::Duration::MAX,
        "{idle:?}"
    );
}
