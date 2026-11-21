//! Settings page Advanced: developer tools and diagnostics (design 2.2).

use egui::Ui;

use crate::settings_command::SettingsCommand;
use crate::settings_rows::{
    PageCx, action_row, note, saved_only, select_row, status_row, switch_row, t, unavailable_row,
};
use crate::settings_view::index_of;
use crate::theme::space;
use crate::widgets::{ButtonKind, Tone};

/// Values of `debug.log_level` in the order of the select.
const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;

    switch_row(
        ui,
        tk,
        &t("settings.advanced.developer"),
        &saved_only(&t("settings.advanced.developer.desc")),
        &mut cx.settings.debug_enabled,
        true,
    );
    let enabled = cx.settings.debug_enabled;
    switch_row(
        ui,
        tk,
        &t("settings.advanced.debug_panel"),
        &saved_only(&t("settings.advanced.debug_panel.desc")),
        &mut cx.settings.debug_show_panel,
        enabled,
    );
    let options: Vec<String> = LOG_LEVELS.iter().map(|l| (*l).to_owned()).collect();
    let selected = index_of(&LOG_LEVELS, &cx.settings.debug_log_level);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_log_level",
        &t("settings.log_level"),
        &saved_only(&t("settings.advanced.log_level.desc")),
        &options,
        selected,
    ) {
        cx.settings.debug_log_level = LOG_LEVELS[i].to_owned();
    }
    switch_row(
        ui,
        tk,
        &t("settings.advanced.barge_in"),
        &saved_only(&t("settings.advanced.barge_in.desc")),
        &mut cx.settings.call_barge_in,
        true,
    );
    if action_row(
        ui,
        tk,
        &t("settings.advanced.export"),
        &t("settings.advanced.export.desc"),
        &t("settings.advanced.export.action"),
        ButtonKind::Secondary,
    ) {
        cx.commands.push(SettingsCommand::ExportDiagnostics);
    }
    let format = prxvc_i18n::t_with(
        "settings.advanced.capture_format.value",
        &[
            &(cx.settings.sample_rate / 1000).to_string(),
            &cx.settings.channels.to_string(),
        ],
    );
    status_row(
        ui,
        tk,
        &t("settings.advanced.capture_format"),
        &t("settings.advanced.capture_format.desc"),
        Tone::Neutral,
        &format,
        None,
    );

    unavailable_row(
        ui,
        tk,
        &t("settings.advanced.setup_again"),
        &t("settings.advanced.setup_again.desc"),
        "settings.reason.first_run",
        None,
    );
    ui.add_space(space::S12);
    crate::settings_rows::body_medium(ui, tk, &t("settings.advanced.experimental"));
    note(ui, tk, &t("settings.advanced.experimental.none"));
}
