//! The assistant's issue card on the orb stage (egui_kittest, through `render_app`): the card with the button that opens the settings page the fix lives on, in both languages.

mod common;

use common::lock_locale;
use egui::{Role, Theme};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::call_view::IssueAction;
use prxvc_desktop_ui::{AssistantIssue, DISMISS_ISSUE, DesktopUiState, SettingsSection};
use prxvc_i18n::Locale;

fn app_harness(state: DesktopUiState) -> egui_kittest::Harness<'static, DesktopUiState> {
    common::builder(egui::vec2(420.0, 800.0), Theme::Light).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
        state,
    )
}

fn issue(key: &str, action: IssueAction) -> AssistantIssue {
    AssistantIssue {
        key: key.into(),
        detail: "cannot connect: connection refused".into(),
        action,
    }
}

fn state_with_issue(i: AssistantIssue) -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.assistant_issue = Some(i);
    st
}

#[test]
fn an_issue_says_what_is_wrong_in_plain_words_with_its_detail_and_in_both_languages() {
    let _guard = lock_locale();
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale.clone());
        let mut h = app_harness(state_with_issue(issue(
            "assistant.issue.dialogue_down",
            IssueAction::OpenAssistantSettings,
        )));
        h.run();
        let message = prxvc_i18n::t("assistant.issue.dialogue_down");
        assert_ne!(
            message, "assistant.issue.dialogue_down",
            "the message exists in {locale:?}"
        );
        assert!(
            h.query_all_by_label_contains(&message).next().is_some(),
            "{locale:?}"
        );
        assert!(
            h.query_all_by_label_contains("connection refused")
                .next()
                .is_some()
        );
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn the_action_of_an_issue_opens_the_settings_page_it_names() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    for (action, label, section) in [
        (
            IssueAction::OpenAssistantSettings,
            "Open Assistant settings",
            SettingsSection::Assistant,
        ),
        (
            IssueAction::OpenSpeechSettings,
            "Open Speech pipeline",
            SettingsSection::SpeechPipeline,
        ),
        (
            IssueAction::OpenAudioSettings,
            "Open Audio settings",
            SettingsSection::Audio,
        ),
        (
            IssueAction::OpenPermissions,
            "Open Permissions",
            SettingsSection::Permissions,
        ),
    ] {
        let mut h = app_harness(state_with_issue(issue("assistant.issue.setting", action)));
        h.run();
        h.get_by_role_and_label(Role::Button, label).click();
        h.run();
        assert_eq!(h.state().settings_view.section, section, "{label}");
        assert!(
            h.state().windows.settings.is_open(),
            "{label} opens the settings window"
        );
    }
}

#[test]
fn an_issue_without_an_action_has_no_action_button_and_close_asks_the_app_to_forget_it() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let mut h = app_harness(state_with_issue(issue(
        "assistant.issue.call_running",
        IssueAction::None,
    )));
    h.run();
    assert!(h.query_all_by_label_contains("Open ").next().is_none());
    h.get_by_role_and_label(Role::Button, "Close").click();
    h.run();
    assert!(h.state().assistant_issue.is_none());
    assert_eq!(h.state().pending_commands, [DISMISS_ISSUE]);
}

#[test]
fn every_issue_message_and_action_label_exists_in_both_languages() {
    let _guard = lock_locale();
    let codes = [
        "call_running",
        "model_missing",
        "model_installing",
        "model_damaged",
        "runtime_missing",
        "unsupported",
        "offline",
        "setting",
        "voice_missing",
        "voice_not_prepared",
        "dialogue_down",
        "dialogue_timeout",
        "dialogue_rejected",
        "dialogue_empty",
        "dialogue_not_available_yet",
        "synthesis_failed",
        "no_audio",
        "listening_failed",
        "microphone_unavailable",
        "microphone_lost",
        "microphone_rate",
        "output_failed",
        "start_failed",
    ];
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale.clone());
        for code in codes {
            let key = format!("assistant.issue.{code}");
            assert_ne!(prxvc_i18n::t(&key), key, "{key} in {locale:?}");
        }
        for key in [
            "assistant.action.open_assistant",
            "assistant.action.open_speech",
            "assistant.action.open_audio",
            "assistant.action.open_permissions",
        ] {
            assert_ne!(prxvc_i18n::t(key), key, "{key} in {locale:?}");
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}
