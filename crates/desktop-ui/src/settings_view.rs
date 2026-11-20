//! The settings surface, built from the RF standard's settings pattern:
//! grouped sections with headings, one setting per row (label and optional
//! description on the left, control on the right), 1px dividers between rows.

use crate::theme::{Tokens, layout, size, space};
use crate::widgets::{self, setting_row, text};
use crate::{Theme, UiSettings};

/// Width reserved for a switch control.
const SWITCH_COL: f32 = layout::SWITCH_WIDTH;
/// Width reserved for the language segmented control.
const SEGMENTED_COL: f32 = 150.0;
/// Width of the theme select.
const SELECT_COL: f32 = 150.0;

/// Display name of a theme in the select. The RF theme is localised; the
/// legacy colour themes keep their own names.
pub fn theme_display_name(theme: &Theme) -> String {
    if theme.is_rf() {
        prxvc_i18n::t("settings.theme.rf")
    } else {
        theme.name.to_owned()
    }
}

/// Render every setting, grouped into sections.
///
/// `current_theme` is the name of the active theme; choosing another theme in
/// the select writes its name to `pending_theme`, which the app loop applies.
pub fn render_settings_content(
    ui: &mut egui::Ui,
    tk: &Tokens,
    settings: &mut UiSettings,
    current_theme: &str,
    pending_theme: &mut Option<String>,
) {
    // Vertical rhythm comes from the explicit paddings of the rows and the
    // section headers, not from the default item spacing.
    ui.spacing_mut().item_spacing.y = 0.0;

    // --- Appearance -------------------------------------------------------
    widgets::section_header(ui, tk, &prxvc_i18n::t("settings.appearance"), true);
    let themes = Theme::all();
    let names: Vec<String> = themes.iter().map(theme_display_name).collect();
    let selected = themes
        .iter()
        .position(|t| t.name == current_theme)
        .unwrap_or(0);
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.theme"),
        Some(&prxvc_i18n::t("settings.theme.desc")),
        SELECT_COL,
        false,
        |ui| {
            if let Some(i) = widgets::select(
                ui,
                tk,
                "settings_theme_select",
                SELECT_COL,
                &names[selected],
                &names,
                selected,
            ) {
                *pending_theme = Some(themes[i].name.to_owned());
            }
        },
    );

    // --- Language ---------------------------------------------------------
    widgets::section_header(ui, tk, &prxvc_i18n::t("settings.section.language"), false);
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.language"),
        Some(&prxvc_i18n::t("settings.language.desc")),
        SEGMENTED_COL,
        false,
        |ui| {
            let selected = usize::from(settings.language == "zh-CN");
            let labels = [
                prxvc_i18n::t("settings.language.en"),
                prxvc_i18n::t("settings.language.zh"),
            ];
            let label_refs = [labels[0].as_str(), labels[1].as_str()];
            match widgets::segmented(ui, tk, selected, &label_refs) {
                Some(0) => {
                    settings.language = "en".into();
                    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
                }
                Some(_) => {
                    settings.language = "zh-CN".into();
                    prxvc_i18n::set_locale(prxvc_i18n::Locale::ZhCn);
                }
                None => {}
            }
        },
    );

    // --- Audio ------------------------------------------------------------
    widgets::section_header(ui, tk, &prxvc_i18n::t("settings.section.audio"), false);
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.auto_play"),
        Some(&prxvc_i18n::t("settings.auto_play.desc")),
        SWITCH_COL,
        false,
        |ui| {
            widgets::switch(ui, tk, &mut settings.auto_play);
        },
    );

    // --- Privacy ----------------------------------------------------------
    widgets::section_header(ui, tk, &prxvc_i18n::t("settings.section.privacy"), false);
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.save_history"),
        Some(&prxvc_i18n::t("settings.save_history.desc")),
        SWITCH_COL,
        false,
        |ui| {
            widgets::switch(ui, tk, &mut settings.save_history);
        },
    );
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.show_transcript"),
        Some(&prxvc_i18n::t("settings.show_transcript.desc")),
        SWITCH_COL,
        false,
        |ui| {
            widgets::switch(ui, tk, &mut settings.show_transcript);
        },
    );

    // --- Connection -------------------------------------------------------
    widgets::section_header(ui, tk, &prxvc_i18n::t("settings.section.connection"), false);
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.use_mock"),
        Some(&prxvc_i18n::t("settings.use_mock.desc")),
        SWITCH_COL,
        false,
        |ui| {
            widgets::switch(ui, tk, &mut settings.use_mock);
        },
    );
    if !settings.use_mock {
        ui.add_space(space::S12);
        widgets::text_field(
            ui,
            tk,
            &prxvc_i18n::t("settings.server_url"),
            &mut settings.server_url,
            Some(&prxvc_i18n::t("settings.server_url.desc")),
            false,
        );
        ui.add_space(space::S12);
        let y = ui.cursor().top();
        let x0 = ui.min_rect().left();
        ui.painter().hline(
            x0..=x0 + ui.available_width(),
            y,
            crate::theme::hairline(tk.line),
        );
    }

    // --- System -----------------------------------------------------------
    widgets::section_header(ui, tk, &prxvc_i18n::t("settings.section.system"), false);
    setting_row(
        ui,
        tk,
        &prxvc_i18n::t("settings.hotkey"),
        Some(&prxvc_i18n::t("settings.hotkey.desc")),
        SWITCH_COL,
        true,
        |ui| {
            widgets::switch(ui, tk, &mut settings.hotkey_enabled);
        },
    );

    // --- Footer -----------------------------------------------------------
    ui.add_space(space::S16);
    ui.label(text(
        format!(
            "{} v{}",
            prxvc_i18n::t("app.name"),
            prxvc_i18n::t("app.version")
        ),
        size::CAPTION,
        tk.secondary,
    ));
}
