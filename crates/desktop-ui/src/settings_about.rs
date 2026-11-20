//! Settings page About: version, licences, and where the data and the log live (design 2.2).

use egui::Ui;

use crate::settings_command::{RevealTarget, SettingsCommand};
use crate::settings_rows::{PageCx, status_row, t, unavailable_row};
use crate::widgets::Tone;

fn reveal_label(is_macos: bool) -> String {
    t(if is_macos {
        "settings.about.reveal.finder"
    } else {
        "settings.about.reveal.files"
    })
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let about = cx.data.about.clone();
    let reveal = reveal_label(cx.data.permissions.is_macos);

    let version = prxvc_i18n::t_with(
        "settings.about.version.value",
        &[if about.version.is_empty() {
            "?"
        } else {
            &about.version
        }],
    );
    status_row(
        ui,
        tk,
        &t("settings.about.version"),
        &t("settings.about.version.desc"),
        Tone::Neutral,
        &version,
        None,
    );
    unavailable_row(
        ui,
        tk,
        &t("settings.about.updates"),
        &t("settings.about.updates.desc"),
        "settings.reason.updates",
        None,
    );
    unavailable_row(
        ui,
        tk,
        &t("settings.about.model_licences"),
        &t("settings.about.model_licences.desc"),
        "settings.reason.model_licences",
        None,
    );

    let notices = t("settings.about.notices.action");
    if status_row(
        ui,
        tk,
        &t("settings.about.notices"),
        &about
            .notices_file
            .clone()
            .unwrap_or_else(|| t("settings.about.notices.missing")),
        Tone::Neutral,
        &t("settings.about.notices.chip"),
        Some((&notices, about.notices_file.is_some())),
    ) {
        cx.commands
            .push(SettingsCommand::Reveal(RevealTarget::Notices));
    }
    if status_row(
        ui,
        tk,
        &t("settings.about.data_dir"),
        &about.data_dir,
        Tone::Neutral,
        &t("settings.about.folder"),
        Some((&reveal, !about.data_dir.is_empty())),
    ) {
        cx.commands
            .push(SettingsCommand::Reveal(RevealTarget::DataDir));
    }
    let (log_text, log_chip) = match &about.log_file {
        Some(p) => (p.clone(), t("settings.about.log.file")),
        None => (
            t("settings.about.log.terminal"),
            t("settings.about.log.terminal.chip"),
        ),
    };
    if status_row(
        ui,
        tk,
        &t("settings.about.log"),
        &log_text,
        Tone::Neutral,
        &log_chip,
        Some((&reveal, about.log_file.is_some())),
    ) {
        cx.commands
            .push(SettingsCommand::Reveal(RevealTarget::LogFile));
    }
    if status_row(
        ui,
        tk,
        &t("settings.about.config"),
        &about.config_file,
        Tone::Neutral,
        &t("settings.about.file"),
        Some((&reveal, !about.config_file.is_empty())),
    ) {
        cx.commands
            .push(SettingsCommand::Reveal(RevealTarget::ConfigFile));
    }
}
