//! Tests of the Call translation window and the orb clicks that lead to it (egui_kittest), through `render_app`: both themes, both languages, a narrow and a wide window, and the idle, translating, degraded and assistant-blocked states; Start and Stop, the menu entry and the orb click that ends an assistant session.

mod call_fixtures;
mod common;

use common::lock_locale;
use egui::{Role, Theme};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::call_view::command;
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{CallPanel, DesktopUiState, IssueSeverity, IssueView};

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

fn panels() -> Vec<(&'static str, CallPanel)> {
    vec![
        ("idle", CallPanel::default()),
        ("loading", call_fixtures::loading_panel()),
        ("listening", call_fixtures::listening_panel()),
        ("translating", call_fixtures::live_panel()),
        ("degraded", call_fixtures::degraded_panel()),
        ("paused", call_fixtures::paused_panel()),
        ("blocked", call_fixtures::blocked_panel()),
        ("long", call_fixtures::long_panel(300)),
    ]
}

fn panel(name: &str) -> CallPanel {
    panels().into_iter().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn the_window_renders_in_both_themes_languages_and_widths_for_every_state() {
    let _guard = lock_locale();
    for scheme in [Theme::Light, Theme::Dark] {
        for locale in [prxvc_i18n::Locale::En, prxvc_i18n::Locale::ZhCn] {
            prxvc_i18n::set_locale(locale);
            for width in [380.0, 900.0] {
                for (name, panel) in panels() {
                    let mut h = common::builder(egui::vec2(width, 900.0), scheme).build_ui_state(
                        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
                        state_with(panel),
                    );
                    h.input_mut().system_theme = Some(scheme);
                    h.run_steps(3);
                    h.check_accessibility();
                    // The window's title, drawn in the embedded fallback.
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
fn the_state_is_said_in_plain_words_in_a_chip() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let cases = [
        ("idle", "Not running"),
        ("loading", "Loading"),
        ("translating", "Translating"),
        ("degraded", "Reduced"),
        ("paused", "Paused"),
    ];
    for (name, words) in cases {
        let mut h = app_harness(state_with(panel(name)), 420.0);
        h.run_steps(3);
        assert!(has_label(&h, words), "{name}: {words}");
    }
}

#[test]
fn the_translate_switch_is_on_by_default_and_start_sends_the_translated_command() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(CallPanel::default()), 420.0);
    h.run_steps(3);
    assert!(
        h.state().call_translate,
        "the window's purpose is translation"
    );
    h.get_by_role_and_label(Role::Button, "Start").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::START_TRANSLATED]);
    // Audio only stays possible: the switch off.
    h.state_mut().pending_commands.clear();
    h.get_by_role_and_label(Role::Switch, "Translate").click();
    h.run_steps(2);
    assert!(!h.state().call_translate);
    h.get_by_role_and_label(Role::Button, "Start").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::START_AUDIO_ONLY]);
}

#[test]
fn start_and_stop_are_one_primary_button_with_pause_beside_it() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(panel("translating")), 420.0);
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
    let mut h = app_harness(state_with(panel("paused")), 420.0);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Pause").is_none());
    h.get_by_role_and_label(Role::Button, "Resume").click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::RESUME]);
}

#[test]
fn space_toggles_pause_when_nothing_has_focus_and_does_nothing_when_idle() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(panel("translating")), 420.0);
    h.run_steps(3);
    h.key_press(egui::Key::Space);
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::PAUSE]);

    let mut h = app_harness(state_with(panel("paused")), 420.0);
    h.run_steps(3);
    h.key_press(egui::Key::Space);
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::RESUME]);

    let mut h = app_harness(state_with(CallPanel::default()), 420.0);
    h.run_steps(3);
    h.key_press(egui::Key::Space);
    h.run_steps(2);
    assert!(h.state().pending_commands.is_empty());
}

#[test]
fn command_w_closes_the_window() {
    let _guard = lock_locale();
    let mut h = app_harness(state_with(CallPanel::default()), 420.0);
    h.run_steps(3);
    assert!(h.state().windows.call.is_open());
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::W,
        physical_key: Some(egui::Key::W),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    h.run_steps(3);
    assert!(!h.state().windows.call.is_open());
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
        let mut h = app_harness(state_with(panel("blocked")), 420.0);
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
            issues: vec![IssueView {
                key: "call.issue.mic_consent_denied".into(),
                severity: IssueSeverity::Blocking,
                detail: String::new(),
                action: prxvc_desktop_ui::call_view::IssueAction::OpenPermissions,
            }],
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
fn issues_appear_only_when_there_is_something_to_do() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(panel("translating")), 420.0);
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::Button, "End assistant and start")
            .is_none()
    );
    assert!(!has_label(&h, "Notes"), "no issues area, no heading");
    let mut h = app_harness(state_with(panel("degraded")), 420.0);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Open Audio settings");
    h.get_by_role_and_label(Role::Button, "Open Speech settings");
}

