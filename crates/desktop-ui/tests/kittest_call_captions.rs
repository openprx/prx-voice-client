//! Interaction tests of the captions of the Call translation window (egui_kittest): the follow rule and the jump pill, copy, the text size, the original line, clear, and the bound on what is laid out.

mod call_fixtures;
mod common;

use call_fixtures::{clause, long_panel};
use common::lock_locale;
use egui::{Role, Theme, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use prxvc_desktop_ui::call_view::ClauseStatus;
use prxvc_desktop_ui::{CallPanel, DesktopUiState};

const WIDTH: f32 = 420.0;
const HEIGHT: f32 = 720.0;

fn harness(panel: CallPanel) -> Harness<'static, DesktopUiState> {
    harness_of(panel, HEIGHT)
}

fn harness_of(panel: CallPanel, height: f32) -> Harness<'static, DesktopUiState> {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.call = panel;
    let mut h = common::builder(vec2(WIDTH, height), Theme::Light).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
        st,
    );
    h.run_steps(6);
    h
}

fn wheel_up(h: &mut Harness<'static, DesktopUiState>) {
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(
            WIDTH / 2.0,
            HEIGHT / 2.0,
        )));
    h.run_steps(2);
    for _ in 0..8 {
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

/// A new utterance of the other party arrives.
fn peer_says(h: &mut Harness<'static, DesktopUiState>, n: u64) {
    h.state_mut().call.incoming.clauses.push(clause(
        n,
        0,
        &format!("Item {n}: one more thing."),
        &format!("第{n}条：还有一件事。"),
        ClauseStatus::Played,
    ));
    h.run_steps(4);
}

#[test]
fn the_captions_follow_the_newest_line_until_the_user_scrolls_up() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = harness(long_panel(60));
    assert!(
        h.state().call_window.at_bottom(),
        "starts at the newest line"
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Jump to latest")
            .is_none()
    );

    peer_says(&mut h, 61);
    assert!(h.state().call_window.at_bottom(), "a new line is followed");
    assert_eq!(h.state().call_window.unseen(), 0);

    wheel_up(&mut h);
    assert!(!h.state().call_window.at_bottom(), "the user scrolled up");
    h.get_by_role_and_label(Role::Button, "Jump to latest");

    peer_says(&mut h, 62);
    peer_says(&mut h, 63);
    assert!(
        !h.state().call_window.at_bottom(),
        "new lines do not pull a reader away from what they read"
    );
    assert_eq!(h.state().call_window.unseen(), 2, "the pill counts them");

    h.get_by_role_and_label(Role::Button, "Jump to latest")
        .click();
    h.run_steps(30);
    assert!(
        h.state().call_window.at_bottom(),
        "the pill brings the reader back"
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Jump to latest")
            .is_none()
    );
    assert_eq!(h.state().call_window.unseen(), 0);

    // And following resumes.
    peer_says(&mut h, 64);
    assert!(h.state().call_window.at_bottom());
}

#[test]
fn only_what_is_on_screen_is_laid_out_however_long_the_conversation() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let h = harness(long_panel(300));
    let drawn = h.query_all_by_label_contains("Item ").count();
    let kept = h.state().call_window.transcript.items().len();
    assert_eq!(kept, 300, "the bound of the timeline holds a long call");
    assert!(
        (1..=60).contains(&drawn),
        "{drawn} of {kept} utterances were laid out: only a screenful should be"
    );
}

#[test]
fn the_timeline_is_bounded_and_says_so() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = harness(long_panel(320));
    assert_eq!(h.state().call_window.transcript.items().len(), 300);
    assert_eq!(h.state().call_window.transcript.forgotten, 340);
    wheel_up_far(&mut h);
}

/// Scroll to the very top to see the note about what is no longer kept.
fn wheel_up_far(h: &mut Harness<'static, DesktopUiState>) {
    for _ in 0..40 {
        wheel_up(h);
        if h.query_all_by_label_contains("no longer kept")
            .next()
            .is_some()
        {
            return;
        }
    }
    panic!("the note about forgotten lines never showed");
}

