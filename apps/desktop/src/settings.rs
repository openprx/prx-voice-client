//! Mapping between the loaded [`AppConfig`] and the settings popup state.
//!
//! The popup shows a subset of the configuration. Loading copies that subset
//! into [`UiSettings`]; saving writes only that subset back into the loaded
//! configuration, so every field the popup does not show (provider names,
//! network language, timeouts, ...) is preserved as it was loaded.

use prxvc_config::AppConfig;
use prxvc_desktop_ui::UiSettings;

/// Initialise the popup state from the configuration in effect.
pub fn ui_settings_from_config(config: &AppConfig) -> UiSettings {
    UiSettings {
        language: config.ui.language.clone(),
        auto_play: config.audio.auto_play_reply,
        save_history: config.privacy.save_history,
        server_url: config.network.server_url.clone(),
        use_mock: config.network.use_mock,
        ..UiSettings::default()
    }
}

/// Write the popup-edited fields into `config`, leaving everything else as is.
pub fn apply_ui_settings(config: &mut AppConfig, settings: &UiSettings) {
    config.network.use_mock = settings.use_mock;
    config.network.server_url = settings.server_url.clone();
    config.ui.language = settings.language.clone();
    config.audio.auto_play_reply = settings.auto_play;
    config.privacy.save_history = settings.save_history;
}

/// The persisted subset of the popup state, used to detect changes.
pub type SettingsSnapshot = (bool, String, String, bool, bool);

/// Snapshot of the fields [`apply_ui_settings`] persists.
pub fn snapshot(settings: &UiSettings) -> SettingsSnapshot {
    (
        settings.use_mock,
        settings.server_url.clone(),
        settings.language.clone(),
        settings.auto_play,
        settings.save_history,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_config() -> AppConfig {
        let mut c = AppConfig::default();
        c.network.use_mock = false;
        c.network.server_url = "http://10.0.0.5:3000".to_owned();
        c.network.language = "zh-CN".to_owned();
        c.network.asr_provider = "sherpa".to_owned();
        c.network.agent_provider = "ollama".to_owned();
        c.network.tts_provider = "kokoro".to_owned();
        c.network.reconnect_attempts = 9;
        c.ui.language = "zh-CN".to_owned();
        c.audio.auto_play_reply = false;
        c.privacy.save_history = false;
        c
    }

    #[test]
    fn startup_state_reflects_the_loaded_config() {
        let ui = ui_settings_from_config(&live_config());
        assert!(!ui.use_mock);
        assert_eq!(ui.server_url, "http://10.0.0.5:3000");
        assert_eq!(ui.language, "zh-CN");
        assert!(!ui.auto_play);
        assert!(!ui.save_history);
    }

    #[test]
    fn changing_the_server_url_keeps_live_mode_and_providers() {
        let mut config = live_config();
        let mut ui = ui_settings_from_config(&config);
        ui.server_url = "http://example.test:4000".to_owned();
        apply_ui_settings(&mut config, &ui);

        assert!(!config.network.use_mock, "use_mock must stay false");
        assert_eq!(config.network.server_url, "http://example.test:4000");
        assert_eq!(config.network.asr_provider, "sherpa");
        assert_eq!(config.network.agent_provider, "ollama");
        assert_eq!(config.network.tts_provider, "kokoro");
        assert_eq!(config.network.language, "zh-CN");
        assert_eq!(config.network.reconnect_attempts, 9);
    }

    #[test]
    fn changing_the_ui_language_does_not_touch_the_session_language() {
        let mut config = live_config();
        let mut ui = ui_settings_from_config(&config);
        ui.language = "en".to_owned();
        apply_ui_settings(&mut config, &ui);
        assert_eq!(config.ui.language, "en");
        assert_eq!(config.network.language, "zh-CN");
        assert!(!config.network.use_mock);
    }

    #[test]
    fn unchanged_state_round_trips_to_an_identical_config() {
        let before = live_config();
        let mut after = before.clone();
        apply_ui_settings(&mut after, &ui_settings_from_config(&before));
        assert_eq!(format!("{before:?}"), format!("{after:?}"));
    }

    #[test]
    fn snapshot_changes_only_with_persisted_fields() {
        let ui = ui_settings_from_config(&live_config());
        let base = snapshot(&ui);
        let mut other = ui_settings_from_config(&live_config());
        other.show_transcript = !other.show_transcript;
        assert_eq!(snapshot(&other), base);
        other.use_mock = true;
        assert_ne!(snapshot(&other), base);
    }
}
