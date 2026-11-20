//! Settings page Audio: devices from the system, echo protection and the virtual microphone (design 2.2, WP5).

use egui::Ui;

use crate::settings_command::SettingsCommand;
use crate::settings_data::{DeviceView, DriverStateView, InputChoiceView, InputReason};
use crate::settings_rows::{
    PageCx, note, notice, select_row, status_row, t, text_row, unavailable_row,
};
use crate::settings_section::SettingsSection;
use crate::settings_view::{index_of, labels};
use crate::theme::space;
use crate::widgets::Tone;

/// Values of `audio.echo_mode` in the order of the select.
const ECHO_MODES: [&str; 3] = ["auto", "always", "off"];

/// Options of a device select: the system default first, then the devices; a configured
/// device that is not connected is kept as an extra option so the row shows the truth.
/// Returns the options, the device UID of each option (empty for the default) and the index
/// of the current choice.
fn device_options(
    default_label: String,
    devices: &[DeviceView],
    configured: &str,
) -> (Vec<String>, Vec<String>, usize) {
    let mut options = vec![default_label];
    let mut uids = vec![String::new()];
    for d in devices {
        options.push(d.name.clone());
        uids.push(d.uid.clone());
    }
    let selected = if configured.is_empty() {
        0
    } else if let Some(i) = uids.iter().position(|u| u == configured) {
        i
    } else {
        options.push(prxvc_i18n::t_with(
            "settings.audio.not_connected",
            &[configured],
        ));
        uids.push(configured.to_owned());
        options.len() - 1
    };
    (options, uids, selected)
}

fn next_session(description: &str) -> String {
    format!("{description} {}", t("settings.note.next_session"))
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let audio = &cx.data.audio;

    // Microphone.
    let default_label = match &audio.default_input {
        Some(name) => prxvc_i18n::t_with("settings.audio.default_named", &[name]),
        None => t("settings.audio.default"),
    };
    let (options, uids, selected) =
        device_options(default_label, &audio.inputs, &cx.settings.input_device);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_input_device",
        &t("settings.audio.microphone"),
        &next_session(&t("settings.input_device.desc")),
        &options,
        selected,
    ) {
        cx.settings.input_device = uids[i].clone();
    }
    match &audio.input_choice {
        Some(InputChoiceView::Chosen { name, reason }) => {
            let key = match reason {
                InputReason::Configured => None,
                InputReason::ConfiguredMissing => Some("settings.audio.reason.missing"),
                InputReason::SystemDefault => None,
                InputReason::DefaultAvoided => Some("settings.audio.reason.avoided"),
            };
            if let Some(key) = key {
                notice(ui, tk, Tone::Warning, &prxvc_i18n::t_with(key, &[name]));
            } else {
                note(ui, tk, &prxvc_i18n::t_with("settings.audio.uses", &[name]));
            }
        }
        Some(InputChoiceView::OwnVirtualDevice) => {
            notice(
                ui,
                tk,
                Tone::Danger,
                &t("settings.audio.reason.own_virtual"),
            );
        }
        Some(InputChoiceView::NoUsableDevice) => {
            notice(ui, tk, Tone::Danger, &t("settings.audio.reason.no_device"));
        }
        None => {}
    }
    if let Some(error) = &audio.list_error {
        notice(
            ui,
            tk,
            Tone::Danger,
            &prxvc_i18n::t_with("settings.audio.list_error", &[error]),
        );
    }
    ui.add_space(space::S4);

    // Output.
    let (options, uids, selected) = device_options(
        t("settings.audio.default"),
        &audio.outputs,
        &cx.settings.output_device,
    );
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_output_device",
        &t("settings.audio.speakers"),
        &next_session(&t("settings.output_device.desc")),
        &options,
        selected,
    ) {
        cx.settings.output_device = uids[i].clone();
    }
    status_row(
        ui,
        tk,
        &t("settings.audio.route"),
        &t("settings.audio.route.desc"),
        Tone::Neutral,
        &t("settings.audio.route.unknown"),
        None,
    );

    // Echo protection.
    let options = labels(&[
        "settings.audio.echo.auto",
        "settings.audio.echo.always",
        "settings.audio.echo.off",
    ]);
    let selected = index_of(&ECHO_MODES, &cx.settings.echo_mode);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_echo_mode",
        &t("settings.audio.echo"),
        &next_session(&t("settings.audio.echo.desc")),
        &options,
        selected,
    ) {
        cx.settings.echo_mode = ECHO_MODES[i].to_owned();
    }

    // Virtual microphone.
    let driver = &cx.data.permissions.driver;
    let (tone, status) = driver_status_text(driver.state, driver.installed_version.as_deref());
    let manage = t("settings.audio.virtual_mic.manage");
    if status_row(
        ui,
        tk,
        &t("settings.audio.virtual_mic"),
        &t("settings.audio.virtual_mic.desc"),
        tone,
        &status,
        Some((&manage, true)),
    ) {
        cx.view.section = SettingsSection::Permissions;
        cx.commands.push(SettingsCommand::Refresh);
    }
    let stored = cx.settings.virtual_mic_device.clone();
    if let Some(uid) = text_row(
        ui,
        tk,
        cx.view,
        "virtual_mic_device",
        &t("settings.audio.virtual_mic_uid"),
        &next_session(&t("settings.audio.virtual_mic_uid.desc")),
        &stored,
        |_| Ok(()),
    ) {
        cx.settings.virtual_mic_device = uid.trim().to_owned();
    }

    unavailable_row(
        ui,
        tk,
        &t("settings.audio.test"),
        &t("settings.audio.test.desc"),
        "settings.reason.audio_test",
        None,
    );
}

