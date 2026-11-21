//! Layout guard for the Call translation window: in every state, at every size from 320x360 up,
//! in both languages and with all three caption sizes, no two widgets overlap and none lies
//! outside the window. The state the owner reported (a finished call in the captions, the details
//! open) is among them, reached the way the owner reached it where a click leads there.

mod call_fixtures;
mod common;

use common::layout_guard;
use egui::{Rect, Role, Theme, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use prxvc_desktop_ui::{CallPanel, CallPhase, DesktopUiState};
use prxvc_i18n::Locale;

const SIZES: [(f32, f32); 4] = [(320.0, 360.0), (380.0, 480.0), (430.0, 630.0), (760.0, 900.0)];
const CAPTION_SIZES: [&str; 3] = ["small", "medium", "large"];

/// How a scenario gets to its state.
#[derive(Clone, Copy, Debug)]
enum Scenario {
    IdleEmpty,
    IdleAfterCall,
    Translating,
    Degraded,
    Blocked,
    /// A finished call, then the details opened with the disclosure button.
    DetailsOpen,
    /// A finished call, then "Edit" clicked.
    EditOpen,
    /// A running call with the details open.
    TranslatingDetailsOpen,
}

const SCENARIOS: [Scenario; 8] = [
    Scenario::IdleEmpty,
    Scenario::IdleAfterCall,
    Scenario::Translating,
    Scenario::Degraded,
    Scenario::Blocked,
    Scenario::DetailsOpen,
    Scenario::EditOpen,
    Scenario::TranslatingDetailsOpen,
];

fn finished_call() -> CallPanel {
    CallPanel { phase: CallPhase::Idle, ..call_fixtures::live_panel() }
}

/// The state with the panel the scenario shows; a finished call keeps its captions.
fn state_for(scenario: Scenario, caption_size: &str) -> DesktopUiState {
    let mut st = DesktopUiState::new();
    st.idle_animation = false;
    st.settings.call_captions_size = caption_size.to_owned();
    let panel = match scenario {
        Scenario::IdleEmpty => CallPanel::default(),
        Scenario::IdleAfterCall | Scenario::DetailsOpen | Scenario::EditOpen => {
            // The call runs first so that the captions are taken in, then it ends.
            st.call = call_fixtures::live_panel();
            let running = st.call.clone();
            st.call_window.sync(&running);
            finished_call()
        }
        Scenario::Translating | Scenario::TranslatingDetailsOpen => call_fixtures::live_panel(),
        Scenario::Degraded => call_fixtures::degraded_panel(),
        Scenario::Blocked => call_fixtures::blocked_panel(),
    };
    st.call = panel;
    st.call_window.began_ago(754);
    st
}

fn harness(st: DesktopUiState, width: f32, height: f32, scheme: Theme) -> egui_kittest::Harness<'static, DesktopUiState> {
    let mut h = common::builder(vec2(width, height), scheme).build_ui_state(
        |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_call_window(ui, st),
        st,
    );
    h.input_mut().system_theme = Some(scheme);
    h
}

/// Run the scenario to its state at the given size.
fn reach(scenario: Scenario, caption_size: &str, width: f32, height: f32) -> egui_kittest::Harness<'static, DesktopUiState> {
    let mut h = harness(state_for(scenario, caption_size), width, height, Theme::Light);
    h.run_steps(4);
    match scenario {
        Scenario::DetailsOpen => {
            let label = prxvc_i18n::t("callwin.details");
            h.get_by_role_and_label(Role::Button, &label).click();
            h.run_steps(4);
        }
        Scenario::EditOpen => {
            let label = prxvc_i18n::t("callwin.edit");
            h.get_by_role_and_label(Role::Button, &label).click();
            h.run_steps(4);
        }
        Scenario::TranslatingDetailsOpen => {
            h.state_mut().call_window.details_open = true;
            h.run_steps(4);
        }
        _ => {}
    }
    h
}

#[test]
fn no_widget_overlaps_another_or_leaves_the_window_in_any_state_size_language_or_text_size() {
    let _guard = common::lock_locale();
    let mut failures = Vec::new();
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale.clone());
        for (width, height) in SIZES {
            for scenario in SCENARIOS {
                for caption_size in CAPTION_SIZES {
                    let h = reach(scenario, caption_size, width, height);
                    let window = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, height));
                    let found = layout_guard::problems(&h, window);
                    if !found.is_empty() {
                        failures.push(format!(
                            "{scenario:?} {locale:?} {width}x{height} {caption_size}:\n  {}",
                            found.join("\n  ")
                        ));
                    }
                }
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
    assert!(failures.is_empty(), "{} failing case(s)\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn the_reported_state_has_the_widgets_of_the_details_in_the_window() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::ZhCn);
    let h = reach(Scenario::EditOpen, "medium", 430.0, 630.0);
    // The two language selects are there, below the controls and each on its own row.
    let bounds = |role, key: &str| {
        let label = prxvc_i18n::t(key);
        h.get_by_role_and_label(role, &label).accesskit_node().raw_bounds().unwrap()
    };
    let a = bounds(Role::ComboBox, "callwin.own_language");
    let b = bounds(Role::ComboBox, "callwin.peer_language");
    assert!(a.y1 <= b.y0 || b.y1 <= a.y0, "selects share a row: {a:?} {b:?}");
    let start = bounds(Role::Button, "callwin.start");
    assert!(start.y1 <= a.y0.min(b.y0), "the controls are above the selects");
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn the_state_is_said_once() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::ZhCn);
    let h = reach(Scenario::IdleAfterCall, "medium", 430.0, 630.0);
    let n = h.query_all_by_label_contains(&prxvc_i18n::t("callwin.chip.idle")).count();
    assert_eq!(n, 1, "the idle state is said {n} times");
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn the_translate_switch_shows_its_real_state_while_a_call_runs() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let h = reach(Scenario::Translating, "medium", 430.0, 630.0);
    let switch = h.get_by_role_and_label(Role::Switch, "Translate");
    let on = format!("{:?}", switch.accesskit_node().toggled());
    assert!(on.contains("True"), "the switch of a translating call is on: {on}");
    prxvc_i18n::set_locale(Locale::En);
}

/// The guard itself must see an overlap: two buttons drawn at the same place.
#[test]
fn the_guard_reports_an_overlap_and_a_widget_outside_the_window() {
    let _guard = common::lock_locale();
    let mut h = common::builder(vec2(200.0, 100.0), Theme::Light).build_ui(|ui| {
        let tk = prxvc_desktop_ui::theme::tokens_for(ui.ctx());
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(80.0, 24.0));
        for (i, name) in ["One", "Two"].into_iter().enumerate() {
            ui.put(rect.translate(vec2(10.0 * i as f32, 0.0)), |ui: &mut egui::Ui| {
                prxvc_desktop_ui::widgets::button(ui, &tk, prxvc_desktop_ui::widgets::ButtonKind::Secondary, name)
            });
        }
        let far = Rect::from_min_size(egui::pos2(150.0, 60.0), vec2(80.0, 24.0));
        ui.put(far, |ui: &mut egui::Ui| {
            prxvc_desktop_ui::widgets::button(ui, &tk, prxvc_desktop_ui::widgets::ButtonKind::Secondary, "Far")
        });
    });
    h.run();
    let found = layout_guard::problems(&h, Rect::from_min_size(egui::Pos2::ZERO, vec2(200.0, 100.0)));
    assert!(found.iter().any(|f| f.starts_with("overlap")), "{found:?}");
    assert!(found.iter().any(|f| f.starts_with("outside")), "{found:?}");
}
