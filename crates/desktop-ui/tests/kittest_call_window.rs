//! Tests of the Call translation window and the orb clicks that lead to it (egui_kittest), through `render_app`: both themes, both languages, a narrow and a wide window, and the idle, translating, degraded and assistant-blocked states; Start and Stop, the menu entry and the orb click that ends an assistant session.

mod common;

use common::lock_locale;
use egui::{Role, Theme};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::call_view::{
    CaptionView, ClauseStatus, CountersView, IssueAction, LatencyView, PartialView, command,
};
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{
    CallPanel, CallPhase, DesktopUiState, Heard, IssueSeverity, IssueView, LaneView,
};

fn app_harness(
    state: DesktopUiState,
    width: f32,
) -> egui_kittest::Harness<'static, DesktopUiState> {
    common::builder(egui::vec2(width, 900.0), Theme::Light).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
        state,
    )
}

/// Whether some node's label contains `text` (a window title is drawn twice in the embedded fallback).
fn has_label(h: &egui_kittest::Harness<'static, DesktopUiState>, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}

fn state_with(panel: CallPanel) -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.call = panel;
    st.windows.request_open(ToolKind::Call);
    st
}

fn issue(key: &str, severity: IssueSeverity, action: IssueAction) -> IssueView {
    IssueView {
        key: key.into(),
        severity,
        detail: "detail from the coordinator".into(),
        action,
    }
}

fn translating_lane(hears: Heard) -> LaneView {
    LaneView {
        running: true,
        translating: true,
        languages: Some(("zh".into(), "en".into())),
        hears,
        device: "Built-in Mic".into(),
        rms: 0.05,
        peak: 0.3,
        partial: Some(PartialView {
            utterance: 3,
            text: "今天天气很好我们去".into(),
            stable_len: 18,
        }),
        clauses: vec![
            CaptionView {
                source: "你好，今天的会议改到下午三点".into(),
                translation: "Hello, today's meeting has moved to three in the afternoon.".into(),
                status: ClauseStatus::Played,
                ..CaptionView::default()
            },
            CaptionView {
                source: "我们开会".into(),
                translation: String::new(),
                status: ClauseStatus::Translating,
                ..CaptionView::default()
            },
        ],
        counters: Some(CountersView {
            filler_dropped: 3,
            backlog_skips: 1,
            stale_skips: 1,
            ..CountersView::default()
        }),
        latency: Some(LatencyView {
            speech_start_to_first_audio_ms: Some(2500.0),
            pause_to_first_audio_ms: Some(900.0),
            stages: vec![("mt_first_token".into(), 90.0)],
        }),
        ..LaneView::default()
    }
}

fn panels() -> Vec<(&'static str, CallPanel)> {
    vec![
        ("idle", CallPanel::default()),
        (
            "translating",
            CallPanel {
                phase: CallPhase::Translating,
                translate_enabled: true,
                in_call_mode: true,
                default_input_ours: true,
                outgoing: translating_lane(Heard::Translation),
                incoming: translating_lane(Heard::Translation),
                ..CallPanel::default()
            },
        ),
        (
            "degraded",
            CallPanel {
                phase: CallPhase::Degraded,
                translate_enabled: true,
                in_call_mode: true,
                default_input_ours: true,
                outgoing: translating_lane(Heard::Original),
                incoming: translating_lane(Heard::Translation),
                issues: vec![
                    issue(
                        "call.issue.speaker_output",
                        IssueSeverity::Degraded,
                        IssueAction::OpenAudioSettings,
                    ),
                    issue(
                        "call.issue.provider.server_unreachable",
                        IssueSeverity::Blocking,
                        IssueAction::OpenSpeechSettings,
                    ),
                ],
                ..CallPanel::default()
            },
        ),
        (
            "blocked",
            CallPanel {
                issues: vec![
                    issue(
                        "call.issue.assistant_active",
                        IssueSeverity::Blocking,
                        IssueAction::EndAssistantAndStart,
                    ),
                    issue(
                        "call.issue.mic_consent_denied",
                        IssueSeverity::Blocking,
                        IssueAction::OpenPermissions,
                    ),
                ],
                last_error: Some(issue(
                    "call.issue.driver_missing",
                    IssueSeverity::Blocking,
                    IssueAction::OpenPermissions,
                )),
                ..CallPanel::default()
            },
        ),
    ]
}