/// The chip tone and text for a driver state (shared with the Permissions page).
pub(crate) fn driver_status_text(state: DriverStateView, version: Option<&str>) -> (Tone, String) {
    let with_version = |key: &str| match version {
        Some(v) => prxvc_i18n::t_with(&format!("{key}.versioned"), &[v]),
        None => t(key),
    };
    match state {
        DriverStateView::Unknown => (Tone::Neutral, t("settings.status.not_checked")),
        DriverStateView::Unsupported => (Tone::Neutral, t("settings.status.not_needed")),
        DriverStateView::NotInstalled => (Tone::Neutral, t("settings.status.not_installed")),
        DriverStateView::Installed => (Tone::Success, with_version("settings.status.installed")),
        DriverStateView::UpdateAvailable => (Tone::Warning, t("settings.status.update_available")),
        DriverStateView::WrongVariant => (Tone::Warning, t("settings.status.wrong_variant")),
        DriverStateView::NeedsRepair => (Tone::Danger, t("settings.status.needs_repair")),
        DriverStateView::InstalledIsNewer => {
            (Tone::Success, with_version("settings.status.installed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_data::TransportView;

    fn dev(uid: &str, name: &str) -> DeviceView {
        DeviceView {
            uid: uid.into(),
            name: name.into(),
            transport: TransportView::BuiltIn,
            is_default: false,
        }
    }

    #[test]
    fn the_default_is_first_and_selected_when_nothing_is_configured() {
        let (options, uids, selected) =
            device_options("Default".into(), &[dev("a", "A"), dev("b", "B")], "");
        assert_eq!(options, ["Default", "A", "B"]);
        assert_eq!(uids, ["", "a", "b"]);
        assert_eq!(selected, 0);
    }

    #[test]
    fn a_configured_device_is_selected_by_uid() {
        let (_, _, selected) = device_options("D".into(), &[dev("a", "A"), dev("b", "B")], "b");
        assert_eq!(selected, 2);
    }

    #[test]
    fn a_configured_device_that_is_gone_stays_visible() {
        let (options, uids, selected) = device_options("D".into(), &[dev("a", "A")], "gone");
        assert_eq!(selected, 2);
        assert_eq!(uids[2], "gone");
        assert!(options[2].contains("gone"));
    }
}
