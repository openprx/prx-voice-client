//! The translation engine rows of the settings window (egui_kittest): the dropdown with the two
//! engines, their one-line explanations, the status of the selected engine with its actions, the
//! read-only line on the Speech pipeline page, and both languages.

mod common;

use common::{WindowState, english, lock_locale, window_harness};
use egui::{Role, Theme};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::SettingsSection;
use prxvc_desktop_ui::settings_command::{SettingsCommand, SystemPane};
use prxvc_desktop_ui::settings_data::*;
use prxvc_i18n::{Locale, t};

fn page(engine: &str, data: EngineData) -> egui_kittest::Harness<'static, WindowState> {
    let mut st = WindowState::new(SettingsSection::CallTranslation);
    st.settings.call_translation_engine = engine.into();
    st.data.engine = data;
    let mut h = window_harness(1200.0, Theme::Light, st);
    h.run();
    h
}

fn has(h: &egui_kittest::Harness<'static, WindowState>, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}

#[test]
fn the_dropdown_offers_both_engines_with_one_line_each() {
    let _g = english();
    let h = page("system", EngineData::default());
    assert!(has(&h, "Translation engine"));
    assert!(has(&h, "System translation (default)"));
    assert!(has(&h, "Fast and light"), "the system engine's line");
    assert!(has(&h, "about 5 GB"), "the language model's line");
}

#[test]
fn choosing_the_language_model_changes_the_setting_and_back() {
    let _g = english();
    let mut h = page(
        "system",
        EngineData {
            system: SystemEngineView::Ready,
            server: ModelServerView::Stopped,
        },
    );
    assert_eq!(h.state().settings.call_translation_engine, "system");
    h.get_by_role_and_label(Role::ComboBox, "Translation engine")
        .click();
    h.run();
    h.get_by_label(&t("settings.engine.llm")).click();
    h.run();
    assert_eq!(h.state().settings.call_translation_engine, "llm");
    h.get_by_role_and_label(Role::ComboBox, "Translation engine")
        .click();
    h.run();
    h.get_by_label(&t("settings.engine.system")).click();
    h.run();
    assert_eq!(h.state().settings.call_translation_engine, "system");
}

#[test]
fn system_translation_says_ready_and_a_missing_pack_has_a_button_to_the_pane() {
    let _g = english();
    let h = page("system", EngineData::default());
    assert!(has(&h, "Checking the language packs"));
    let h = page(
        "system",
        EngineData {
            system: SystemEngineView::Ready,
            ..EngineData::default()
        },
    );
    assert!(has(&h, "the language packs are installed"));

    let mut h = page(
        "system",
        EngineData {
            system: SystemEngineView::PackMissing("Chinese \u{2192} English".into()),
            ..EngineData::default()
        },
    );
    assert!(has(&h, "A language pack is missing for Chinese"));
    h.get_by_role_and_label(Role::Button, "Status: Open System Settings")
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        vec![SettingsCommand::OpenSystemPane(
            SystemPane::TranslationLanguages
        )]
    );
}

#[test]
fn an_old_system_and_a_build_without_the_bridge_are_told_with_no_button() {
    let _g = english();
    for (system, text) in [
        (SystemEngineView::OsTooOld, "needs macOS 26"),
        (
            SystemEngineView::NotBuilt,
            "does not include system translation",
        ),
    ] {
        let h = page(
            "system",
            EngineData {
                system,
                ..EngineData::default()
            },
        );
        assert!(has(&h, text), "{text}");
        assert!(!has(&h, "Open System Settings"));
    }
}

#[test]
fn the_model_server_shows_starting_ready_adopted_and_failed_with_try_again() {
    let _g = english();
    let h = page(
        "llm",
        EngineData {
            server: ModelServerView::Starting { waited_s: 7 },
            ..EngineData::default()
        },
    );
    assert!(has(&h, "Starting the model server (7 s)"));
    let h = page(
        "llm",
        EngineData {
            server: ModelServerView::Ready {
                adopted: false,
                address: "127.0.0.1:8080".into(),
            },
            ..EngineData::default()
        },
    );
    assert!(has(&h, "the model server is running"));
    let h = page(
        "llm",
        EngineData {
            server: ModelServerView::Ready {
                adopted: true,
                address: "127.0.0.1:8080".into(),
            },
            ..EngineData::default()
        },
    );
    assert!(has(&h, "already running at 127.0.0.1:8080"));
    let mut h = page(
        "llm",
        EngineData {
            server: ModelServerView::Failed("llama-server was not found".into()),
            ..EngineData::default()
        },
    );
    assert!(has(&h, "llama-server was not found"));
    h.get_by_role_and_label(Role::Button, "Status: Try again")
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        vec![SettingsCommand::RetryModelServer]
    );
}

#[test]
fn the_speech_pipeline_page_mirrors_the_engine_read_only() {
    let _g = english();
    let mut st = WindowState::new(SettingsSection::SpeechPipeline);
    st.settings.call_translation_engine = "system".into();
    let mut h = window_harness(1200.0, Theme::Light, st);
    h.run();
    assert!(has(&h, "Translation engine: System translation (default)"));
    assert!(
        !h.query_all_by_role_and_label(Role::ComboBox, "Translation engine")
            .next()
            .is_some(),
        "no control on this page"
    );
}

#[test]
fn the_rows_read_in_simplified_chinese() {
    let _g = lock_locale();
    prxvc_i18n::set_locale(Locale::ZhCn);
    let mut st = WindowState::new(SettingsSection::CallTranslation);
    st.settings.call_translation_engine = "system".into();
    st.data.engine.system =
        SystemEngineView::PackMissing("\u{4e2d}\u{6587} \u{2192} English".into());
    let mut h = window_harness(1200.0, Theme::Light, st);
    h.run();
    assert!(
        has(&h, "\u{7ffb}\u{8bd1}\u{5f15}\u{64ce}"),
        "the title in Chinese"
    );
    assert!(has(
        &h,
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{ff08}\u{9ed8}\u{8ba4}\u{ff09}"
    ));
    assert!(
        has(&h, "\u{6253}\u{5f00}\u{7cfb}\u{7edf}\u{8bbe}\u{7f6e}"),
        "the button"
    );
    prxvc_i18n::set_locale(Locale::En);
}