#[test]
fn the_window_renders_in_both_themes_languages_and_widths_for_every_state() {
    let _guard = lock_locale();
    for scheme in [Theme::Light, Theme::Dark] {
        for locale in [prxvc_i18n::Locale::En, prxvc_i18n::Locale::ZhCn] {
            prxvc_i18n::set_locale(locale);
            for width in [420.0, 900.0] {
                for (name, panel) in panels() {
                    let mut h = common::builder(egui::vec2(width, 900.0), scheme).build_ui_state(
                        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
                        state_with(panel),
                    );
                    h.input_mut().system_theme = Some(scheme);
                    h.run_steps(3);
                    h.check_accessibility();
                    // The window's title, drawn in the embedded fallback, and its state line.
                    assert!(has_label(&h, &prxvc_i18n::t("callwin.title")));
                    assert!(
                        h.state().windows.call.is_open(),
                        "{name} {scheme:?} {width}"
                    );
                }
            }
        }
    }
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
}

#[test]
fn the_state_is_said_in_plain_words() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let cases = [
        ("idle", "Not running"),
        ("translating", "Translating both directions"),
        ("degraded", "Reduced:"),
    ];
    for (name, words) in cases {
        let panel = panels().into_iter().find(|(n, _)| *n == name).unwrap().1;
        let mut h = app_harness(state_with(panel), 420.0);
        h.run_steps(3);
        assert!(has_label(&h, words), "{name}: {words}");
    }
}

#[test]
fn start_and_stop_are_one_primary_button_that_sends_the_call_commands() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    // Idle: Start, audio only until the Translate switch is on.
    let mut h = app_harness(state_with(CallPanel::default()), 420.0);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Stop").is_none());
    h.get_by_role_and_label(Role::Button, "Start").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::START_AUDIO_ONLY]);
    // The Translate switch decides what the next Start does.
    h.state_mut().pending_commands.clear();
    h.get_by_role_and_label(Role::Switch, "Translate").click();
    h.run_steps(2);
    assert!(h.state().call_translate);
    h.get_by_role_and_label(Role::Button, "Start").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::START_TRANSLATED]);

    // Translating: the same button is Stop; Pause is next to it.
    let translating = panels()
        .into_iter()
        .find(|(n, _)| *n == "translating")
        .unwrap()
        .1;
    let mut h = app_harness(state_with(translating), 420.0);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Start").is_none());
    h.get_by_role_and_label(Role::Button, "Pause").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Stop").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::PAUSE, command::STOP]);
}

#[test]
fn resume_replaces_pause_while_paused() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let panel = CallPanel {
        phase: CallPhase::Paused,
        ..CallPanel::default()
    };
    let mut h = app_harness(state_with(panel), 420.0);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Pause").is_none());
    h.get_by_role_and_label(Role::Button, "Resume").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::RESUME]);
}

#[test]
fn the_issue_buttons_end_the_assistant_and_start_or_open_the_settings_page() {
    let _guard = lock_locale();
    for (locale, end, perm) in [
        (
            prxvc_i18n::Locale::En,
            "End assistant and start",
            "Open Permissions",
        ),
        (prxvc_i18n::Locale::ZhCn, "结束助手并开始", "打开权限页"),
    ] {
        prxvc_i18n::set_locale(locale);
        let blocked = panels()
            .into_iter()
            .find(|(n, _)| *n == "blocked")
            .unwrap()
            .1;
        let mut h = app_harness(state_with(blocked), 420.0);
        h.run_steps(3);
        h.get_by_role_and_label(Role::Button, end).click();
        h.run_steps(2);
        assert_eq!(
            h.state().pending_commands,
            [command::END_ASSISTANT_AND_START],
            "{end}"
        );
        // A permission issue's button opens the Permissions page of the settings.
        let consent = CallPanel {
            issues: vec![issue(
                "call.issue.mic_consent_denied",
                IssueSeverity::Blocking,
                IssueAction::OpenPermissions,
            )],
            ..CallPanel::default()
        };
        let mut h = app_harness(state_with(consent), 420.0);
        h.run_steps(3);
        h.get_by_role_and_label(Role::Button, perm).click();
        h.run_steps(3);
        assert!(h.state().windows.settings.is_open(), "{perm}");
        assert_eq!(
            h.state().settings_view.section,
            prxvc_desktop_ui::SettingsSection::Permissions
        );
    }
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
}

