//! Interaction tests for the settings surface (egui_kittest): the content
//! view on its own, and opening and closing the settings window through
//! `render_app`.
//!
//! `render_app` schedules repaints through the policy in `repaint.rs`.
//! `Harness::run` only waits for immediate repaints, so it settles on an idle
//! orb whether or not the breathing animation is on; the tests that need a
//! still orb set `DesktopUiState::idle_animation` to false.

mod common;

use egui::{Role, Theme, vec2};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{DesktopUiState, UiSettings, settings_view, theme};
use prxvc_i18n::Locale;

const LOCALES: [Locale; 2] = [Locale::En, Locale::ZhCn];

fn still_state() -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st
}

struct ContentState {
    settings: UiSettings,
    pending_theme: Option<String>,
}

fn content_state(use_mock: bool) -> ContentState {
    ContentState {
        settings: UiSettings { use_mock, ..UiSettings::default() },
        pending_theme: None,
    }
}

#[test]
fn every_switch_is_named_and_toggles_in_both_languages() {
    let _guard = common::lock_locale();
    for locale in LOCALES {
        prxvc_i18n::set_locale(locale.clone());
        let mut harness = common::builder(vec2(520.0, 1600.0), Theme::Light).build_ui_state(
            |ui, st: &mut ContentState| {
                let tk = theme::tokens_for(ui.ctx());
                settings_view::render_settings_content(
                    ui,
                    &tk,
                    &mut st.settings,
                    theme::RF_THEME_NAME,
                    &mut st.pending_theme,
                );
            },
            content_state(false),
        );
        // `run` also performs the accessibility check: an unnamed input
        // widget (the switches, the select, the URL field) fails it.
        harness.run();

        let auto_play = prxvc_i18n::t("settings.auto_play");
        let before = harness.state().settings.auto_play;
        harness.get_by_role_and_label(Role::Switch, &auto_play).click();
        harness.run();
        assert_eq!(harness.state().settings.auto_play, !before, "{locale:?}");

        let hotkey = prxvc_i18n::t("settings.hotkey");
        let before = harness.state().settings.hotkey_enabled;
        harness.get_by_role_and_label(Role::Switch, &hotkey).click();
        harness.run();
        assert_eq!(harness.state().settings.hotkey_enabled, !before, "{locale:?}");
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn language_segmented_control_switches_the_ui_language() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let mut harness = common::builder(vec2(520.0, 1600.0), Theme::Light).build_ui_state(
        |ui, st: &mut ContentState| {
            let tk = theme::tokens_for(ui.ctx());
            settings_view::render_settings_content(
                ui,
                &tk,
                &mut st.settings,
                theme::RF_THEME_NAME,
                &mut st.pending_theme,
            );
        },
        content_state(true),
    );
    harness.run();

    let zh = prxvc_i18n::t("settings.language.zh");
    harness.get_by_role_and_label(Role::RadioButton, &zh).click();
    harness.run();
    assert_eq!(harness.state().settings.language, "zh-CN");

    // The same labels are now in Chinese; every glyph must be drawable
    // (egui_kittest panics on a missing glyph).
    harness.get_by_label(&prxvc_i18n::t("settings.section.audio"));
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn settings_window_opens_and_closes_with_escape() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let mut harness = common::builder(vec2(480.0, 760.0), Theme::Light).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
        still_state(),
    );
    harness.run();
    assert!(!harness.state().windows.settings.is_open());
    assert!(harness.query_by_label(&prxvc_i18n::t("settings.appearance")).is_none());

    harness.state_mut().windows.request_open(ToolKind::Settings);
    harness.run();
    assert!(harness.state().windows.settings.is_open());
    harness.get_by_label(&prxvc_i18n::t("settings.appearance"));
    harness.check_accessibility();

    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().windows.settings.is_open(), "Escape closes the window");
    assert!(harness.query_by_label(&prxvc_i18n::t("settings.appearance")).is_none());
}

#[test]
fn idle_orb_settles_with_and_without_the_breathing_animation() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    for idle_animation in [true, false] {
        let mut harness = common::builder(vec2(480.0, 760.0), Theme::Light).build_ui_state(
            |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
            {
                let mut st = DesktopUiState::new();
                st.idle_animation = idle_animation;
                st
            },
        );
        assert!(harness.run_ok().is_some(), "idle_animation={idle_animation} did not settle");
    }
}

#[test]
fn active_orb_keeps_repainting_every_frame() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let mut state = DesktopUiState::new();
    state.session_state = "AssistantSpeaking".to_owned();
    let mut harness = common::builder(vec2(480.0, 760.0), Theme::Light).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
        state,
    );
    assert!(harness.run_ok().is_none(), "an active orb must not settle");
}
