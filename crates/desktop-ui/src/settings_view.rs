//! The pages of the settings window: dispatch from a section to the file that draws it.
//!
//! Each `settings_<section>.rs` file draws one page of design 2.2 from the row vocabulary of
//! `settings_rows`. This file also keeps small helpers the pages share.

use egui::Ui;

use crate::Theme;
use crate::settings_rows::PageCx;
use crate::settings_section::SettingsSection;
use crate::{
    settings_about, settings_advanced, settings_assistant, settings_audio, settings_call,
    settings_general, settings_permissions, settings_pipeline, settings_privacy,
    settings_shortcuts, settings_voice,
};

/// Display name of a theme in the select. The RF theme is localised; the
/// legacy colour themes keep their own names.
pub fn theme_display_name(theme: &Theme) -> String {
    if theme.is_rf() {
        prxvc_i18n::t("settings.theme.rf")
    } else {
        theme.name.to_owned()
    }
}

/// Draw the page of `section`.
pub(crate) fn render_page(ui: &mut Ui, cx: &mut PageCx<'_>, section: SettingsSection) {
    match section {
        SettingsSection::General => settings_general::page(ui, cx),
        SettingsSection::CallTranslation => settings_call::page(ui, cx),
        SettingsSection::Assistant => settings_assistant::page(ui, cx),
        SettingsSection::Audio => settings_audio::page(ui, cx),
        SettingsSection::Voice => settings_voice::page(ui, cx),
        SettingsSection::SpeechPipeline => settings_pipeline::page(ui, cx),
        SettingsSection::Permissions => settings_permissions::page(ui, cx),
        SettingsSection::Shortcuts => settings_shortcuts::page(ui, cx),
        SettingsSection::Privacy => settings_privacy::page(ui, cx),
        SettingsSection::Advanced => settings_advanced::page(ui, cx),
        SettingsSection::About => settings_about::page(ui, cx),
    }
}

/// Index of `current` among `values`; the first when it is not one of them.
pub(crate) fn index_of(values: &[&str], current: &str) -> usize {
    values.iter().position(|v| *v == current).unwrap_or(0)
}

/// Translate several keys.
pub(crate) fn labels(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| prxvc_i18n::t(k)).collect()
}

/// Whether `text` is an `http` or `https` URL with a host.
pub(crate) fn is_http_url(text: &str) -> bool {
    let t = text.trim();
    let rest = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty() && !r.starts_with('/') && !r.contains(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_need_a_scheme_and_a_host() {
        assert!(is_http_url("http://localhost:3000"));
        assert!(is_http_url("https://example.org/api"));
        assert!(!is_http_url("localhost:3000"));
        assert!(!is_http_url("ftp://example.org"));
        assert!(!is_http_url("http://"));
        assert!(!is_http_url("http://a b"));
    }

    #[test]
    fn index_of_falls_back_to_the_first() {
        assert_eq!(index_of(&["a", "b"], "b"), 1);
        assert_eq!(index_of(&["a", "b"], "x"), 0);
    }
}
