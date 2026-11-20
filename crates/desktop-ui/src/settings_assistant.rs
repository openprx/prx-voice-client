//! Settings page Assistant: reply playback, the session language and the server connection (design 2.2).

use egui::Ui;

use crate::settings_command::SettingsCommand;
use crate::settings_rows::{PageCx, select_row, status_row, switch_row, t, text_row};
use crate::settings_view::{index_of, is_http_url};
use crate::shared::NetworkStatus;
use crate::widgets::Tone;

/// Values of `network.language` in the order of the select.
const LANGUAGES: [&str; 2] = ["zh-CN", "en"];

/// A description with the note that the setting is read when the app starts.
fn restart(description: &str) -> String {
    format!("{description} {}", t("settings.note.restart"))
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;

    switch_row(
        ui,
        tk,
        &t("settings.assistant.speak"),
        &restart(&t("settings.auto_play.desc")),
        &mut cx.settings.auto_play,
        true,
    );

    let options = vec![t("settings.call.lang.zh"), t("settings.call.lang.en")];
    let selected = index_of(&LANGUAGES, &cx.settings.network_language);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_assistant_language",
        &t("settings.assistant.language"),
        &restart(&t("settings.assistant.language.desc")),
        &options,
        selected,
    ) {
        cx.settings.network_language = LANGUAGES[i].to_owned();
    }

    switch_row(
        ui,
        tk,
        &t("settings.assistant.demo"),
        &restart(&t("settings.use_mock.desc")),
        &mut cx.settings.use_mock,
        true,
    );

    if !cx.settings.use_mock {
        let stored = cx.settings.server_url.clone();
        if let Some(url) = text_row(
            ui,
            tk,
            cx.view,
            "server_url",
            &t("settings.assistant.server"),
            &restart(&t("settings.server_url.desc")),
            &stored,
            |v| {
                if is_http_url(v) {
                    Ok(())
                } else {
                    Err(t("settings.server_url.invalid"))
                }
            },
        ) {
            cx.settings.server_url = url.trim().to_owned();
        }
    }

    let (tone, status) = match cx.network {
        NetworkStatus::Connected => (Tone::Success, t("settings.connection.connected")),
        NetworkStatus::Connecting | NetworkStatus::Reconnecting => {
            (Tone::Neutral, t("settings.connection.connecting"))
        }
        NetworkStatus::Disconnected | NetworkStatus::Failed => {
            (Tone::Danger, t("settings.connection.not_connected"))
        }
    };
    let retry = t("settings.connection.retry");
    if status_row(
        ui,
        tk,
        &t("settings.connection"),
        &t("settings.connection.desc"),
        tone,
        &status,
        Some((&retry, !cx.settings.use_mock)),
    ) {
        cx.commands.push(SettingsCommand::RetryConnection);
    }
}
