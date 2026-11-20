//! Settings page General: mode, interface language, theme and main-window behaviour (design 2.2).

use egui::Ui;

use crate::Theme;
use crate::settings_rows::{
    PageCx, saved_only, segmented_row, select_row, switch_row, t, unavailable_row,
};
use crate::settings_view::{index_of, labels, theme_display_name};

/// Values of `app.mode` in the order of the segmented control.
const MODES: [&str; 2] = ["call", "assistant"];
/// Values of `ui.language` in the order of the select.
const LANGUAGES: [&str; 3] = ["system", "en", "zh-CN"];

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;

    let options = labels(&["settings.mode.call", "settings.mode.assistant"]);
    let selected = index_of(&MODES, &cx.settings.mode);
    if let Some(i) = segmented_row(
        ui,
        tk,
        &t("settings.mode"),
        &saved_only(&t("settings.mode.desc")),
        &options,
        selected,
    ) {
        cx.settings.mode = MODES[i].to_owned();
    }

    let options = labels(&[
        "settings.language.system",
        "settings.language.en",
        "settings.language.zh",
    ]);
    let selected = index_of(&LANGUAGES, &cx.settings.language);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_language",
        &t("settings.general.language"),
        &t("settings.language.desc"),
        &options,
        selected,
    ) {
        cx.settings.language = LANGUAGES[i].to_owned();
        match LANGUAGES[i] {
            "en" => prxvc_i18n::set_locale(prxvc_i18n::Locale::En),
            "zh-CN" => prxvc_i18n::set_locale(prxvc_i18n::Locale::ZhCn),
            // "system" is resolved against the operating system by the app.
            _ => {}
        }
    }

    let themes = Theme::all();
    let names: Vec<String> = themes.iter().map(theme_display_name).collect();
    let selected = themes
        .iter()
        .position(|t| t.name == cx.current_theme)
        .unwrap_or(0);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_theme",
        &t("settings.theme"),
        &t("settings.theme.desc"),
        &names,
        selected,
    ) {
        cx.settings.theme = themes[i].name.to_owned();
        *cx.pending_theme = Some(themes[i].name.to_owned());
    }

    switch_row(
        ui,
        tk,
        &t("settings.general.captions"),
        &saved_only(&t("settings.show_transcript.desc")),
        &mut cx.settings.show_transcript,
        true,
    );
    switch_row(
        ui,
        tk,
        &t("settings.compact"),
        &saved_only(&t("settings.compact.desc")),
        &mut cx.settings.compact_mode,
        true,
    );
    switch_row(
        ui,
        tk,
        &t("settings.always_on_top"),
        &t("settings.always_on_top.desc"),
        &mut cx.settings.always_on_top,
        true,
    );
    unavailable_row(
        ui,
        tk,
        &t("settings.launch_at_login"),
        &t("settings.launch_at_login.desc"),
        "settings.reason.login_item",
        Some(&t(if cx.settings.launch_at_login {
            "common.on"
        } else {
            "common.off"
        })),
    );
}
