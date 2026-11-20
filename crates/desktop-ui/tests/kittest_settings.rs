//! Interaction tests for the settings window (egui_kittest): navigation by click and keyboard,
//! the two layouts, every control type, rows that are not available yet, and the pages' own
//! rows. The permission and driver rows are in `kittest_settings_permissions`, opening the window in
//! `kittest_settings_entry`.
//!
//! The window body is driven on its own (`common::window_harness`).

mod common;

use common::{WindowState, english, narrow, wide, window_harness};
use egui::{Role, Theme};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use prxvc_desktop_ui::settings_command::SettingsCommand;
use prxvc_desktop_ui::settings_data::*;
use prxvc_desktop_ui::SettingsSection;
use prxvc_i18n::t;

const WIDE: f32 = 1200.0;

#[test]
fn the_section_list_has_every_section_in_the_designed_order() {
    let _guard = english();
    let harness = wide(SettingsSection::General);
    let labels: Vec<String> = harness
        .query_all(egui_kittest::kittest::By::new().role(Role::MenuItem))
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    let expected: Vec<String> = SettingsSection::ALL.iter().map(|s| s.label()).collect();
    assert_eq!(labels, expected);
    assert_eq!(
        expected,
        [
            "General",
            "Call translation",
            "Assistant",
            "Audio",
            "Voice",
            "Speech pipeline",
            "Permissions",
            "Shortcuts",
            "Privacy and data",
            "Advanced",
            "About"
        ]
    );
}

#[test]
fn clicking_a_section_shows_its_page() {
    let _guard = english();
    // A label that only that page has.
    let marker = |s: SettingsSection| match s {
        SettingsSection::General => "Interface language",
        SettingsSection::CallTranslation => "My language",
        SettingsSection::Assistant => "Server address",
        SettingsSection::Audio => "Echo protection",
        SettingsSection::Voice => "My voice",
        SettingsSection::SpeechPipeline => "Data leaving this computer",
        SettingsSection::Permissions => "Notifications",
        SettingsSection::Shortcuts => "Start or stop session",
        SettingsSection::Privacy => "Include spoken text in logs",
        SettingsSection::Advanced => "Capture format",
        SettingsSection::About => "Data folder",
    };
    let mut harness = wide(SettingsSection::General);
    // Server address needs live mode.
    harness.state_mut().settings.use_mock = false;
    for section in SettingsSection::ALL {
        harness
            .get_by_role_and_label(Role::MenuItem, &section.label())
            .click();
        harness.run();
        assert_eq!(harness.state().view.section, section);
        assert!(
            harness
                .query_all_by_label_contains(marker(section))
                .next()
                .is_some(),
            "{section:?}: {} not shown",
            marker(section)
        );
    }
}

#[test]
fn arrow_keys_move_the_selection_and_enter_chooses() {
    let _guard = english();
    let mut harness = wide(SettingsSection::General);
    harness
        .get_by_role_and_label(Role::MenuItem, "General")
        .focus();
    harness.run();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert_eq!(
        harness.state().view.section,
        SettingsSection::CallTranslation,
        "an arrow key that lands on a section chooses it"
    );
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert_eq!(harness.state().view.section, SettingsSection::Assistant);
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(
        harness.state().view.section,
        SettingsSection::CallTranslation
    );

    // Tab moves on without choosing; Enter on a focused item chooses it.
    harness
        .get_by_role_and_label(Role::MenuItem, "Audio")
        .focus();
    harness.run();
    assert_eq!(
        harness.state().view.section,
        SettingsSection::CallTranslation,
        "focus alone does not choose"
    );
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().view.section, SettingsSection::Audio);
}

#[test]
fn below_900_px_the_sections_are_a_select_and_from_900_px_a_list() {
    let _guard = english();
    let n = narrow(SettingsSection::General);
    assert!(
        n.query_by_role(Role::MenuItem).is_none(),
        "no list in one column"
    );
    let name = t("settings.section_select");
    n.get_by_role_and_label(Role::ComboBox, &name);
    let w = wide(SettingsSection::General);
    assert!(
        w.query_by_role_and_label(Role::ComboBox, &t("settings.section_select"))
            .is_none()
    );
    assert_eq!(
        w.query_all(egui_kittest::kittest::By::new().role(Role::MenuItem))
            .count(),
        11
    );
}

#[test]
fn the_section_select_changes_the_page() {
    let _guard = english();
    let mut h = narrow(SettingsSection::General);
    h.get_by_role_and_label(Role::ComboBox, &t("settings.section_select"))
        .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "Audio").click();
    h.run();
    assert_eq!(h.state().view.section, SettingsSection::Audio);
}

