//! Image snapshots of the settings window (egui_kittest): every section in light and dark in
//! English, and the General and Speech pipeline sections in Chinese.
//!
//! The window is drawn at 960 by 900 points, so the wide layout (section list and content column)
//! shows; the data is `common::rich_data`, fixed, so the images do not depend on the machine
//! they are made on except through the GPU and its driver. The reference machine is the developer
//! Mac the images were created on. Create or update with
//! `UPDATE_SNAPSHOTS=true cargo test -p prxvc-desktop-ui --test kittest_snapshots` and review the
//! PNGs before committing.

mod common;

use common::{WindowState, window_harness_in};
use egui::{Theme, vec2};
use prxvc_desktop_ui::SettingsSection;
use prxvc_i18n::Locale;

/// The window size of every image.
const SIZE: egui::Vec2 = vec2(960.0, 900.0);

fn snapshot(section: SettingsSection, scheme: Theme, locale: Locale) {
    let _guard = common::lock_locale();
    let tag = match locale {
        Locale::En => "en",
        Locale::ZhCn => "zh",
    };
    prxvc_i18n::set_locale(locale);
    let name = format!(
        "settings_{}_{}_{tag}",
        section.key(),
        if scheme == Theme::Light {
            "light"
        } else {
            "dark"
        }
    );
    let mut harness = window_harness_in(
        common::builder(SIZE, scheme).wgpu(),
        WindowState::new(section),
    );
    harness.input_mut().system_theme = Some(scheme);
    harness.run();
    harness.snapshot(&name);
    prxvc_i18n::set_locale(Locale::En);
}

macro_rules! english_pages {
    ($( $light:ident, $dark:ident => $section:expr ),+ $(,)?) => {$(
        #[test]
        fn $light() {
            snapshot($section, Theme::Light, Locale::En);
        }

        #[test]
        fn $dark() {
            snapshot($section, Theme::Dark, Locale::En);
        }
    )+};
}

english_pages! {
    general_light_en, general_dark_en => SettingsSection::General,
    call_light_en, call_dark_en => SettingsSection::CallTranslation,
    assistant_light_en, assistant_dark_en => SettingsSection::Assistant,
    audio_light_en, audio_dark_en => SettingsSection::Audio,
    voice_light_en, voice_dark_en => SettingsSection::Voice,
    pipeline_light_en, pipeline_dark_en => SettingsSection::SpeechPipeline,
    permissions_light_en, permissions_dark_en => SettingsSection::Permissions,
    shortcuts_light_en, shortcuts_dark_en => SettingsSection::Shortcuts,
    privacy_light_en, privacy_dark_en => SettingsSection::Privacy,
    advanced_light_en, advanced_dark_en => SettingsSection::Advanced,
    about_light_en, about_dark_en => SettingsSection::About,
}

#[test]
fn general_light_zh() {
    snapshot(SettingsSection::General, Theme::Light, Locale::ZhCn);
}

#[test]
fn pipeline_light_zh() {
    snapshot(SettingsSection::SpeechPipeline, Theme::Light, Locale::ZhCn);
}
