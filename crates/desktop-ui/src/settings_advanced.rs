//! Settings page Advanced: developer tools, diagnostics, assistant providers and connection timing (design 2.2).

use egui::Ui;

use crate::settings_command::SettingsCommand;
use crate::settings_rows::{
    PageCx, action_row, note, saved_only, select_row, status_row, switch_row, t, text_row,
    unavailable_row,
};
use crate::settings_view::{index_of, is_number_in};
use crate::theme::space;
use crate::widgets::{ButtonKind, Tone};

/// Values of `debug.log_level` in the order of the select.
const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

fn restart(description: &str) -> String {
    format!("{description} {}", t("settings.note.restart"))
}

fn not_empty(v: &str) -> Result<(), String> {
    if v.trim().is_empty() {
        Err(t("settings.advanced.required"))
    } else {
        Ok(())
    }
}

fn number(v: &str, min: u64, max: u64) -> Result<(), String> {
    if is_number_in(v, min, max) {
        Ok(())
    } else {
        Err(prxvc_i18n::t_with(
            "settings.advanced.number_range",
            &[&min.to_string(), &max.to_string()],
        ))
    }
}

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

    ui.add_space(space::S12);
    crate::settings_rows::body_medium(ui, tk, &t("settings.advanced.providers"));
    note(ui, tk, &restart(&t("settings.advanced.providers.desc")));
    let fields: [(&'static str, &str, String); 3] = [
        (
            "asr_provider",
            "settings.advanced.asr_provider",
            cx.settings.network_asr_provider.clone(),
        ),
        (
            "agent_provider",
            "settings.advanced.agent_provider",
            cx.settings.network_agent_provider.clone(),
        ),
        (
            "tts_provider",
            "settings.advanced.tts_provider",
            cx.settings.network_tts_provider.clone(),
        ),
    ];
    for (key, label, stored) in fields {
        if let Some(v) = text_row(ui, tk, cx.view, key, &t(label), "", &stored, not_empty) {
            let v = v.trim().to_owned();
            match key {
                "asr_provider" => cx.settings.network_asr_provider = v,
                "agent_provider" => cx.settings.network_agent_provider = v,
                _ => cx.settings.network_tts_provider = v,
            }
        }
    }

    crate::settings_rows::body_medium(ui, tk, &t("settings.advanced.timing"));
    note(ui, tk, &restart(&t("settings.advanced.timing.desc")));
    let timeout = cx.settings.connect_timeout_ms.to_string();
    if let Some(v) = text_row(
        ui,
        tk,
        cx.view,
        "connect_timeout_ms",
        &t("settings.advanced.connect_timeout"),
        "",
        &timeout,
        |v| number(v, 100, 600_000),
    ) {
        cx.settings.connect_timeout_ms = v.trim().parse().unwrap_or(cx.settings.connect_timeout_ms);
    }
    let attempts = cx.settings.reconnect_attempts.to_string();
    if let Some(v) = text_row(
        ui,
        tk,
        cx.view,
        "reconnect_attempts",
        &t("settings.advanced.reconnect_attempts"),
        "",
        &attempts,
        |v| number(v, 0, 100),
    ) {
        cx.settings.reconnect_attempts = v.trim().parse().unwrap_or(cx.settings.reconnect_attempts);
    }
    let delay = cx.settings.reconnect_delay_ms.to_string();
    if let Some(v) = text_row(
        ui,
        tk,
        cx.view,
        "reconnect_delay_ms",
        &t("settings.advanced.reconnect_delay"),
        "",
        &delay,
        |v| number(v, 0, 600_000),
    ) {
        cx.settings.reconnect_delay_ms = v.trim().parse().unwrap_or(cx.settings.reconnect_delay_ms);
    }

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
