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
    undisclosed_light_en_420: "callwin_undisclosed_light_en_420", call_fixtures::undisclosed_panel(), Light, En, 420.0;
    degraded_light_en_420: "callwin_degraded_light_en_420", call_fixtures::degraded_panel(), Light, En, 420.0;
    degraded_dark_zh_420: "callwin_degraded_dark_zh_420", call_fixtures::degraded_panel(), Dark, ZhCn, 420.0;
    paused_light_zh_420: "callwin_paused_light_zh_420", call_fixtures::paused_panel(), Light, ZhCn, 420.0;
    long_scrolled_light_en_420: "callwin_long_scrolled_light_en_420", call_fixtures::long_panel(150), Light, En, 420.0, true;
    long_scrolled_dark_en_760: "callwin_long_scrolled_dark_en_760", call_fixtures::long_panel(150), Dark, En, 760.0, true;
}
