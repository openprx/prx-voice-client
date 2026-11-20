//! Interaction tests for the Permissions page (egui_kittest): the status word of every permission,
//! the action each status calls for, every driver state and typed error, the destructive
//! confirmation, the bundle warning, the offline switch and its allow list.

mod common;

use common::{WindowState, english, wide, window_harness};
use egui::{Role, Theme};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use prxvc_desktop_ui::SettingsSection;
use prxvc_desktop_ui::settings_command::{SettingsCommand, SystemPane};
use prxvc_desktop_ui::settings_data::*;
use prxvc_i18n::t;

const WIDE: f32 = 1200.0;

#[test]
fn every_driver_state_shows_its_word_and_its_actions() {
    let _guard = english();
    let cases = [
        (
            DriverStateView::NotInstalled,
            "Not installed",
            &["Install"][..],
        ),
        (
            DriverStateView::Installed,
            "Installed 1.2.0",
            &["Uninstall"],
        ),
        (
            DriverStateView::UpdateAvailable,
            "Update available",
            &["Update", "Uninstall"],
        ),
        (
            DriverStateView::WrongVariant,
            "Wrong variant",
            &["Update", "Uninstall"],
        ),
        (
            DriverStateView::NeedsRepair,
            "Needs repair",
            &["Repair", "Uninstall"],
        ),
        (DriverStateView::Unsupported, "Not needed", &[]),
    ];
    for (state, word, actions) in cases {
        let mut st = WindowState::new(SettingsSection::Permissions);
        st.data.permissions.driver.state = state;
        let mut h = window_harness(WIDE, Theme::Light, st);
        h.run();
        assert!(
            h.query_all_by_label(word).next().is_some(),
            "{state:?}: {word}"
        );
        for action in ["Install", "Update", "Repair", "Uninstall"] {
            let found = h
                .query_by_role_and_label(Role::Button, &format!("Virtual microphone: {action}"))
                .is_some();
            assert_eq!(found, actions.contains(&action), "{state:?}: {action}");
        }
    }
}

#[test]
fn install_and_uninstall_issue_the_driver_commands_and_uninstall_asks_first() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Permissions);
    st.data.permissions.driver.state = DriverStateView::NotInstalled;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_role_and_label(Role::Button, "Virtual microphone: Install")
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        [SettingsCommand::InstallDriver]
    );

    let mut st = WindowState::new(SettingsSection::Permissions);
    st.data.permissions.driver.state = DriverStateView::Installed;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_role_and_label(Role::Button, "Virtual microphone: Uninstall")
        .click();
    h.run();
    assert!(
        h.state().commands_without_refresh().is_empty(),
        "nothing happens before the confirmation"
    );
    h.get_by_label_contains(&t("settings.driver.uninstall.confirm"));
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    h.run();
    assert!(h.state().commands_without_refresh().is_empty());
    h.get_by_role_and_label(Role::Button, "Virtual microphone: Uninstall")
        .click();
    h.run();
    h.get_by_role_and_label(Role::Button, &t("settings.driver.uninstall.yes"))
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        [SettingsCommand::UninstallDriver]
    );
}

#[test]
fn a_driver_operation_in_progress_disables_the_actions() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Permissions);
    st.data.permissions.driver.state = DriverStateView::NotInstalled;
    st.data.permissions.driver.operation =
        DriverOperationView::Working(DriverOperationKind::Install);
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_label("Working…");
    let install = h.get_by_role_and_label(Role::Button, "Virtual microphone: Install");
    assert!(install.accesskit_node().is_disabled());
}

#[test]
fn each_typed_driver_error_shows_its_advice() {
    let _guard = english();
    let advice = [
        (
            "bundle_not_found",
            "This copy of PRX Voice has no driver inside.",
        ),
        ("source_rejected", "failed its checks"),
        ("cancelled", "Nothing was changed."),
        ("authorization_failed", "administrator authorization failed"),
        ("copy_failed", "could not be copied"),
        ("restart_failed", "did not restart"),
        ("device_did_not_appear", "macOS did not load it"),
        ("device_still_present", "still lists the device"),
        ("unsupported", "macOS only"),
    ];
    for (code, expect) in advice {
        let mut st = WindowState::new(SettingsSection::Permissions);
        st.data.permissions.driver.operation = DriverOperationView::Failed {
            operation: DriverOperationKind::Install,
            code: code.into(),
        };
        let mut h = window_harness(WIDE, Theme::Light, st);
        h.run();
        assert!(
            h.query_by_label_contains(expect).is_some(),
            "{code}: {expect}"
        );
    }
    // A code this version does not know is still shown, with the code.
    let mut st = WindowState::new(SettingsSection::Permissions);
    st.data.permissions.driver.operation = DriverOperationView::Failed {
        operation: DriverOperationKind::Uninstall,
        code: "brand_new".into(),
    };
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    assert!(h.query_by_label_contains("brand_new").is_some());
}

