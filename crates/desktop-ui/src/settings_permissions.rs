//! Settings page Permissions: one row per permission or prerequisite, with status, purpose, consequence and one action (design 3.5).

use egui::Ui;

use crate::settings_audio::driver_status_text;
use crate::settings_command::{SettingsCommand, SystemPane};
use crate::settings_data::{
    DriverOperationKind, DriverOperationView, DriverStateView, PermissionView, RequestOutcome,
};
use crate::settings_rows::{
    PageCx, button_strip, note, notice, status_row, status_row_multi, switch_row, t,
};
use crate::theme::space;
use crate::widgets::{self, ButtonKind, Tone};

/// Which permission a row is about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Microphone,
    CallAudio,
}

/// The text of a "Needed for / Without it" description.
fn needed(for_key: &str, without_key: &str) -> String {
    format!(
        "{}\n{}",
        prxvc_i18n::t_with("settings.perm.needed_for", &[&t(for_key)]),
        prxvc_i18n::t_with("settings.perm.without", &[&t(without_key)])
    )
}

/// The chip of a permission: tone and status word.
fn permission_chip(status: PermissionView, waiting: bool) -> (Tone, String) {
    if waiting && status != PermissionView::Allowed {
        return (Tone::Neutral, t("settings.status.waiting"));
    }
    match status {
        PermissionView::NotChecked => (Tone::Neutral, t("settings.status.not_checked")),
        PermissionView::Allowed => (Tone::Success, t("settings.status.allowed")),
        PermissionView::NotAllowed => (Tone::Danger, t("settings.status.not_allowed")),
        PermissionView::NotAsked => (Tone::Neutral, t("settings.status.not_asked")),
        PermissionView::NotNeeded => (Tone::Neutral, t("settings.status.not_needed")),
    }
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    permission_row(ui, cx, Which::Microphone);
    permission_row(ui, cx, Which::CallAudio);
    status_row(
        ui,
        cx.tk,
        &t("settings.perm.shortcuts"),
        &t("settings.perm.shortcuts.desc"),
        Tone::Neutral,
        &t("settings.status.not_needed"),
        None,
    );
    driver_rows(ui, cx);
    status_row(
        ui,
        cx.tk,
        &t("settings.perm.notifications"),
        &t("settings.perm.notifications.desc"),
        Tone::Neutral,
        &t("settings.status.not_used"),
        None,
    );
    network_rows(ui, cx);
    if cx.data.permissions.is_macos && cx.data.permissions.outside_bundle {
        ui.add_space(space::S12);
        notice(ui, cx.tk, Tone::Warning, &t("settings.perm.bundle"));
    }
}

fn permission_row(ui: &mut Ui, cx: &mut PageCx<'_>, which: Which) {
    let perms = &cx.data.permissions;
    let (status, outcome) = match which {
        Which::Microphone => (perms.microphone, perms.microphone_request),
        Which::CallAudio => (perms.call_audio, perms.call_audio_request),
    };
    let waiting_since = match which {
        Which::Microphone => &mut cx.view.mic_waiting_since,
        Which::CallAudio => &mut cx.view.call_audio_waiting_since,
    };
    if status == PermissionView::Allowed || status == PermissionView::NotNeeded {
        *waiting_since = None;
    }
    let waiting = waiting_since.is_some();
    let (tone, chip) = permission_chip(status, waiting);
    let (label, description, pane) = match which {
        Which::Microphone => (
            t("settings.perm.microphone"),
            needed(
                "settings.perm.microphone.for",
                "settings.perm.microphone.without",
            ),
            SystemPane::Microphone,
        ),
        Which::CallAudio => (
            t("settings.perm.call_audio"),
            needed(
                "settings.perm.call_audio.for",
                "settings.perm.call_audio.without",
            ),
            SystemPane::AudioCapture,
        ),
    };
    let ask = t(match which {
        Which::Microphone => "settings.perm.allow",
        Which::CallAudio => "settings.perm.check_now",
    });
    let open = t("settings.perm.open_system_settings");
    let again = t("settings.perm.check_again");
    // (label, what a click does)
    enum Do {
        Ask,
        Open,
        Refresh,
    }
    let action: Option<(&str, Do)> = if waiting {
        Some((&again, Do::Refresh))
    } else {
        match status {
            PermissionView::NotAsked => Some((&ask, Do::Ask)),
            PermissionView::NotAllowed => Some((&open, Do::Open)),
            PermissionView::NotChecked => Some((&again, Do::Refresh)),
            PermissionView::Allowed | PermissionView::NotNeeded => None,
        }
    };
    let actions: Vec<(&str, ButtonKind, bool)> = action
        .iter()
        .map(|(l, _)| (*l, ButtonKind::Secondary, true))
        .collect();
    if status_row_multi(ui, cx.tk, &label, &description, tone, &chip, &actions).is_some() {
        match action.map(|(_, d)| d) {
            Some(Do::Ask) => cx.commands.push(match which {
                Which::Microphone => SettingsCommand::RequestMicrophone,
                Which::CallAudio => SettingsCommand::RequestCallAudio,
            }),
            Some(Do::Open) => {
                let since = match which {
                    Which::Microphone => &mut cx.view.mic_waiting_since,
                    Which::CallAudio => &mut cx.view.call_audio_waiting_since,
                };
                *since = Some(cx.now);
                cx.view.last_recheck = cx.now;
                cx.commands.push(SettingsCommand::OpenSystemPane(pane));
            }
            Some(Do::Refresh) => cx.commands.push(SettingsCommand::Refresh),
            None => {}
        }
    }
    let key = match outcome {
        Some(RequestOutcome::NotInBundle) => {
            Some(("settings.perm.outcome.not_in_bundle", Tone::Warning))
        }
        Some(RequestOutcome::Refused) => Some(("settings.perm.outcome.refused", Tone::Warning)),
        Some(RequestOutcome::NoAnswer) => Some(("settings.perm.outcome.no_answer", Tone::Neutral)),
        Some(RequestOutcome::Granted) | None => None,
    };
    if let Some((key, tone)) = key {
        notice(ui, cx.tk, tone, &t(key));
        ui.add_space(space::S8);
    }
}

