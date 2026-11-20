//! Interaction tests for the settings surface (egui_kittest): the content
//! view on its own, and opening and closing the settings window through
//! `render_app`.
//!
//! `render_app` requests a repaint every frame, so `Harness::run` would never
//! settle on it: those tests use stepped runs.

mod common;

use egui::{Role, Theme, vec2};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{DesktopUiState, UiSettings, settings_view, theme};
use prxvc_i18n::Locale;

const LOCALES: [Locale; 2] = [Locale::En, Locale::ZhCn];

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
        DesktopUiState::new(),
    );
    harness.run_steps(3);
    assert!(!harness.state().windows.settings.is_open());
    assert!(harness.query_by_label(&prxvc_i18n::t("settings.appearance")).is_none());

    harness.state_mut().windows.request_open(ToolKind::Settings);
    harness.run_steps(4);
    assert!(harness.state().windows.settings.is_open());
    harness.get_by_label(&prxvc_i18n::t("settings.appearance"));
    harness.check_accessibility();

    harness.key_press(egui::Key::Escape);
    harness.run_steps(4);
    assert!(!harness.state().windows.settings.is_open(), "Escape closes the window");
    assert!(harness.query_by_label(&prxvc_i18n::t("settings.appearance")).is_none());
}
