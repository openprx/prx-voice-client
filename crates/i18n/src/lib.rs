//! Internationalization support for PRX Voice Client.
//!
//! Provides compile-time embedded translations for English and Chinese (Simplified)
//! with a global locale store, fallback logic, and parameter substitution.

mod assistant_strings;
mod call_strings;
mod call_captions_strings;
mod call_window_strings;
mod settings_permission_strings;
mod settings_pipeline_strings;
mod settings_session_strings;
mod settings_strings;
mod translation_engine_strings;
mod translation_strings;

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::RwLock;

// ---------------------------------------------------------------------------
// Locale
// ---------------------------------------------------------------------------

/// Supported locales for the application UI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Locale {
    /// English (default).
    En,
    /// Simplified Chinese.
    ZhCn,
}

impl Locale {
    /// Parse a locale string into a [`Locale`] variant.
    ///
    /// Recognised inputs (case-insensitive): `"zh-cn"`, `"zh_cn"`, `"zh"`,
    /// `"chinese"` all map to [`Locale::ZhCn`]. Everything else maps to
    /// [`Locale::En`].
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "zh-cn" | "zh_cn" | "zh" | "chinese" => Locale::ZhCn,
            _ => Locale::En,
        }
    }

    /// Return the BCP-47-style tag for this locale.
    pub fn as_str(&self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhCn => "zh-CN",
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced by the i18n subsystem.
#[derive(Debug, thiserror::Error)]
pub enum I18nError {
    /// A translation key was not found in any loaded locale.
    #[error("translation key not found: {0}")]
    KeyNotFound(String),
    /// The requested locale has not been loaded.
    #[error("locale not loaded: {0}")]
    LocaleNotLoaded(String),
}

// ---------------------------------------------------------------------------
// Translation store (global)
// ---------------------------------------------------------------------------

/// Internal store holding loaded translations and the active locale.
struct TranslationStore {
    current_locale: Locale,
    translations: HashMap<Locale, HashMap<String, String>>,
}

/// Global translation store initialised with built-in English and Chinese data.
static STORE: LazyLock<RwLock<TranslationStore>> = LazyLock::new(|| {
    let mut translations = HashMap::new();
    translations.insert(Locale::En, build_en_translations());
    translations.insert(Locale::ZhCn, build_zh_cn_translations());
    RwLock::new(TranslationStore {
        current_locale: Locale::En,
        translations,
    })
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Set the active locale for all subsequent [`t`] / [`t_with`] calls.
pub fn set_locale(locale: Locale) {
    STORE.write().current_locale = locale;
}

/// Return the currently active locale.
pub fn current_locale() -> Locale {
    STORE.read().current_locale.clone()
}

/// Translate `key` using the active locale.
///
/// Fallback order:
/// 1. Current locale
/// 2. English
/// 3. The key itself (returned verbatim)
pub fn t(key: &str) -> String {
    let store = STORE.read();
    // Try current locale first.
    if let Some(map) = store.translations.get(&store.current_locale) {
        if let Some(val) = map.get(key) {
            return val.clone();
        }
    }
    // Fallback to English.
    if store.current_locale != Locale::En {
        if let Some(map) = store.translations.get(&Locale::En) {
            if let Some(val) = map.get(key) {
                return val.clone();
            }
        }
    }
    // Last resort: return the key itself.
    key.to_owned()
}

/// Translate `key` into `locale`, whatever the active locale is (the spoken notice to the other
/// party is in their language, not the interface's). Falls back to English, then to the key.
pub fn t_in(locale: Locale, key: &str) -> String {
    let store = STORE.read();
    store
        .translations
        .get(&locale)
        .and_then(|m| m.get(key))
        .or_else(|| store.translations.get(&Locale::En).and_then(|m| m.get(key)))
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

/// Translate `key` and substitute positional parameters.
///
/// Placeholders `{0}`, `{1}`, ... in the translated string are replaced with
/// the corresponding entries from `params`.
pub fn t_with(key: &str, params: &[&str]) -> String {
    let mut result = t(key);
    for (i, param) in params.iter().enumerate() {
        let placeholder = format!("{{{i}}}");
        result = result.replace(&placeholder, param);
    }
    result
}

/// Return all locales that have translations loaded.
pub fn available_locales() -> Vec<Locale> {
    let store = STORE.read();
    store.translations.keys().cloned().collect()
}

// ---------------------------------------------------------------------------
// Built-in translations — English
// ---------------------------------------------------------------------------

fn build_en_translations() -> HashMap<String, String> {
    let pairs: &[(&str, &str)] = &[
        // App
        ("app.name", "PRX Voice Client"),
        ("app.version", "0.1.0"),
        (
            "app.description",
            "An open-source, cross-platform, real-time voice AI client.",
        ),
        ("app.license", "MIT OR Apache-2.0"),
        // Navigation
        ("nav.session", "Session"),
        ("nav.history", "History"),
        ("nav.settings", "Settings"),
        ("nav.debug", "Debug"),
        ("nav.about", "About"),
        // Session states
        ("state.idle", "Idle"),
        ("state.standby", "Standby"),
        ("state.starting", "Starting"),
        ("state.listening", "Listening"),
        ("state.user_speaking", "User Speaking"),
        ("state.recognizing", "Recognizing"),
        ("state.thinking", "Thinking"),
        ("state.assistant_speaking", "Assistant Speaking"),
        ("state.interrupted", "Interrupted"),
        ("state.paused", "Paused"),
        ("state.recovering", "Recovering"),
        ("state.completed", "Completed"),
        ("state.failed", "Failed"),
        // Session controls
        ("session.start", "Start"),
        ("session.stop", "Stop"),
        ("session.interrupt", "Interrupt"),
        ("session.status", "Status"),
        ("session.transcript", "Transcript"),
        ("session.no_transcript", "No transcript yet"),
        ("session.controls", "Controls"),
        ("session.turn_count", "Turn {0} of {1}"),
        // History
        ("history.title", "Session History"),
        ("history.empty", "No sessions yet"),
        ("history.session_prefix", "Session"),
        ("history.date", "Date"),
        ("history.duration", "Duration"),
        ("history.state", "State"),
        ("history.clear", "Clear History"),
        (
            "history.clear_confirm",
            "Are you sure you want to clear all history?",
        ),
        // Settings
        ("settings.title", "Settings"),
        ("settings.language", "Language"),
        ("settings.audio", "Audio"),
        ("settings.input_device", "Input Device"),
        ("settings.output_device", "Output Device"),
        ("settings.auto_play", "Auto-play replies"),
        ("settings.save_history", "Save history"),
        ("settings.show_transcript", "Show transcript"),
        ("settings.hotkey", "Enable hotkey"),
        ("settings.privacy", "Privacy"),
        ("settings.debug_mode", "Debug mode"),
        ("settings.log_level", "Log level"),
        ("settings.saved", "Settings saved"),
        // Debug
        ("debug.title", "Debug"),
        ("debug.input_device", "Input device"),
        ("debug.state_machine", "State Machine"),
        ("debug.current_state", "Current State"),
        ("debug.events", "Recent Events"),
        ("debug.pending_commands", "Pending Commands"),
        ("debug.no_events", "No events recorded"),
        // About
        ("about.title", "About"),
        (
            "about.description",
            "An open-source, cross-platform, real-time voice AI client.",
        ),
        ("about.built_with", "Built with Rust + egui"),
        // Errors
        ("error.permission_denied", "Permission denied"),
        ("error.no_microphone", "No microphone available"),
        ("error.network_unavailable", "Network unavailable"),
        ("error.session_timeout", "Session timed out"),
        ("error.device_failed", "Audio device failed"),
        ("error.unknown", "An unknown error occurred"),
        ("error.recovery_failed", "Recovery failed"),
        ("error.title", "Error"),
        ("error.dismiss", "Dismiss"),
        // Network
        // Common
        ("common.ok", "OK"),
        ("common.cancel", "Cancel"),
        ("common.yes", "Yes"),
        ("common.no", "No"),
        ("common.save", "Save"),
        ("common.close", "Close"),
        ("common.loading", "Loading..."),
        ("common.retry", "Retry"),
        ("common.default_device", "Default"),
        ("tray.show", "Show PRX Voice"),
        ("tray.quit", "Quit"),
        // Orb UI
        ("orb.ready", "Ready to listen"),
        ("orb.listening", "Listening..."),
        ("orb.hearing", "Hearing you"),
        ("orb.processing", "Processing..."),
        ("orb.thinking", "Thinking..."),
        ("orb.speaking", "Speaking"),
        ("orb.interrupted", "Interrupted"),
        ("orb.ended", "Session ended"),
        (
            "session.hint.idle",
            "Click the orb or press Ctrl+Shift+Space",
        ),
        ("session.hint.listening", "Speak naturally"),
        ("session.hint.speaking", "Click to interrupt"),
        ("session.hint.mic_paused", "Mic paused while I speak, click to interrupt"),
        // Transcript
        ("transcript.you", "YOU"),
        ("transcript.ai", "AI"),
        // Settings sections
        ("settings.appearance", "Appearance"),
        ("settings.section.language", "Language"),
        ("settings.section.audio", "Audio"),
        ("settings.section.privacy", "Privacy"),
        ("settings.section.connection", "Connection"),
        ("settings.section.system", "System"),
        ("settings.theme", "Theme"),
        (
            "settings.theme.desc",
            "Colour theme of the window. RF follows the system light or dark setting.",
        ),
        ("settings.theme.rf", "RF (default)"),
        ("settings.language.desc", "Language of the interface."),
        ("settings.language.en", "English"),
        ("settings.language.zh", "\u{4e2d}\u{6587}"),
        (
            "settings.auto_play.desc",
            "Play the assistant's spoken replies automatically.",
        ),
        (
            "settings.save_history.desc",
            "Keep finished sessions on this device.",
        ),
        (
            "settings.show_transcript.desc",
            "Show the live transcript under the orb.",
        ),
        (
            "settings.hotkey.desc",
            "Start or stop a session with Ctrl+Shift+Space.",
        ),
        ("app.version_label", "Version"),
        ("app.license_label", "License"),
        // Status
        ("status.ready", "Ready"),
        ("status.offline", "Offline"),
        ("status.connecting", "Connecting"),
        ("status.reconnecting", "Reconnecting"),
        ("status.failed", "Failed"),
        // Debug sections
        ("debug.state_machine_label", "State Machine:"),
        ("debug.pending_commands_label", "Pending Commands:"),
        ("debug.recent_events", "Recent Events:"),
        ("debug.no_events_recorded", "No events recorded"),
        ("debug.no_events_short", "No events"),
    ];
    pairs
        .iter()
        .chain(assistant_strings::EN)
        .chain(call_strings::EN)
        .chain(call_window_strings::EN)
        .chain(call_captions_strings::EN)
        .chain(translation_strings::EN)
        .chain(translation_engine_strings::EN)
        .chain(settings_strings::EN)
        .chain(settings_session_strings::EN)
        .chain(settings_pipeline_strings::EN)
        .chain(settings_permission_strings::EN)
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

// ---------------------------------------------------------------------------
// Built-in translations — Chinese (Simplified)
// ---------------------------------------------------------------------------

fn build_zh_cn_translations() -> HashMap<String, String> {
    let pairs: &[(&str, &str)] = &[
        // App
        ("app.name", "PRX \u{8bed}\u{97f3}\u{5ba2}\u{6237}\u{7aef}"),
        ("app.version", "0.1.0"),
        (
            "app.description",
            "\u{5f00}\u{6e90}\u{3001}\u{8de8}\u{7aef}\u{3001}\u{5b9e}\u{65f6}\u{8bed}\u{97f3} AI \u{5ba2}\u{6237}\u{7aef}",
        ),
        ("app.license", "MIT OR Apache-2.0"),
        // Navigation
        ("nav.session", "\u{4f1a}\u{8bdd}"),
        ("nav.history", "\u{5386}\u{53f2}"),
        ("nav.settings", "\u{8bbe}\u{7f6e}"),
        ("nav.debug", "\u{8c03}\u{8bd5}"),
        ("nav.about", "\u{5173}\u{4e8e}"),
        // Session states
        ("state.idle", "\u{5f85}\u{673a}"),
        ("state.standby", "\u{5f85}\u{673a}"),
        ("state.starting", "\u{542f}\u{52a8}\u{4e2d}"),
        ("state.listening", "\u{8046}\u{542c}\u{4e2d}"),
        (
            "state.user_speaking",
            "\u{7528}\u{6237}\u{8bf4}\u{8bdd}\u{4e2d}",
        ),
        ("state.recognizing", "\u{8bc6}\u{522b}\u{4e2d}"),
        ("state.thinking", "\u{601d}\u{8003}\u{4e2d}"),
        (
            "state.assistant_speaking",
            "\u{52a9}\u{624b}\u{56de}\u{590d}\u{4e2d}",
        ),
        ("state.interrupted", "\u{5df2}\u{6253}\u{65ad}"),
        ("state.paused", "\u{5df2}\u{6682}\u{505c}"),
        ("state.recovering", "\u{6062}\u{590d}\u{4e2d}"),
        ("state.completed", "\u{5df2}\u{5b8c}\u{6210}"),
        ("state.failed", "\u{5931}\u{8d25}"),
        // Session controls
        ("session.start", "\u{5f00}\u{59cb}"),
        ("session.stop", "\u{7ed3}\u{675f}"),
        ("session.interrupt", "\u{6253}\u{65ad}"),
        ("session.status", "\u{72b6}\u{6001}"),
        ("session.transcript", "\u{6587}\u{672c}\u{8bb0}\u{5f55}"),
        (
            "session.no_transcript",
            "\u{6682}\u{65e0}\u{6587}\u{672c}\u{8bb0}\u{5f55}",
        ),
        ("session.controls", "\u{63a7}\u{5236}"),
        (
            "session.turn_count",
            "\u{7b2c} {0} \u{8f6e}\u{ff0c}\u{5171} {1} \u{8f6e}",
        ),
        // History
        ("history.title", "\u{4f1a}\u{8bdd}\u{5386}\u{53f2}"),
        (
            "history.empty",
            "\u{6682}\u{65e0}\u{4f1a}\u{8bdd}\u{8bb0}\u{5f55}",
        ),
        ("history.session_prefix", "\u{4f1a}\u{8bdd}"),
        ("history.date", "\u{65e5}\u{671f}"),
        ("history.duration", "\u{65f6}\u{957f}"),
        ("history.state", "\u{72b6}\u{6001}"),
        ("history.clear", "\u{6e05}\u{9664}\u{5386}\u{53f2}"),
        (
            "history.clear_confirm",
            "\u{786e}\u{5b9a}\u{8981}\u{6e05}\u{9664}\u{6240}\u{6709}\u{5386}\u{53f2}\u{8bb0}\u{5f55}\u{5417}\u{ff1f}",
        ),
        // Settings
        ("settings.title", "\u{8bbe}\u{7f6e}"),
        ("settings.language", "\u{8bed}\u{8a00}"),
        ("settings.audio", "\u{97f3}\u{9891}"),
        ("settings.input_device", "\u{8f93}\u{5165}\u{8bbe}\u{5907}"),
        ("settings.output_device", "\u{8f93}\u{51fa}\u{8bbe}\u{5907}"),
        (
            "settings.auto_play",
            "\u{81ea}\u{52a8}\u{64ad}\u{653e}\u{56de}\u{590d}",
        ),
        (
            "settings.save_history",
            "\u{4fdd}\u{5b58}\u{5386}\u{53f2}\u{8bb0}\u{5f55}",
        ),
        (
            "settings.show_transcript",
            "\u{663e}\u{793a}\u{6587}\u{672c}\u{8bb0}\u{5f55}",
        ),
        (
            "settings.hotkey",
            "\u{542f}\u{7528}\u{5feb}\u{6377}\u{952e}",
        ),
        ("settings.privacy", "\u{9690}\u{79c1}"),
        ("settings.debug_mode", "\u{8c03}\u{8bd5}\u{6a21}\u{5f0f}"),
        ("settings.log_level", "\u{65e5}\u{5fd7}\u{7ea7}\u{522b}"),
        ("settings.saved", "\u{8bbe}\u{7f6e}\u{5df2}\u{4fdd}\u{5b58}"),
        // Debug
        ("debug.title", "\u{8c03}\u{8bd5}"),
        ("debug.input_device", "\u{8f93}\u{5165}\u{8bbe}\u{5907}"),
        ("debug.state_machine", "\u{72b6}\u{6001}\u{673a}"),
        ("debug.current_state", "\u{5f53}\u{524d}\u{72b6}\u{6001}"),
        ("debug.events", "\u{6700}\u{8fd1}\u{4e8b}\u{4ef6}"),
        (
            "debug.pending_commands",
            "\u{5f85}\u{5904}\u{7406}\u{547d}\u{4ee4}",
        ),
        (
            "debug.no_events",
            "\u{6682}\u{65e0}\u{4e8b}\u{4ef6}\u{8bb0}\u{5f55}",
        ),
        // About
        ("about.title", "\u{5173}\u{4e8e}"),
        (
            "about.description",
            "\u{5f00}\u{6e90}\u{3001}\u{8de8}\u{7aef}\u{3001}\u{5b9e}\u{65f6}\u{8bed}\u{97f3} AI \u{5ba2}\u{6237}\u{7aef}",
        ),
        (
            "about.built_with",
            "\u{57fa}\u{4e8e} Rust + egui \u{6784}\u{5efa}",
        ),
        // Errors
        (
            "error.permission_denied",
            "\u{6743}\u{9650}\u{88ab}\u{62d2}\u{7edd}",
        ),
        (
            "error.no_microphone",
            "\u{65e0}\u{53ef}\u{7528}\u{9ea6}\u{514b}\u{98ce}",
        ),
        (
            "error.network_unavailable",
            "\u{7f51}\u{7edc}\u{4e0d}\u{53ef}\u{7528}",
        ),
        ("error.session_timeout", "\u{4f1a}\u{8bdd}\u{8d85}\u{65f6}"),
        (
            "error.device_failed",
            "\u{97f3}\u{9891}\u{8bbe}\u{5907}\u{5f02}\u{5e38}",
        ),
        (
            "error.unknown",
            "\u{53d1}\u{751f}\u{672a}\u{77e5}\u{9519}\u{8bef}",
        ),
        ("error.recovery_failed", "\u{6062}\u{590d}\u{5931}\u{8d25}"),
        ("error.title", "\u{9519}\u{8bef}"),
        ("error.dismiss", "\u{5ffd}\u{7565}"),
        // Network
        // Common
        ("common.ok", "\u{786e}\u{5b9a}"),
        ("common.cancel", "\u{53d6}\u{6d88}"),
        ("common.yes", "\u{662f}"),
        ("common.no", "\u{5426}"),
        ("common.save", "\u{4fdd}\u{5b58}"),
        ("common.close", "\u{5173}\u{95ed}"),
        ("common.loading", "\u{52a0}\u{8f7d}\u{4e2d}..."),
        ("common.retry", "\u{91cd}\u{8bd5}"),
        ("common.default_device", "\u{9ed8}\u{8ba4}"),
        ("tray.show", "\u{663e}\u{793a} PRX Voice"),
        ("tray.quit", "\u{9000}\u{51fa}"),
        // Orb UI
        ("orb.ready", "\u{51c6}\u{5907}\u{8046}\u{542c}"),
        ("orb.listening", "\u{8046}\u{542c}\u{4e2d}..."),
        ("orb.hearing", "\u{6b63}\u{5728}\u{542c}\u{4f60}\u{8bf4}"),
        ("orb.processing", "\u{5904}\u{7406}\u{4e2d}..."),
        ("orb.thinking", "\u{601d}\u{8003}\u{4e2d}..."),
        ("orb.speaking", "\u{56de}\u{590d}\u{4e2d}"),
        ("orb.interrupted", "\u{5df2}\u{6253}\u{65ad}"),
        ("orb.ended", "\u{4f1a}\u{8bdd}\u{7ed3}\u{675f}"),
        (
            "session.hint.idle",
            "\u{70b9}\u{51fb}\u{58f0}\u{7eb9}\u{7403}\u{6216}\u{6309} Ctrl+Shift+Space",
        ),
        ("session.hint.listening", "\u{81ea}\u{7136}\u{5730}\u{8bf4}\u{8bdd}"),
        ("session.hint.speaking", "\u{70b9}\u{51fb}\u{6253}\u{65ad}"),
        (
            "session.hint.mic_paused",
            "\u{56de}\u{590d}\u{65f6}\u{9ea6}\u{514b}\u{98ce}\u{6682}\u{505c}\u{ff0c}\u{70b9}\u{51fb}\u{6253}\u{65ad}",
        ),
        // Transcript
        ("transcript.you", "\u{4f60}"),
        ("transcript.ai", "AI"),
        // Settings sections
        ("settings.appearance", "\u{5916}\u{89c2}"),
        ("settings.section.language", "\u{8bed}\u{8a00}"),
        ("settings.section.audio", "\u{97f3}\u{9891}"),
        ("settings.section.privacy", "\u{9690}\u{79c1}"),
        ("settings.section.connection", "\u{8fde}\u{63a5}"),
        ("settings.section.system", "\u{7cfb}\u{7edf}"),
        ("settings.theme", "\u{4e3b}\u{9898}"),
        (
            "settings.theme.desc",
            "\u{7a97}\u{53e3}\u{7684}\u{914d}\u{8272}\u{4e3b}\u{9898}\u{3002}RF \u{4e3b}\u{9898}\u{8ddf}\u{968f}\u{7cfb}\u{7edf}\u{7684}\u{6d45}\u{8272}\u{6216}\u{6df1}\u{8272}\u{8bbe}\u{7f6e}\u{3002}",
        ),
        ("settings.theme.rf", "RF\u{ff08}\u{9ed8}\u{8ba4}\u{ff09}"),
        ("settings.language.desc", "\u{754c}\u{9762}\u{4f7f}\u{7528}\u{7684}\u{8bed}\u{8a00}\u{3002}"),
        ("settings.language.en", "English"),
        ("settings.language.zh", "\u{4e2d}\u{6587}"),
        ("settings.auto_play.desc", "\u{81ea}\u{52a8}\u{64ad}\u{653e}\u{52a9}\u{624b}\u{7684}\u{8bed}\u{97f3}\u{56de}\u{590d}\u{3002}"),
        ("settings.save_history.desc", "\u{5c06}\u{5df2}\u{7ed3}\u{675f}\u{7684}\u{4f1a}\u{8bdd}\u{4fdd}\u{5b58}\u{5728}\u{672c}\u{8bbe}\u{5907}\u{4e0a}\u{3002}"),
        ("settings.show_transcript.desc", "\u{5728}\u{5149}\u{7403}\u{4e0b}\u{65b9}\u{663e}\u{793a}\u{5b9e}\u{65f6}\u{6587}\u{5b57}\u{8bb0}\u{5f55}\u{3002}"),
        ("settings.hotkey.desc", "\u{4f7f}\u{7528} Ctrl+Shift+Space \u{5f00}\u{59cb}\u{6216}\u{7ed3}\u{675f}\u{4f1a}\u{8bdd}\u{3002}"),
        ("app.version_label", "\u{7248}\u{672c}"),
        ("app.license_label", "\u{8bb8}\u{53ef}\u{8bc1}"),
        // Status
        ("status.ready", "\u{5c31}\u{7eea}"),
        ("status.offline", "\u{79bb}\u{7ebf}"),
        ("status.connecting", "\u{8fde}\u{63a5}\u{4e2d}"),
        ("status.reconnecting", "\u{91cd}\u{8fde}\u{4e2d}"),
        ("status.failed", "\u{5931}\u{8d25}"),
        // Debug sections
        ("debug.state_machine_label", "\u{72b6}\u{6001}\u{673a}\u{ff1a}"),
        ("debug.pending_commands_label", "\u{5f85}\u{5904}\u{7406}\u{547d}\u{4ee4}\u{ff1a}"),
        ("debug.recent_events", "\u{6700}\u{8fd1}\u{4e8b}\u{4ef6}\u{ff1a}"),
        ("debug.no_events_recorded", "\u{6682}\u{65e0}\u{4e8b}\u{4ef6}\u{8bb0}\u{5f55}"),
        ("debug.no_events_short", "\u{6682}\u{65e0}\u{4e8b}\u{4ef6}"),
    ];
    pairs
        .iter()
        .chain(assistant_strings::ZH_CN)
        .chain(call_strings::ZH_CN)
        .chain(call_window_strings::ZH_CN)
        .chain(call_captions_strings::ZH_CN)
        .chain(translation_strings::ZH_CN)
        .chain(translation_engine_strings::ZH_CN)
        .chain(settings_strings::ZH_CN)
        .chain(settings_session_strings::ZH_CN)
        .chain(settings_pipeline_strings::ZH_CN)
        .chain(settings_permission_strings::ZH_CN)
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for common usage.
pub mod prelude {
    pub use crate::{I18nError, Locale, available_locales, current_locale, set_locale, t, t_with};
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the global locale to prevent data races
    /// between parallel test threads.
    static TEST_LOCK: LazyLock<parking_lot::Mutex<()>> =
        LazyLock::new(|| parking_lot::Mutex::new(()));

    /// Acquire the test lock and reset locale to English.
    fn lock_and_reset() -> parking_lot::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock();
        set_locale(Locale::En);
        guard
    }

    #[test]
    fn test_default_locale_is_english() {
        let _g = lock_and_reset();
        assert_eq!(current_locale(), Locale::En);
    }

    #[test]
    fn test_set_locale_to_chinese() {
        let _g = lock_and_reset();
        set_locale(Locale::ZhCn);
        assert_eq!(current_locale(), Locale::ZhCn);
    }

    #[test]
    fn test_translate_english() {
        let _g = lock_and_reset();
        assert_eq!(t("app.name"), "PRX Voice Client");
        assert_eq!(t("nav.session"), "Session");
        assert_eq!(t("common.ok"), "OK");
        assert_eq!(t("state.listening"), "Listening");
        assert_eq!(t("error.unknown"), "An unknown error occurred");
    }

    #[test]
    fn test_translate_chinese() {
        let _g = lock_and_reset();
        set_locale(Locale::ZhCn);
        assert_eq!(t("nav.session"), "\u{4f1a}\u{8bdd}");
        assert_eq!(t("common.ok"), "\u{786e}\u{5b9a}");
        assert_eq!(t("state.listening"), "\u{8046}\u{542c}\u{4e2d}");
    }

    #[test]
    fn test_fallback_to_english() {
        let _g = lock_and_reset();
        set_locale(Locale::ZhCn);
        // All built-in keys exist in both locales, so we verify that a known
        // key produces a non-empty result (not the raw key) when zh-CN is active.
        let result = t("app.name");
        assert!(!result.is_empty());
        assert_ne!(result, "app.name");
    }

    #[test]
    fn test_missing_key_returns_key_itself() {
        let _g = lock_and_reset();
        let missing = "this.key.does.not.exist";
        assert_eq!(t(missing), missing);
    }

    #[test]
    fn test_t_with_params() {
        let _g = lock_and_reset();
        let result = t_with("session.turn_count", &["3", "10"]);
        assert_eq!(result, "Turn 3 of 10");

        set_locale(Locale::ZhCn);
        let result_zh = t_with("session.turn_count", &["3", "10"]);
        assert_eq!(result_zh, "\u{7b2c} 3 \u{8f6e}\u{ff0c}\u{5171} 10 \u{8f6e}");
    }

    #[test]
    fn test_locale_from_str_variants() {
        // No global state mutation — lock not strictly needed, but consistent.
        let _g = lock_and_reset();
        assert_eq!(Locale::from_str("zh"), Locale::ZhCn);
        assert_eq!(Locale::from_str("zh-CN"), Locale::ZhCn);
        assert_eq!(Locale::from_str("zh-cn"), Locale::ZhCn);
        assert_eq!(Locale::from_str("zh_cn"), Locale::ZhCn);
        assert_eq!(Locale::from_str("ZH_CN"), Locale::ZhCn);
        assert_eq!(Locale::from_str("Chinese"), Locale::ZhCn);
        assert_eq!(Locale::from_str("chinese"), Locale::ZhCn);
        assert_eq!(Locale::from_str("en"), Locale::En);
        assert_eq!(Locale::from_str("EN"), Locale::En);
        assert_eq!(Locale::from_str("english"), Locale::En);
        assert_eq!(Locale::from_str("fr"), Locale::En); // unknown falls back to En
    }

    #[test]
    fn test_available_locales() {
        let _g = lock_and_reset();
        let locales = available_locales();
        assert!(locales.contains(&Locale::En));
        assert!(locales.contains(&Locale::ZhCn));
        assert_eq!(locales.len(), 2);
    }

    #[test]
    fn test_current_locale_after_set() {
        let _g = lock_and_reset();
        assert_eq!(current_locale(), Locale::En);
        set_locale(Locale::ZhCn);
        assert_eq!(current_locale(), Locale::ZhCn);
        set_locale(Locale::En);
        assert_eq!(current_locale(), Locale::En);
    }
}