#[test]
fn a_switch_toggles_and_is_named_by_its_row() {
    let _guard = english();
    let mut h = wide(SettingsSection::General);
    let before = h.state().settings.always_on_top;
    h.get_by_role_and_label(Role::Switch, "Keep main window on top")
        .click();
    h.run();
    assert_eq!(h.state().settings.always_on_top, !before);
}

#[test]
fn a_segmented_control_chooses_a_mode() {
    let _guard = english();
    let mut h = wide(SettingsSection::General);
    h.get_by_role_and_label(Role::RadioButton, "Call translation")
        .click();
    h.run();
    assert_eq!(h.state().settings.mode, "call");
    h.get_by_role_and_label(Role::RadioButton, "Voice assistant")
        .click();
    h.run();
    assert_eq!(h.state().settings.mode, "assistant");
}

#[test]
fn a_select_chooses_an_option_from_its_menu() {
    let _guard = english();
    let mut h = wide(SettingsSection::Audio);
    h.get_by_role_and_label(Role::ComboBox, "Echo protection")
        .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "Always").click();
    h.run();
    assert_eq!(h.state().settings.echo_mode, "always");

    // A device is stored by its UID, the system default as the empty string.
    h.get_by_role_and_label(Role::ComboBox, "Microphone")
        .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "USB Studio Mic")
        .click();
    h.run();
    assert_eq!(h.state().settings.input_device, "usb-mic");
    h.get_by_role_and_label(Role::ComboBox, "Microphone")
        .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "System default · MacBook Pro Microphone")
        .click();
    h.run();
    assert_eq!(h.state().settings.input_device, "");
}

#[test]
fn a_radio_list_chooses_what_i_hear() {
    let _guard = english();
    let mut h = wide(SettingsSection::CallTranslation);
    h.get_by_role_and_label(Role::RadioButton, "Original and translation")
        .click();
    h.run();
    assert_eq!(h.state().settings.call_local_output_mode, "both");
}

#[test]
fn the_two_call_languages_must_differ() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::CallTranslation);
    st.settings.call_self_language = "zh".into();
    st.settings.call_peer_language = "en".into();
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Other party's language")
        .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "中文").click();
    h.run();
    assert_eq!(
        h.state().settings.call_peer_language,
        "en",
        "the clash is refused"
    );
}

#[test]
fn a_text_field_commits_a_valid_value_on_blur_and_restores_an_invalid_one() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Assistant);
    st.settings.use_mock = false;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    let field = h.get_by_role_and_label(Role::TextInput, "Server address");
    field.focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Server address")
        .type_text("http://10.0.0.5:4000");
    h.run();
    assert_eq!(
        h.state().settings.server_url,
        "http://localhost:3000",
        "not committed while typing"
    );
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(h.state().settings.server_url, "http://10.0.0.5:4000");

    // An invalid address is not stored and the old value returns.
    let field = h.get_by_role_and_label(Role::TextInput, "Server address");
    field.focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Server address")
        .type_text("not a url");
    h.run();
    assert!(
        h.query_by_label_contains(&t("settings.server_url.invalid"))
            .is_some(),
        "the error shows while typing"
    );
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(h.state().settings.server_url, "http://10.0.0.5:4000");
}

#[test]
fn a_row_that_is_not_available_yet_has_a_disabled_button_and_a_reason() {
    let _guard = english();
    let mut h = wide(SettingsSection::General);
    let before = h.state().settings.clone();
    let name = format!("Launch at login: {}", t("settings.unavailable"));
    let button = h.get_by_role_and_label(Role::Button, &name);
    assert!(
        button.accesskit_node().is_disabled(),
        "the control is the disabled variant"
    );
    button.click();
    h.run();
    assert_eq!(h.state().settings, before, "clicking it changes nothing");
    assert!(h.state().commands_without_refresh().is_empty());
    h.get_by_label_contains(&t("settings.reason.login_item"));
}

#[test]
fn rows_that_wait_for_a_back_end_are_disabled_on_every_page() {
    let _guard = english();
    for (section, row) in [
        (SettingsSection::CallTranslation, "Call app"),
        (SettingsSection::Audio, "Test audio"),
        (SettingsSection::Shortcuts, "Push to talk"),
        (SettingsSection::SpeechPipeline, "Choose providers"),
        (SettingsSection::Privacy, "Delete all history"),
        (SettingsSection::Advanced, "Run setup again"),
        (SettingsSection::About, "Updates"),
    ] {
        let mut h = wide(section);
        let name = format!("{row}: {}", t("settings.unavailable"));
        let button = h.get_by_role_and_label(Role::Button, &name);
        assert!(button.accesskit_node().is_disabled(), "{section:?}: {row}");
        button.click();
        h.run();
        assert!(
            h.state().commands_without_refresh().is_empty(),
            "{section:?}: {row}"
        );
    }
}