fn driver_rows(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let driver = cx.data.permissions.driver.clone();
    let working = matches!(driver.operation, DriverOperationView::Working(_));
    let (tone, chip) = if working {
        (Tone::Neutral, t("settings.driver.working"))
    } else {
        driver_status_text(driver.state, driver.installed_version.as_deref())
    };
    let can_install = driver.bundle_available && !working;
    let mut actions: Vec<(&str, ButtonKind, bool)> = Vec::new();
    let install = t("settings.driver.install");
    let update = t("settings.driver.update");
    let repair = t("settings.driver.repair");
    let uninstall = t("settings.driver.uninstall");
    enum Do {
        Install,
        Uninstall,
    }
    let mut effects: Vec<Do> = Vec::new();
    match driver.state {
        DriverStateView::NotInstalled => {
            actions.push((&install, ButtonKind::Secondary, can_install));
            effects.push(Do::Install);
        }
        DriverStateView::UpdateAvailable | DriverStateView::WrongVariant => {
            actions.push((&update, ButtonKind::Secondary, can_install));
            effects.push(Do::Install);
            actions.push((&uninstall, ButtonKind::DangerQuiet, !working));
            effects.push(Do::Uninstall);
        }
        DriverStateView::NeedsRepair => {
            actions.push((&repair, ButtonKind::Secondary, can_install));
            effects.push(Do::Install);
            actions.push((&uninstall, ButtonKind::DangerQuiet, !working));
            effects.push(Do::Uninstall);
        }
        DriverStateView::Installed | DriverStateView::InstalledIsNewer => {
            actions.push((&uninstall, ButtonKind::DangerQuiet, !working));
            effects.push(Do::Uninstall);
        }
        DriverStateView::Unknown | DriverStateView::Unsupported => {}
    }
    let mut description = needed("settings.perm.driver.for", "settings.perm.driver.without");
    if let Some(variant) = &driver.installed_variant {
        description.push('\n');
        description.push_str(&prxvc_i18n::t_with("settings.driver.variant", &[variant]));
    }
    let needs_bundle = matches!(
        driver.state,
        DriverStateView::NotInstalled
            | DriverStateView::UpdateAvailable
            | DriverStateView::WrongVariant
            | DriverStateView::NeedsRepair
    );
    if needs_bundle && !driver.bundle_available {
        description.push('\n');
        description.push_str(&t("settings.driver.no_bundle"));
    }
    if let Some(i) = status_row_multi(
        ui,
        tk,
        &t("settings.perm.driver"),
        &description,
        tone,
        &chip,
        &actions,
    ) {
        match effects[i] {
            Do::Install => cx.commands.push(SettingsCommand::InstallDriver),
            Do::Uninstall => cx.view.confirm_uninstall = true,
        }
    }
    if cx.view.confirm_uninstall {
        confirm_uninstall(ui, cx);
    }
    operation_result(ui, cx, &driver.operation);
}

/// The destructive confirmation of removing the driver (RF 3.5 rule 7: bound to its object).
fn confirm_uninstall(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    ui.add_space(space::S4);
    notice(
        ui,
        tk,
        Tone::Danger,
        &t("settings.driver.uninstall.confirm"),
    );
    ui.add_space(space::S8);
    button_strip(ui, |ui| {
        if widgets::button(
            ui,
            tk,
            ButtonKind::Danger,
            &t("settings.driver.uninstall.yes"),
        )
        .clicked()
        {
            cx.view.confirm_uninstall = false;
            cx.commands.push(SettingsCommand::UninstallDriver);
        }
        if widgets::button(ui, tk, ButtonKind::Secondary, &t("common.cancel")).clicked() {
            cx.view.confirm_uninstall = false;
        }
    });
    ui.add_space(space::S8);
}