#[test]
fn the_permission_rows_offer_the_action_their_status_calls_for() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Permissions);
    st.data.permissions.microphone = PermissionView::NotAsked;
    st.data.permissions.call_audio = PermissionView::NotAllowed;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    h.get_by_role_and_label(Role::Button, "Microphone: Allow")
        .click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Call audio: Open System Settings")
        .click();
    h.run();
    assert_eq!(
        h.state().commands_without_refresh(),
        [
            SettingsCommand::RequestMicrophone,
            SettingsCommand::OpenSystemPane(SystemPane::AudioCapture)
        ]
    );
    // After opening System Settings the row waits and offers "Check again".
    h.get_by_label(&t("settings.status.waiting"));
    h.get_by_role_and_label(Role::Button, "Call audio: Check again")
        .click();
    h.run();
    assert_eq!(h.state().commands.last(), Some(&SettingsCommand::Refresh));
}

#[test]
fn every_permission_status_has_its_word() {
    let _guard = english();
    for (status, word) in [
        (PermissionView::Allowed, "Allowed"),
        (PermissionView::NotAllowed, "Not allowed"),
        (PermissionView::NotAsked, "Not asked yet"),
        (PermissionView::NotChecked, "Not checked yet"),
        (PermissionView::NotNeeded, "Not needed"),
    ] {
        let mut st = WindowState::new(SettingsSection::Permissions);
        st.data.permissions.microphone = status;
        let mut h = window_harness(WIDE, Theme::Light, st);
        h.run();
        assert!(
            h.query_all_by_label(word).next().is_some(),
            "{status:?}: {word}"
        );
    }
}

#[test]
fn the_bundle_warning_shows_only_outside_a_bundle() {
    let _guard = english();
    let mut st = WindowState::new(SettingsSection::Permissions);
    st.data.permissions.outside_bundle = true;
    let mut h = window_harness(WIDE, Theme::Light, st);
    h.run();
    assert!(
        h.query_by_label_contains("Running outside the app bundle")
            .is_some()
    );
    let h = wide(SettingsSection::Permissions);
    assert!(
        h.query_by_label_contains("Running outside the app bundle")
            .is_none()
    );
}

#[test]
fn the_offline_switch_and_its_allow_list_edit_the_settings() {
    let _guard = english();
    let mut h = wide(SettingsSection::Permissions);
    assert!(!h.state().settings.offline);
    h.get_by_role_and_label(Role::Switch, "Offline mode")
        .click();
    h.run();
    assert!(h.state().settings.offline);

    // Add a host.
    h.get_by_role_and_label(Role::TextInput, "Add a host")
        .click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Add a host")
        .type_text("192.168.1.20");
    h.run();
    h.get_by_role_and_label(Role::Button, "Add").click();
    h.run();
    assert_eq!(h.state().settings.allow_hosts, ["192.168.1.20"]);
    // The list of what stays reachable names loopback, the local provider and the host.
    h.get_by_label_contains("127.0.0.1 and localhost");
    h.get_by_label_contains("127.0.0.1:8080 (Local llama-server)");
    assert!(h.query_all_by_label("192.168.1.20").count() >= 1);

    // A host with a scheme cannot be added.
    h.get_by_role_and_label(Role::TextInput, "Add a host")
        .click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Add a host")
        .type_text("http://x.test");
    h.run();
    assert!(
        h.get_by_role_and_label(Role::Button, "Add")
            .accesskit_node()
            .is_disabled()
    );

    // Remove it again.
    h.get_by_role_and_label(Role::Button, "Remove 192.168.1.20")
        .click();
    h.run();
    assert!(h.state().settings.allow_hosts.is_empty());
}