#[test]
fn the_voice_switch_waits_for_a_profile() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Voice);
    st.data.voice_profile = None;
    st.settings.voice_use_clone = true;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_role_and_label(Role::Switch, "Speak my translations in my voice")
        .click();
    h.run();
    assert!(
        h.state().settings.voice_use_clone,
        "disabled while no profile is ready"
    );
    h.get_by_label_contains(&t("settings.voice.use_clone.needs_profile"));
    let setup = h.get_by_role_and_label(Role::Button, "My voice: Set up my voice");
    assert!(setup.accesskit_node().is_disabled());
}

#[test]
fn problems_and_the_missing_runtime_explanation_are_shown_with_their_instruction() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::SpeechPipeline);
    st.data.pipeline = PipelineData::Problems(vec![
        PipelineProblem {
            subject: Some((PipelineRole::Recognition, "zh".into())),
            fallback: false,
            code: "runtime_not_built_in".into(),
            args: vec!["qwen3-asr".into(), "transcribe".into()],
        },
        PipelineProblem {
            subject: Some((PipelineRole::Translation, "zh>en".into())),
            fallback: false,
            code: "model_not_installed".into(),
            args: vec!["gemma-4-e4b".into(), "4.6 GB".into()],
        },
    ]);
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    assert!(h.query_by_label_contains("does not include").is_some());
    assert!(
        h.query_by_label_contains("Download it, then try again")
            .is_some()
    );
}

#[test]
fn the_resolved_pipeline_shows_provider_readiness_and_what_leaves() {
    let _guard = english();
    let h = wide(SettingsSection::SpeechPipeline);
    assert!(
        h.query_by_label_contains("Sends text to api.example.com")
            .is_some()
    );
    assert!(h.query_by_label_contains("Runs on this computer").is_some());
    assert!(
        h.query_by_label_contains("Sent to api.example.com")
            .is_some()
    );
}

#[test]
fn the_audio_page_explains_a_missing_or_avoided_input_device() {
    let _guard = english();
    for (reason, expect) in [
        (
            InputReason::ConfiguredMissing,
            "is not connected. MacBook Pro Microphone is used instead",
        ),
        (
            InputReason::DefaultAvoided,
            "virtual device, so MacBook Pro Microphone is used",
        ),
    ] {
        let mut st = WindowState::new(SettingsSection::Audio);
        st.data.audio.input_choice = Some(InputChoiceView::Chosen {
            name: "MacBook Pro Microphone".into(),
            reason,
        });
        let mut h = window_harness(WIDE, Theme::Light, st);
        h.run();
        assert!(h.query_by_label_contains(expect).is_some(), "{reason:?}");
    }
    // A configured device that is gone stays visible in the select.
    let mut st = WindowState::new(SettingsSection::Audio);
    st.settings.input_device = "gone-mic".into();
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Microphone")
        .click();
    h.run();
    assert!(
        h.query_by_role_and_label(Role::MenuItem, "gone-mic (not connected)")
            .is_some(),
        "the configured device that is gone stays in the menu"
    );
}

#[test]
fn the_virtual_microphone_row_leads_to_the_permissions_page() {
    let _guard = english();
    let mut h = wide(SettingsSection::Audio);
    h.get_by_role_and_label(Role::Button, "Virtual microphone: Manage")
        .click();
    h.run();
    assert_eq!(h.state().view.section, SettingsSection::Permissions);
}

#[test]
fn retry_connection_and_export_diagnostics_issue_their_commands() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Assistant);
    st.settings.use_mock = false;
    st.network = prxvc_desktop_ui::NetworkStatus::Failed;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_label("Not connected");
    h.get_by_role_and_label(Role::Button, "Connection: Retry connection")
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        [SettingsCommand::RetryConnection]
    );

    let mut h = wide(SettingsSection::Advanced);
    h.get_by_role_and_label(Role::Button, "Export diagnostics: Export")
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        [SettingsCommand::ExportDiagnostics]
    );
}

#[test]
fn a_notice_about_an_unreadable_settings_file_is_shown_on_every_page() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Audio);
    st.data.notices.push(SettingsNotice::UnreadableFileKept {
        kept_as: "config.toml.unreadable-1".into(),
    });
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    assert!(
        h.query_by_label_contains("config.toml.unreadable-1")
            .is_some()
    );
}