#[test]
fn copy_puts_the_utterance_on_the_clipboard_and_says_so() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = harness(call_fixtures::live_panel());
    assert_eq!(h.state().call_window.last_copied(), None);
    let first = h
        .query_all_by_role_and_label(Role::Button, "Copy")
        .next()
        .expect("a Copy action");
    first.click();
    let mut copied = None;
    for _ in 0..4 {
        h.step();
        copied = copied.or_else(|| {
            h.output()
                .platform_output
                .commands
                .iter()
                .find_map(|c| match c {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                })
        });
    }
    assert!(h.state().call_window.last_copied().is_some());
    assert!(
        copied.is_some_and(|t| !t.is_empty()),
        "the text went to the clipboard"
    );
}

/// Open the Options menu of the captions bar.
fn open_options(h: &mut Harness<'static, DesktopUiState>) {
    h.get_by_role_and_label(Role::Button, "Options").click();
    h.run_steps(3);
}

#[test]
fn the_text_size_is_a_setting_the_window_changes() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = harness(call_fixtures::live_panel());
    assert_eq!(h.state().settings.call_captions_size, "medium");
    open_options(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "L").click();
    h.run_steps(3);
    assert_eq!(h.state().settings.call_captions_size, "large");
    h.get_by_role_and_label(Role::RadioButton, "S").click();
    h.run_steps(3);
    assert_eq!(h.state().settings.call_captions_size, "small");
}

#[test]
fn the_original_line_can_be_hidden() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = harness(call_fixtures::live_panel());
    assert!(
        h.query_all_by_label_contains("Sounds good, three works for me.")
            .next()
            .is_some()
    );
    open_options(&mut h);
    h.get_by_role_and_label(Role::Switch, "Show original")
        .click();
    h.run_steps(3);
    assert!(!h.state().settings.call_captions_show_source);
    assert!(
        h.query_all_by_label_contains("Sounds good, three works for me.")
            .next()
            .is_none()
    );
    assert!(
        h.query_all_by_label_contains("好的，三点我可以。")
            .next()
            .is_some()
    );
}

#[test]
fn clear_empties_the_captions_and_the_empty_state_returns() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let mut h = harness(call_fixtures::live_panel());
    assert!(!h.state().call_window.transcript.is_empty());
    open_options(&mut h);
    h.get_by_role_and_label(Role::MenuItem, "Clear").click();
    h.run_steps(3);
    assert!(h.state().call_window.transcript.is_empty());
    assert!(h.query_all_by_label_contains("Listening").next().is_some());
}

#[test]
fn a_live_partial_shows_as_speaking_and_the_playing_clause_as_playing() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    let h = harness_of(call_fixtures::live_panel(), 1100.0);
    assert!(h.query_all_by_label_contains("speaking").next().is_some());
    assert!(h.query_all_by_label_contains("playing").next().is_some());
    assert!(h.query_all_by_label_contains("not spoken").next().is_some());
    assert!(
        h.query_all_by_label_contains("Not translated: already in 中文")
            .next()
            .is_some()
    );
    assert!(
        h.query_all_by_label_contains("reads as an answer")
            .next()
            .is_some()
    );
    assert!(
        h.query_all_by_label_contains("translated again")
            .next()
            .is_some()
    );
}

#[test]
fn chinese_mixed_with_english_and_long_utterances_render_in_both_languages_without_missing_glyphs()
{
    let _guard = lock_locale();
    for locale in [prxvc_i18n::Locale::En, prxvc_i18n::Locale::ZhCn] {
        prxvc_i18n::set_locale(locale);
        for width in [380.0, 760.0] {
            let mut st = DesktopUiState::new();
            st.idle_animation = false;
            st.call = long_panel(30);
            let mut h = common::builder(vec2(width, HEIGHT), Theme::Dark).build_ui_state(
                |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
                st,
            );
            h.run_steps(5);
            h.check_accessibility();
        }
    }
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
}
