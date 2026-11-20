//! Interaction tests for the ways the settings window opens and closes (egui_kittest), through
//! `render_app`: at a named section, from the orb menu, with the keyboard, and the repaint policy.
//!
//! `render_app` schedules repaints through the policy in `repaint.rs`, so the tests that run it set
//! `idle_animation` to false to settle.

mod common;

use common::english;
use egui::{Role, Theme};
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::settings_command::SettingsCommand;
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{DesktopUiState, SettingsSection};

fn still_state() -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st
}

fn app_harness(state: DesktopUiState) -> egui_kittest::Harness<'static, DesktopUiState> {
    common::builder(egui::vec2(900.0, 900.0), Theme::Light).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
        state,
    )
}

#[test]
fn the_window_opens_at_a_named_section_and_a_second_request_reuses_it() {
    let _guard = english();
    let mut h = app_harness(still_state());
    h.run();
    assert!(!h.state().windows.settings.is_open());

    h.state_mut()
        .open_settings(Some(SettingsSection::Permissions));
    h.run();
    assert!(h.state().windows.settings.is_open());
    assert_eq!(
        h.state().settings_view.section,
        SettingsSection::Permissions
    );
    h.get_by_label("Notifications");
    h.check_accessibility();

    // Opening again at another page shows that page in the same window.
    h.state_mut()
        .open_settings(Some(SettingsSection::SpeechPipeline));
    h.run();
    assert!(h.state().windows.settings.is_open());
    h.get_by_label_contains("Data leaving this computer");
    // Without a section the page stays where it was.
    h.state_mut().open_settings(None);
    h.run();
    assert_eq!(
        h.state().settings_view.section,
        SettingsSection::SpeechPipeline
    );
}

#[test]
fn the_orb_menu_entries_open_named_pages() {
    let _guard = english();
    let mut h = app_harness(still_state());
    h.run();
    for (entry, section) in [
        ("Speech pipeline", SettingsSection::SpeechPipeline),
        ("Permissions", SettingsSection::Permissions),
        ("Settings", SettingsSection::General),
    ] {
        // The orb is the only clickable thing in the centre of the stage; its context menu
        // opens on a secondary click.
        let orb = egui::pos2(450.0, 90.0);
        h.input_mut().events.push(egui::Event::PointerMoved(orb));
        h.run();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: orb,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: orb,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
        h.get_by_role_and_label(Role::MenuItem, entry).click();
        h.run();
        assert!(h.state().windows.settings.is_open(), "{entry}");
        assert_eq!(h.state().settings_view.section, section, "{entry}");
        h.state_mut().windows.get_mut(ToolKind::Settings).close();
        h.run();
    }
}

#[test]
fn the_keyboard_shortcut_opens_the_settings_and_escape_and_cmd_w_close_them() {
    let _guard = english();
    let mut h = app_harness(still_state());
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run();
    assert!(
        h.state().windows.settings.is_open(),
        "Cmd or Ctrl and comma opens the window"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().windows.settings.is_open(), "Escape closes it");
    h.state_mut().open_settings(None);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    h.run();
    assert!(
        !h.state().windows.settings.is_open(),
        "Cmd or Ctrl and W closes it"
    );
}

#[test]
fn opening_the_window_asks_the_app_to_probe_again() {
    let _guard = english();
    let mut st = still_state();
    st.open_settings(None);
    assert_eq!(st.settings_commands, [SettingsCommand::Refresh]);
}

#[test]
fn idle_orb_settles_with_and_without_the_breathing_animation() {
    let _guard = english();
    for idle_animation in [true, false] {
        let mut st = DesktopUiState::new();
        st.idle_animation = idle_animation;
        let mut h = app_harness(st);
        assert!(
            h.run_ok().is_some(),
            "idle_animation={idle_animation} did not settle"
        );
    }
}

#[test]
fn active_orb_keeps_repainting_every_frame() {
    let _guard = english();
    let mut state = DesktopUiState::new();
    state.session_state = "AssistantSpeaking".to_owned();
    let mut h = app_harness(state);
    assert!(h.run_ok().is_none(), "an active orb must not settle");
}
