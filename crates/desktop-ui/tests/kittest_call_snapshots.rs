//! Image snapshots of the Call translation window (egui_kittest): idle, loading, listening, live captions with partials on both sides, degraded with issues, paused and a long conversation scrolled up with the jump pill; in both themes, both languages, a narrow and a wide window.
//!
//! The window content is drawn without the orb (`render_call_window`). Create or update with
//! `UPDATE_SNAPSHOTS=true cargo test -p prxvc-desktop-ui --test kittest_call_snapshots` and look at
//! the PNGs before committing; the reference machine is the developer Mac the images were made on.

mod call_fixtures;
mod common;

use egui::{Theme, vec2};
use prxvc_desktop_ui::{CallPanel, DesktopUiState};
use prxvc_i18n::Locale;

/// The height of a window that shows a whole short conversation, and of one that must scroll.
const TALL: f32 = 960.0;
const SHORT: f32 = 720.0;

fn state_with(panel: CallPanel, elapsed_s: u64) -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.call = panel;
    st.call.translation = prxvc_desktop_ui::call_view::TranslationView {
        engine: "system".into(),
        engine_key: "callwin.engine.system",
        no_context: true,
        actions: Vec::new(),
    };
    // Take the panel in (a start would stamp the time), then make the call look as old as wanted.
    let seen = st.call.clone();
    st.call_window.sync(&seen);
    st.call_window.began_ago(elapsed_s);
    st
}

fn snapshot(name: &str, panel: CallPanel, scheme: Theme, locale: Locale, width: f32, scroll_up: bool) {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(locale);
    let height = if scroll_up { SHORT } else { TALL };
    let mut h = common::builder(vec2(width, height), scheme)
        .wgpu()
        .build_ui_state(
            |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
            state_with(panel, 754),
        );
    h.input_mut().system_theme = Some(scheme);
    h.run_steps(4);
    if scroll_up {
        let over = egui::pos2(width / 2.0, height / 2.0);
        h.input_mut().events.push(egui::Event::PointerMoved(over));
        h.run_steps(2);
        for _ in 0..3 {
            h.input_mut().events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 900.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(3);
        }
    }
    h.run_steps(3);
    h.snapshot(name);
    prxvc_i18n::set_locale(Locale::En);
}

macro_rules! shots {
    ($( $fn:ident : $name:literal, $panel:expr, $scheme:ident, $locale:ident, $width:literal $(, $scroll:literal)? );+ $(;)?) => {$(
        #[test]
        fn $fn() {
            #[allow(unused_mut, unused_assignments)]
            let mut scroll = false;
            $( scroll = $scroll; )?
            snapshot($name, $panel, Theme::$scheme, Locale::$locale, $width, scroll);
        }
    )+};
}

shots! {
    idle_light_en_420: "callwin_idle_light_en_420", CallPanel::default(), Light, En, 420.0;
    idle_dark_zh_420: "callwin_idle_dark_zh_420", CallPanel::default(), Dark, ZhCn, 420.0;
    blocked_light_en_420: "callwin_blocked_light_en_420", call_fixtures::blocked_panel(), Light, En, 420.0;
    loading_light_en_420: "callwin_loading_light_en_420", call_fixtures::loading_panel(), Light, En, 420.0;
    listening_dark_en_420: "callwin_listening_dark_en_420", call_fixtures::listening_panel(), Dark, En, 420.0;
    live_light_en_380: "callwin_live_light_en_380", call_fixtures::live_panel(), Light, En, 380.0;
    live_light_en_760: "callwin_live_light_en_760", call_fixtures::live_panel(), Light, En, 760.0;
    live_dark_en_420: "callwin_live_dark_en_420", call_fixtures::live_panel(), Dark, En, 420.0;
    live_dark_zh_380: "callwin_live_dark_zh_380", call_fixtures::live_panel(), Dark, ZhCn, 380.0;
    live_light_zh_760: "callwin_live_light_zh_760", call_fixtures::live_panel(), Light, ZhCn, 760.0;
    drained_light_en_420: "callwin_drained_light_en_420", call_fixtures::drained_panel(), Light, En, 420.0;
    drained_dark_zh_380: "callwin_drained_dark_zh_380", call_fixtures::drained_panel(), Dark, ZhCn, 380.0;
    undisclosed_light_en_420: "callwin_undisclosed_light_en_420", call_fixtures::undisclosed_panel(), Light, En, 420.0;
    degraded_light_en_420: "callwin_degraded_light_en_420", call_fixtures::degraded_panel(), Light, En, 420.0;
    degraded_dark_zh_420: "callwin_degraded_dark_zh_420", call_fixtures::degraded_panel(), Dark, ZhCn, 420.0;
    paused_light_zh_420: "callwin_paused_light_zh_420", call_fixtures::paused_panel(), Light, ZhCn, 420.0;
    long_scrolled_light_en_420: "callwin_long_scrolled_light_en_420", call_fixtures::long_panel(150), Light, En, 420.0, true;
    long_scrolled_dark_en_760: "callwin_long_scrolled_dark_en_760", call_fixtures::long_panel(150), Dark, En, 760.0, true;
}

/// A call that has ended: its conversation is still in the captions and the window is idle.
fn after_call_state(details_open: bool) -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.call = call_fixtures::live_panel();
    let running = st.call.clone();
    st.call_window.sync(&running);
    st.call = CallPanel { phase: prxvc_desktop_ui::CallPhase::Idle, ..call_fixtures::live_panel() };
    st.call_window.began_ago(754);
    st.call_window.details_open = details_open;
    st
}

fn after_call_snapshot(name: &str, details_open: bool, scheme: Theme, locale: Locale, width: f32, height: f32) {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(locale);
    let mut h = common::builder(vec2(width, height), scheme).wgpu().build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
        after_call_state(details_open),
    );
    h.input_mut().system_theme = Some(scheme);
    h.run_steps(6);
    h.snapshot(name);
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn after_a_call_with_the_details_open_light_zh_430x630() {
    after_call_snapshot("callwin_details_open_light_zh_430x630", true, Theme::Light, Locale::ZhCn, 430.0, 630.0);
}

#[test]
fn after_a_call_with_the_captions_light_zh_430x630() {
    after_call_snapshot("callwin_after_call_light_zh_430x630", false, Theme::Light, Locale::ZhCn, 430.0, 630.0);
}

#[test]
fn after_a_call_with_the_details_open_dark_en_380x480() {
    after_call_snapshot("callwin_details_open_dark_en_380x480", true, Theme::Dark, Locale::En, 380.0, 480.0);
}

#[test]
fn after_a_call_at_the_smallest_size_light_en_320x360() {
    after_call_snapshot("callwin_after_call_light_en_320x360", false, Theme::Light, Locale::En, 320.0, 360.0);
}
