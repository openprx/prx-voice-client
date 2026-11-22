//! Interaction tests for the custom RF widgets, driven through the
//! accessibility tree by role and label (egui_kittest).

mod common;

use egui::{Role, Theme, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use prxvc_desktop_ui::theme;
use prxvc_desktop_ui::widgets::{self, ButtonKind};

const ROW_LABEL: &str = "Auto-play replies";

#[derive(Default)]
struct WidgetState {
    on: bool,
    clicks: u32,
    selected: usize,
}

#[test]
fn switch_toggles_by_label() {
    let _guard = common::lock_locale();
    let mut harness = common::builder(vec2(420.0, 120.0), Theme::Light).build_ui_state(
        |ui, st: &mut WidgetState| {
            let tk = theme::tokens_for(ui.ctx());
            widgets::setting_row(ui, &tk, ROW_LABEL, None, 60.0, true, |ui| {
                widgets::switch(ui, &tk, &mut st.on);
            });
        },
        WidgetState::default(),
    );
    harness.run();

    let switch = harness.get_by_role_and_label(Role::Switch, ROW_LABEL);
    assert_eq!(
        switch.accesskit_node().toggled(),
        Some(accesskit_toggled(false))
    );
    switch.click();
    harness.run();
    assert!(harness.state().on, "the switch must turn on");

    harness
        .get_by_role_and_label(Role::Switch, ROW_LABEL)
        .click();
    harness.run();
    assert!(!harness.state().on, "the switch must turn off again");
}

#[test]
fn button_activates_by_label_and_by_keyboard() {
    let _guard = common::lock_locale();
    let mut harness = common::builder(vec2(300.0, 100.0), Theme::Light).build_ui_state(
        |ui, st: &mut WidgetState| {
            let tk = theme::tokens_for(ui.ctx());
            if widgets::button(ui, &tk, ButtonKind::Primary, "Save changes").clicked() {
                st.clicks += 1;
            }
        },
        WidgetState::default(),
    );
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Save changes")
        .click();
    harness.run();
    assert_eq!(harness.state().clicks, 1);

    harness.get_by_label("Save changes").focus();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().clicks,
        2,
        "Enter activates the focused button"
    );
}

#[test]
fn segmented_control_selects_the_clicked_segment() {
    let _guard = common::lock_locale();
    let mut harness = common::builder(vec2(300.0, 100.0), Theme::Dark).build_ui_state(
        |ui, st: &mut WidgetState| {
            let tk = theme::tokens_for(ui.ctx());
            if let Some(i) = widgets::segmented(ui, &tk, st.selected, &["Light", "Dark"]) {
                st.selected = i;
            }
        },
        WidgetState::default(),
    );
    harness.run();

    harness
        .get_by_role_and_label(Role::RadioButton, "Dark")
        .click();
    harness.run();
    assert_eq!(harness.state().selected, 1);

    harness
        .get_by_role_and_label(Role::RadioButton, "Light")
        .click();
    harness.run();
    assert_eq!(harness.state().selected, 0);
}

fn accesskit_toggled(on: bool) -> egui::accesskit::Toggled {
    if on {
        egui::accesskit::Toggled::True
    } else {
        egui::accesskit::Toggled::False
    }
}
