//! Image snapshots of the settings surface (egui_kittest): light and dark,
//! English and Chinese.
//!
//! The reference machine is the developer Mac the images were created on
//! (rendering depends on the GPU and its driver). Create or update with
//! `UPDATE_SNAPSHOTS=true cargo test -p prxvc-desktop-ui --test kittest_snapshots`
//! and review the PNGs before committing.

mod common;

use egui::{Theme, vec2};
use prxvc_desktop_ui::{UiSettings, settings_view, theme};
use prxvc_i18n::Locale;

struct ContentState {
    settings: UiSettings,
    pending_theme: Option<String>,
}

fn snapshot_settings(name: &str, scheme: Theme, locale: Locale) {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(locale);
    let state = ContentState {
        settings: UiSettings { use_mock: false, ..UiSettings::default() },
        pending_theme: None,
    };
    let mut harness = common::builder(vec2(520.0, 1000.0), scheme)
        .wgpu()
        .build_ui_state(
            |ui, st: &mut ContentState| {
                egui::Frame::NONE
                    .fill(theme::tokens_for(ui.ctx()).bg)
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        let tk = theme::tokens_for(ui.ctx());
                        settings_view::render_settings_content(
                            ui,
                            &tk,
                            &mut st.settings,
                            theme::RF_THEME_NAME,
                            &mut st.pending_theme,
                        );
                    });
            },
            state,
        );
    harness.input_mut().system_theme = Some(scheme);
    harness.run();
    harness.snapshot(name);
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn settings_light_en() {
    snapshot_settings("settings_light_en", Theme::Light, Locale::En);
}

#[test]
fn settings_dark_en() {
    snapshot_settings("settings_dark_en", Theme::Dark, Locale::En);
}

#[test]
fn settings_light_zh() {
    snapshot_settings("settings_light_zh", Theme::Light, Locale::ZhCn);
}

#[test]
fn settings_dark_zh() {
    snapshot_settings("settings_dark_zh", Theme::Dark, Locale::ZhCn);
}