#[test]
fn the_languages_swap_while_idle_and_are_edited_in_the_details_and_locked_while_a_call_runs() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut st = state_with(CallPanel::default());
    st.settings.call_self_language = "zh".into();
    st.settings.call_peer_language = "en".into();
    let mut h = app_harness(st, 420.0);
    h.run_steps(3);
    assert!(has_label(&h, "中文 \u{2194} English"));
    h.get_by_role_and_label(Role::Button, "Swap").click();
    h.run_steps(2);
    assert_eq!(h.state().settings.call_self_language, "en");
    assert_eq!(h.state().settings.call_peer_language, "zh");
    // Edit opens the details, where the two selects are.
    h.get_by_role_and_label(Role::Button, "Edit").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "You speak");
    h.get_by_role_and_label(Role::ComboBox, "They speak");

    let mut st = state_with(panel("translating"));
    st.call_window.details_open = true;
    let mut h = app_harness(st, 420.0);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Swap").is_none());
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
fn the_details_hold_the_counters_and_stay_closed_until_asked() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(panel("translating")), 420.0);
    h.run_steps(3);
    assert!(!has_label(&h, "Filler dropped 3"));
    h.get_by_role_and_label(Role::Button, "Details").click();
    h.run_steps(3);
    assert!(has_label(&h, "Filler dropped 3"));
    assert!(has_label(&h, "Already in target language 1"));
    assert!(has_label(&h, "2500 ms"));
    h.get_by_role_and_label(Role::Button, "Hide details")
        .click();
    h.run_steps(3);
    assert!(!has_label(&h, "Filler dropped 3"));
}

#[test]
fn the_footer_shows_the_delay_when_there_is_room_and_drops_it_before_running_over() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    for (width, shown) in [(760.0, true), (320.0, false)] {
        let mut h = common::builder(egui::vec2(width, 600.0), Theme::Light).build_ui_state(
            |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
            state_with(panel("translating")),
        );
        h.run_steps(3);
        assert_eq!(has_label(&h, "Delay 2.5 s"), shown, "{width}");
        h.get_by_role_and_label(Role::Button, "Details");
    }
}

#[test]
fn a_graceful_stop_shows_the_chip_the_hint_and_stop_now() {
    let _guard = lock_locale();
    for (locale, chip, hint, stop_now) in [
        (
            prxvc_i18n::Locale::En,
            "Finishing",
            "Press Stop again to end now (4 s at most)",
            "Stop now",
        ),
        (
            prxvc_i18n::Locale::ZhCn,
            "正在收尾",
            "最多 4 秒",
            "立即停止",
        ),
    ] {
        prxvc_i18n::set_locale(locale);
        let mut panel = panel("translating");
        panel.finishing_s = Some(4);
        let mut h = app_harness(state_with(panel), 420.0);
        h.run_steps(3);
        assert!(has_label(&h, chip), "{chip}");
        assert!(has_label(&h, hint), "{hint}");
        h.get_by_role_and_label(Role::Button, stop_now).click();
        h.run_steps(2);
        assert_eq!(h.state().pending_commands, [command::STOP]);
    }
    // Without a graceful stop the button says Stop and no hint is shown.
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = app_harness(state_with(panel("translating")), 420.0);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Stop");
    assert!(!has_label(&h, "Press Stop again"));
}