/// The outcome of the last driver operation, with the typed error's advice.
fn operation_result(ui: &mut Ui, cx: &PageCx<'_>, operation: &DriverOperationView) {
    let (tone, message) = match operation {
        DriverOperationView::Idle | DriverOperationView::Working(_) => return,
        DriverOperationView::Succeeded(DriverOperationKind::Install) => {
            (Tone::Success, t("settings.driver.done.install"))
        }
        DriverOperationView::Succeeded(DriverOperationKind::Uninstall) => {
            (Tone::Success, t("settings.driver.done.uninstall"))
        }
        DriverOperationView::Cancelled(_) => (Tone::Neutral, t("settings.driver.error.cancelled")),
        DriverOperationView::Failed { code, .. } => (Tone::Danger, driver_error_text(code)),
    };
    ui.add_space(space::S4);
    notice(ui, cx.tk, tone, &message);
    ui.add_space(space::S8);
}

/// The translated advice for a driver error code (`DriverInstallError::code`).
pub(crate) fn driver_error_text(code: &str) -> String {
    let key = format!("settings.driver.error.{code}");
    let text = t(&key);
    if text == key {
        prxvc_i18n::t_with("settings.driver.error.other", &[code])
    } else {
        text
    }
}

/// The codes the app's typed driver errors can carry (`DriverInstallError::code`).
#[cfg(test)]
const DRIVER_ERROR_CODES: [&str; 9] = [
    "bundle_not_found",
    "source_rejected",
    "cancelled",
    "authorization_failed",
    "copy_failed",
    "restart_failed",
    "device_did_not_appear",
    "device_still_present",
    "unsupported",
];

/// Whether `host` can go on the allow list: no scheme, path or spaces.
fn is_host(host: &str) -> bool {
    let h = host.trim();
    !h.is_empty()
        && h.len() <= 253
        && !h.contains("://")
        && !h.contains(['/', ' ', '\t', '?', '#', '@'])
}

fn network_rows(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    switch_row(
        ui,
        tk,
        &t("settings.perm.offline"),
        &format!(
            "{}\n{}",
            needed("settings.perm.network.for", "settings.perm.network.without"),
            t("settings.perm.offline.desc")
        ),
        &mut cx.settings.offline,
        true,
    );

    // Allowed local hosts.
    ui.add_space(space::S8);
    crate::settings_rows::body_medium(ui, tk, &t("settings.perm.allow_hosts"));
    note(ui, tk, &t("settings.perm.allow_hosts.desc"));
    let mut remove = None;
    for (i, host) in cx.settings.allow_hosts.iter().enumerate() {
        ui.add_space(space::S4);
        ui.horizontal(|ui| {
            ui.label(widgets::text(host, crate::theme::size::BODY, tk.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = t("settings.perm.allow_hosts.remove");
                if widgets::button_with(
                    ui,
                    tk,
                    ButtonKind::Secondary,
                    &label,
                    crate::theme::layout::CONTROL_HEIGHT_SMALL,
                    true,
                )
                .accessible_name(prxvc_i18n::t_with(
                    "settings.perm.allow_hosts.remove_named",
                    &[host],
                ))
                .clicked()
                {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        cx.settings.allow_hosts.remove(i);
    }
    ui.add_space(space::S8);
    let label = t("settings.perm.allow_hosts.add_label");
    widgets::text_field(ui, tk, &label, &mut cx.view.host_draft, None, false);
    ui.add_space(space::S8);
    let host = cx.view.host_draft.trim().to_owned();
    let valid = is_host(&host) && !cx.settings.allow_hosts.contains(&host);
    if widgets::button_with(
        ui,
        tk,
        ButtonKind::Secondary,
        &t("settings.perm.allow_hosts.add"),
        crate::theme::layout::CONTROL_HEIGHT,
        valid,
    )
    .clicked()
        && valid
    {
        cx.settings.allow_hosts.push(host);
        cx.view.host_draft.clear();
    }
    ui.add_space(space::S12);

    // What stays reachable in offline mode.
    crate::settings_rows::body_medium(ui, tk, &t("settings.perm.reachable"));
    ui.add_space(space::S4);
    note(ui, tk, &t("settings.perm.reachable.loopback"));
    for (name, host) in &cx.data.permissions.local_endpoints {
        note(ui, tk, &format!("{host} ({name})"));
    }
    for host in &cx.settings.allow_hosts {
        note(ui, tk, host);
    }
    ui.add_space(space::S8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_plain_names_or_addresses() {
        assert!(is_host("192.168.1.20"));
        assert!(is_host("llama.local"));
        assert!(is_host("[::1]"));
        assert!(!is_host(""));
        assert!(!is_host("http://llama.local"));
        assert!(!is_host("a b"));
        assert!(!is_host("host/path"));
    }

    #[test]
    fn every_driver_error_code_has_a_translation() {
        for code in DRIVER_ERROR_CODES {
            let key = format!("settings.driver.error.{code}");
            assert_ne!(t(&key), key, "missing {key}");
        }
    }
}