#[test]
fn the_end_assistant_button_appears_only_next_to_that_issue() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let degraded = panels()
        .into_iter()
        .find(|(n, _)| *n == "degraded")
        .unwrap()
        .1;
    let mut h = app_harness(state_with(degraded), 420.0);
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::Button, "End assistant and start")
            .is_none()
    );
}

#[test]
fn the_languages_are_the_settings_and_locked_while_a_call_runs() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(CallPanel::default()), 420.0);
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "You speak");
    h.get_by_role_and_label(Role::ComboBox, "They speak");
    let translating = panels()
        .into_iter()
        .find(|(n, _)| *n == "translating")
        .unwrap()
        .1;
    let mut h = app_harness(state_with(translating), 420.0);
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::ComboBox, "You speak")
            .is_none()
    );
    assert!(has_label(
        &h,
        "The languages change while call mode is off."
    ));
}

#[test]
fn the_notes_say_what_was_dropped_and_skipped() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let translating = panels()
        .into_iter()
        .find(|(n, _)| *n == "translating")
        .unwrap()
        .1;
    let mut h = app_harness(state_with(translating), 420.0);
    h.run_steps(3);
    assert!(has_label(&h, "Dropped filler: 3. Skipped to keep up: 2."));
    assert!(has_label(&h, "2500 ms"));
}

#[test]
fn the_orb_menu_has_a_call_translation_entry_that_opens_the_window() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    let mut h = app_harness(st, 900.0);
    h.run();
    assert!(!h.state().windows.call.is_open());
    let orb = egui::pos2(450.0, 90.0);
    h.input_mut().events.push(egui::Event::PointerMoved(orb));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: orb,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "Call translation")
        .click();
    h.run();
    assert!(h.state().windows.call.is_open());
    // Escape closes it, as it does the other tool windows.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().windows.call.is_open());
}

#[test]
fn the_debug_window_links_to_the_call_window_and_no_longer_runs_the_call() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.windows.request_open(ToolKind::Debug);
    let mut h = app_harness(st, 900.0);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Start").is_none());
    assert!(
        h.query_by_role_and_label(Role::Button, "Start call mode")
            .is_none()
    );
    h.get_by_role_and_label(Role::Button, "Open the Call translation window")
        .click();
    h.run_steps(3);
    assert!(h.state().windows.call.is_open());
}

fn click_orb(h: &mut egui_kittest::Harness<'static, DesktopUiState>) {
    let orb = egui::pos2(450.0, 90.0);
    h.input_mut().events.push(egui::Event::PointerMoved(orb));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: orb,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(2);
}

#[test]
fn a_second_orb_click_ends_the_session_unless_the_assistant_is_speaking() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    for (session_state, expected) in [
        ("Listening", "StopSession"),
        ("UserSpeaking", "StopSession"),
        ("AssistantSpeaking", "InterruptPlayback"),
    ] {
        let mut st = DesktopUiState::new();
        st.idle_animation = false;
        st.session_active = true;
        st.session_state = session_state.to_owned();
        let mut h = app_harness(st, 900.0);
        h.run_steps(3);
        click_orb(&mut h);
        assert_eq!(h.state().pending_commands, [expected], "{session_state}");
    }
    // Without a session the click starts one.
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    let mut h = app_harness(st, 900.0);
    h.run();
    click_orb(&mut h);
    assert_eq!(
        h.state().pending_commands,
        ["CreateSession", "StartSession"]
    );
}