#[test]
fn the_notice_to_the_other_party_is_shown_repeatable_and_switchable() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    // Ready: it plays before the first translated sentence; the repeat is offered.
    let mut h = app_harness(state_with(call_fixtures::listening_panel()), 420.0);
    h.run_steps(3);
    assert!(has_label(&h, "Plays when you first speak."));
    h.get_by_role_and_label(Role::Button, "Say it again")
        .click();
    h.run_steps(2);
    assert_eq!(h.state().pending_commands, [command::SAY_DISCLOSURE]);

    // Played: the window says when.
    let h2 = {
        let mut h = app_harness(state_with(panel("translating")), 420.0);
        h.run_steps(3);
        h
    };
    assert!(has_label(&h2, "Played to the other party at 0:12."));

    // Preparing: no repeat yet.
    let mut h = app_harness(state_with(panel("loading")), 420.0);
    h.run_steps(3);
    assert!(has_label(&h, "Getting the spoken notice ready."));
    assert!(
        h.query_by_role_and_label(Role::Button, "Say it again")
            .is_none()
    );

    // Off: a quiet line says the other party is not being told, and there is nothing to repeat.
    let mut h = app_harness(state_with(call_fixtures::undisclosed_panel()), 420.0);
    h.run_steps(3);
    assert!(has_label(&h, "The other party is not being told"));
    assert!(
        h.query_by_role_and_label(Role::Button, "Say it again")
            .is_none()
    );

    // The switch lives in the details and is the setting.
    let mut h = app_harness(state_with(CallPanel::default()), 420.0);
    h.run_steps(3);
    assert!(!h.state().settings.call_disclosure, "off by default");
    assert!(
        !has_label(&h, "The other party is not being told"),
        "idle: quiet"
    );
    h.get_by_role_and_label(Role::Button, "Details").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Switch, "Tell the other party (spoken notice)")
        .click();
    h.run_steps(3);
    assert!(h.state().settings.call_disclosure);
    // The line is about a running call; idle, the switch is the only place that says it.
    assert!(!has_label(&h, "The other party is not being told"));
}

#[test]
fn a_conversation_starts_with_what_will_happen_and_listening_has_its_own_words() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    for (name, title) in [
        ("idle", "Ready when you are"),
        ("loading", "Getting ready"),
        ("listening", "Listening"),
        ("paused", "Paused"),
    ] {
        let mut panel = panel(name);
        if name == "paused" {
            panel.outgoing.clauses.clear();
            panel.outgoing.partial = None;
            panel.incoming.clauses.clear();
            panel.incoming.partial = None;
        }
        let mut h = app_harness(state_with(panel), 420.0);
        h.run_steps(3);
        assert!(has_label(&h, title), "{name}: {title}");
    }
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

fn with_engine(mut panel: CallPanel, engine: &str) -> CallPanel {
    let (key, no_context) = if engine == "llm" {
        ("callwin.engine.llm", false)
    } else {
        ("callwin.engine.system", true)
    };
    panel.translation = prxvc_desktop_ui::call_view::TranslationView {
        engine: engine.into(),
        engine_key: key,
        no_context,
        actions: Vec::new(),
    };
    panel
}

#[test]
fn the_engine_name_is_in_the_head_and_in_the_details_for_both_engines_and_languages() {
    let _guard = lock_locale();
    for locale in [prxvc_i18n::Locale::En, prxvc_i18n::Locale::ZhCn] {
        prxvc_i18n::set_locale(locale);
        for engine in ["system", "llm"] {
            let key = format!("callwin.engine.{engine}");
            let name = prxvc_i18n::t(&key);
            let mut st = DesktopUiState::new();
            st.idle_animation = false;
            st.call = with_engine(panel("translating"), engine);
            let mut h = common::builder(egui::vec2(430.0, 700.0), Theme::Light).build_ui_state(
                |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
                st,
            );
            h.run_steps(3);
            assert!(has_label(&h, &name), "{engine}: {name} in the head");
            assert!(!has_label(&h, &prxvc_i18n::t("callwin.engine.no_context")));
            h.state_mut().call_window.details_open = true;
            h.run_steps(3);
            let line = format!("{}: {name}", prxvc_i18n::t("callwin.engine.label"));
            assert!(has_label(&h, &line), "{engine}: {line} in the details");
            assert_eq!(
                has_label(&h, &prxvc_i18n::t("callwin.engine.no_context")),
                engine == "system",
                "{engine}: the no-context note"
            );
        }
    }
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
}

#[test]
fn each_engine_action_under_an_engine_issue_sends_its_command() {
    use prxvc_desktop_ui::call_view::EngineAction;
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let all = [
        EngineAction::SwitchToLlm,
        EngineAction::SwitchToSystem,
        EngineAction::OpenTranslationLanguages,
    ];
    for action in all {
        let mut p = with_engine(panel("degraded"), "system");
        p.translation.actions = vec![action];
        let mut h = app_harness(state_with(p), 420.0);
        h.run_steps(3);
        h.get_by_role_and_label(Role::Button, &prxvc_i18n::t(action.label_key()))
            .click();
        h.run_steps(2);
        assert_eq!(h.state().pending_commands, [action.command()]);
    }
    // No engine issue: no engine button.
    let mut p = with_engine(panel("translating"), "system");
    p.translation.actions = all.to_vec();
    let mut h = app_harness(state_with(p), 420.0);
    h.run_steps(3);
    for action in all {
        assert!(
            h.query_by_role_and_label(Role::Button, &prxvc_i18n::t(action.label_key()))
                .is_none()
        );
    }
}
