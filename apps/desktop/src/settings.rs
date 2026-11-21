//! Mapping between the loaded [`AppConfig`] and the state of the settings window.
//!
//! Every function here is pure. Loading copies the config keys the window shows into
//! [`UiSettings`]; saving writes into a config only the fields the user changed
//! (`before` against `after`), so every other key, including one that was edited by hand while
//! the window was open, is kept as it is. The write itself is `AppConfigManager::update`, which
//! re-reads the file, applies a closure that calls [`apply_changes`], and writes atomically.
//!
//! Three kinds of field: *plain* fields are copied as they are (one macro lists them, and the
//! tests check that each one is covered), *converted* fields (`theme`, `echo_mode`) go through
//! a mapping, and *read-only* fields are shown but never written by the window.

use prxvc_config::{AppConfig, EchoMode, SettingsWindowConfig};
use prxvc_desktop_ui::tool_window::Geometry;
use prxvc_desktop_ui::{SettingsSection, UiSettings};

/// The last state of the window known to be stored, used to find what changed.
pub type SettingsSnapshot = UiSettings;

/// The state the window starts from for `config`.
pub fn ui_settings_from_config(config: &AppConfig) -> UiSettings {
    let mut ui = UiSettings::default();
    read_plain(&mut ui, config);
    ui.theme = theme_from_config(&config.ui.theme);
    ui.echo_mode = echo_mode_to_text(config.audio.echo_mode).to_owned();
    ui.dialogue_provider = dialogue_provider_of(config);
    // Read-only here: shown, never written by the window.
    ui.sample_rate = config.audio.sample_rate;
    ui.channels = config.audio.channels;
    ui.voice_profile_id = config.voice.self_profile_id.clone();
    ui.providers_preset = config.providers.preset.clone();
    ui.hotkey_session_toggle = config.hotkeys.session_toggle.clone();
    ui.hotkey_push_to_talk = config.hotkeys.push_to_talk.clone();
    ui.hotkey_interrupt = config.hotkeys.interrupt.clone();
    ui
}

/// The state to remember after a save: what the window shows.
pub fn snapshot(settings: &UiSettings) -> SettingsSnapshot {
    settings.clone()
}

/// Write into `config` each field that differs between `before` and `after`, and nothing else.
pub fn apply_changes(config: &mut AppConfig, before: &UiSettings, after: &UiSettings) {
    write_plain(config, before, after);
    if before.theme != after.theme {
        config.ui.theme = theme_to_config(&after.theme);
    }
    if before.echo_mode != after.echo_mode {
        config.audio.echo_mode = echo_mode_from_text(&after.echo_mode);
    }
    if before.dialogue_provider != after.dialogue_provider {
        set_dialogue_provider(config, &after.dialogue_provider);
    }
    config.normalize();
}

/// Put the old value back into `current` for each field that `before` to `after` changed and
/// that still shows the new value (a save failed; edits made since stay).
pub fn revert_changes(current: &mut UiSettings, before: &UiSettings, after: &UiSettings) {
    revert_plain(current, before, after);
    if before.theme != after.theme && current.theme == after.theme {
        current.theme = before.theme.clone();
    }
    if before.echo_mode != after.echo_mode && current.echo_mode == after.echo_mode {
        current.echo_mode = before.echo_mode.clone();
    }
    if before.dialogue_provider != after.dialogue_provider
        && current.dialogue_provider == after.dialogue_provider
    {
        current.dialogue_provider = before.dialogue_provider.clone();
    }
}

/// Fields copied as they are: `ui field => config key`.
macro_rules! plain_fields {
    ($($field:ident => $($seg:ident).+),+ $(,)?) => {
        fn read_plain(ui: &mut UiSettings, config: &AppConfig) {
            $( ui.$field = config.$($seg).+.clone(); )+
        }

        fn write_plain(config: &mut AppConfig, before: &UiSettings, after: &UiSettings) {
            $( if before.$field != after.$field { config.$($seg).+ = after.$field.clone(); } )+
        }

        fn revert_plain(current: &mut UiSettings, before: &UiSettings, after: &UiSettings) {
            $(
                if before.$field != after.$field && current.$field == after.$field {
                    current.$field = before.$field.clone();
                }
            )+
        }

        /// Names of the plain fields, for the test that every one has a case.
        #[cfg(test)]
        const PLAIN_FIELDS: &[&str] = &[ $( stringify!($field) ),+ ];

        /// The config keys of the plain fields, for the test that every key of the schema has
        /// a decision.
        #[cfg(test)]
        const PLAIN_KEYS: &[&str] = &[ $( concat!($( stringify!($seg), "." ),+) ),+ ];
    };
}

plain_fields! {
    input_device => audio.input_device,
    output_device => audio.output_device,
    language => ui.language,
    auto_play => audio.auto_play_reply,
    save_history => privacy.save_history,
    show_transcript => ui.show_transcript,
    hotkey_enabled => hotkeys.enabled,
    mode => app.mode,
    compact_mode => ui.compact_mode,
    always_on_top => ui.always_on_top,
    launch_at_login => app.launch_at_login,
    call_self_language => call.self_language,
    call_peer_language => call.peer_language,
    call_target_app => call.target_app,
    call_local_output_mode => call.local_output_mode,
    call_outgoing_capture => call.outgoing_capture,
    call_captions_show_source => call.captions_show_source,
    call_captions_size => call.captions_size,
    call_disclosure => call.disclosure,
    call_barge_in => call.barge_in,
    assistant_language => assistant.language,
    assistant_voice => assistant.voice,
    assistant_voice_preset => assistant.voice_preset,
    virtual_mic_device => audio.virtual_mic_device,
    voice_use_clone => voice.use_clone_outgoing,
    voice_incoming_preset => voice.incoming_preset,
    voice_fallback_preset => voice.fallback_preset,
    models_preload => models.preload,
    models_warmup => models.warmup,
    models_endpoint => models.endpoint,
    models_max_concurrent => models.max_concurrent,
    models_dir => models.dir,
    models_catalogue_url => models.catalogue_url,
    models_catalogue_auto_update => models.catalogue_auto_update,
    hotkey_assistant_mode => hotkeys.assistant_mode,
    save_transcript => privacy.save_transcript,
    log_sensitive => privacy.log_sensitive,
    debug_enabled => debug.enabled,
    debug_show_panel => debug.show_debug_panel,
    debug_log_level => debug.log_level,
    offline => network_policy.offline,
    allow_hosts => network_policy.allow_hosts,
}

// ---------------------------------------------------------------------------
// Converted fields
// ---------------------------------------------------------------------------

/// Config text of the default theme.
const RF_CONFIG_NAME: &str = "rf";

/// The theme name the window uses for the config text `text` (the RF theme is `"rf"` in the
/// file and `"RF"` in the window; a name the window does not know is kept as it is).
pub fn theme_from_config(text: &str) -> String {
    if text.trim().is_empty() || text.eq_ignore_ascii_case(RF_CONFIG_NAME) {
        prxvc_desktop_ui::theme::RF_THEME_NAME.to_owned()
    } else {
        text.to_owned()
    }
}

/// The config text of a theme name of the window.
pub fn theme_to_config(name: &str) -> String {
    if name == prxvc_desktop_ui::theme::RF_THEME_NAME {
        RF_CONFIG_NAME.to_owned()
    } else {
        name.to_owned()
    }
}

/// `providers.active.dialogue`: the id of the dialogue backend; empty (not set) is the local server.
fn dialogue_provider_of(config: &AppConfig) -> String {
    config
        .providers
        .active
        .dialogue
        .get("")
        .cloned()
        .unwrap_or_default()
}

/// Select the dialogue backend `id`; an empty id removes the selection (the local server).
fn set_dialogue_provider(config: &mut AppConfig, id: &str) {
    if id.trim().is_empty() {
        config.providers.active.dialogue.remove("");
    } else {
        config
            .providers
            .active
            .dialogue
            .insert(String::new(), id.trim().to_owned());
    }
}

fn echo_mode_to_text(mode: EchoMode) -> &'static str {
    match mode {
        EchoMode::Auto => "auto",
        EchoMode::Always => "always",
        EchoMode::Off => "off",
    }
}

fn echo_mode_from_text(text: &str) -> EchoMode {
    match text {
        "always" => EchoMode::Always,
        "off" => EchoMode::Off,
        _ => EchoMode::Auto,
    }
}

// ---------------------------------------------------------------------------
// Interface language
// ---------------------------------------------------------------------------

/// The interface locale for `ui.language`. `"system"` follows `system` (the operating system's
/// language tag, for example `zh-Hans-CN`); an unknown value or an unknown system language is
/// English.
pub fn resolve_locale(language: &str, system: Option<&str>) -> prxvc_i18n::Locale {
    let tag = match language {
        "system" => system.unwrap_or(""),
        other => other,
    };
    if tag.to_ascii_lowercase().starts_with("zh") {
        prxvc_i18n::Locale::ZhCn
    } else {
        prxvc_i18n::Locale::En
    }
}

// ---------------------------------------------------------------------------
// Window memory
// ---------------------------------------------------------------------------

/// What `ui.settings_window` remembers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedWindow {
    /// Position and size, with the size of the monitor they were recorded on.
    pub geometry: Option<(Geometry, Option<egui::Vec2>)>,
    /// The page that was open.
    pub section: Option<SettingsSection>,
}

/// Read `ui.settings_window`. A value that is not a usable geometry is ignored.
pub fn window_from_config(window: &SettingsWindowConfig) -> SavedWindow {
    let geometry = match (window.x, window.y, window.width, window.height) {
        (Some(x), Some(y), Some(w), Some(h)) => Geometry::from_tuple((x, y, w, h)).map(|g| {
            let monitor = match (window.monitor_width, window.monitor_height) {
                (Some(w), Some(h)) if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 => {
                    Some(egui::vec2(w, h))
                }
                _ => None,
            };
            (g, monitor)
        }),
        _ => None,
    };
    SavedWindow {
        geometry,
        section: window.page.as_deref().and_then(SettingsSection::from_key),
    }
}

/// The value to store in `ui.settings_window`.
pub fn window_to_config(
    geometry: Option<Geometry>,
    monitor: Option<egui::Vec2>,
    section: SettingsSection,
) -> SettingsWindowConfig {
    SettingsWindowConfig {
        x: geometry.map(|g| g.pos.x),
        y: geometry.map(|g| g.pos.y),
        width: geometry.map(|g| g.size.x),
        height: geometry.map(|g| g.size.y),
        monitor_width: monitor.map(|m| m.x),
        monitor_height: monitor.map(|m| m.y),
        page: Some(section.key().to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config with a non-default value in every key the window shows.
    fn busy_config() -> AppConfig {
        let mut c = AppConfig::default();
        c.assistant.language = "en".to_owned();
        c.assistant.voice = "clone".to_owned();
        c.assistant.voice_preset = "calm-zh".to_owned();
        c.providers
            .active
            .dialogue
            .insert(String::new(), "cloud-chat".to_owned());
        c.ui.language = "zh-CN".to_owned();
        c.ui.theme = "Midnight".to_owned();
        c.audio.auto_play_reply = false;
        c.audio.echo_mode = EchoMode::Always;
        c.audio.input_device = "usb-mic".to_owned();
        c.audio.output_device = "bt-out".to_owned();
        c.audio.virtual_mic_device = "other.sink".to_owned();
        c.privacy.save_history = false;
        c.privacy.save_transcript = true;
        c.privacy.log_sensitive = true;
        c.call.self_language = "zh".to_owned();
        c.call.peer_language = "en".to_owned();
        c.call.target_app = "net.whatsapp.WhatsApp".to_owned();
        c.call.local_output_mode = "both".to_owned();
        c.call.outgoing_capture = "push_to_talk".to_owned();
        c.call.captions_show_source = false;
        c.call.captions_size = "large".to_owned();
        c.call.disclosure = false;
        c.call.barge_in = false;
        c.voice.self_profile_id = "me".to_owned();
        c.voice.use_clone_outgoing = false;
        c.voice.incoming_preset = "warm-en".to_owned();
        c.voice.fallback_preset = "calm-zh".to_owned();
        c.models.preload = false;
        c.models.warmup = false;
        c.models.endpoint = "https://mirror.example.org".to_owned();
        c.models.max_concurrent = 3;
        c.models.dir = "/data/models".to_owned();
        c.models.catalogue_url = "https://example.org/catalogue.json".to_owned();
        c.models.catalogue_auto_update = true;
        c.hotkeys.enabled = false;
        c.hotkeys.session_toggle = "Alt+Space".to_owned();
        c.hotkeys.push_to_talk = "F5".to_owned();
        c.hotkeys.interrupt = "F6".to_owned();
        c.hotkeys.assistant_mode = "hold".to_owned();
        c.debug.enabled = true;
        c.debug.show_debug_panel = true;
        c.debug.log_level = "debug".to_owned();
        c.app.mode = "call".to_owned();
        c.app.launch_at_login = true;
        c.ui.compact_mode = true;
        c.ui.always_on_top = false;
        c.ui.show_transcript = false;
        c.network_policy.offline = true;
        c.network_policy.allow_hosts = vec!["192.168.1.20".to_owned()];
        c.providers.preset = "hybrid".to_owned();
        c.normalize();
        c
    }

    #[test]
    fn startup_state_reflects_every_key_of_the_loaded_config() {
        let c = busy_config();
        let ui = ui_settings_from_config(&c);
        assert_eq!(ui.assistant_language, "en");
        assert_eq!(ui.assistant_voice, "clone");
        assert_eq!(ui.assistant_voice_preset, "calm-zh");
        assert_eq!(ui.dialogue_provider, "cloud-chat");
        assert_eq!(ui.language, "zh-CN");
        assert_eq!(ui.theme, "Midnight");
        assert_eq!(ui.echo_mode, "always");
        assert_eq!(ui.input_device, "usb-mic");
        assert_eq!(ui.models_max_concurrent, 3);
        assert_eq!(ui.allow_hosts, ["192.168.1.20"]);
        assert_eq!(ui.sample_rate, 16_000);
        assert_eq!(ui.voice_profile_id, "me");
        assert_eq!(ui.providers_preset, "hybrid");
        assert_eq!(ui.hotkey_push_to_talk, "F5");
        assert!(!ui.hotkey_enabled);
        assert!(ui.offline);
    }

    #[test]
    fn unchanged_state_round_trips_to_an_identical_config() {
        let before = busy_config();
        let ui = ui_settings_from_config(&before);
        let mut after = before.clone();
        apply_changes(&mut after, &ui, &ui);
        assert_eq!(before, after);
        // Also from the defaults.
        let before = AppConfig::default();
        let ui = ui_settings_from_config(&before);
        let mut after = before.clone();
        apply_changes(&mut after, &ui, &ui);
        assert_eq!(before, after);
    }

    /// One case per plain field: change it in the window state, and the same change in the
    /// config is the only difference.
    macro_rules! cases {
        ($( $field:ident => $($seg:ident).+ : $value:expr ),+ $(,)?) => {{
            let mut covered: Vec<&str> = Vec::new();
            $(
                {
                    let original = busy_config();
                    let before = ui_settings_from_config(&original);
                    let mut after = before.clone();
                    after.$field = $value;
                    assert_ne!(before.$field, after.$field, "{}: pick a different value", stringify!($field));
                    let mut written = original.clone();
                    apply_changes(&mut written, &before, &after);
                    let mut expected = original.clone();
                    expected.$($seg).+ = $value;
                    expected.normalize();
                    assert_eq!(written, expected, "{}: exactly its key changes", stringify!($field));
                    // And the new state reads back from the written config.
                    assert_eq!(
                        ui_settings_from_config(&written).$field,
                        after.$field,
                        "{}: reads back",
                        stringify!($field)
                    );
                    covered.push(stringify!($field));
                }
            )+
            covered
        }};
    }

    #[test]
    fn changing_one_plain_field_changes_exactly_its_key() {
        let covered = cases! {
            input_device => audio.input_device : "builtin-mic".to_owned(),
            output_device => audio.output_device : String::new(),
            language => ui.language : "system".to_owned(),
            auto_play => audio.auto_play_reply : true,
            save_history => privacy.save_history : true,
            show_transcript => ui.show_transcript : true,
            hotkey_enabled => hotkeys.enabled : true,
            mode => app.mode : "assistant".to_owned(),
            compact_mode => ui.compact_mode : false,
            always_on_top => ui.always_on_top : true,
            launch_at_login => app.launch_at_login : false,
            call_self_language => call.self_language : "en".to_owned(),
            call_peer_language => call.peer_language : "zh".to_owned(),
            call_target_app => call.target_app : String::new(),
            call_local_output_mode => call.local_output_mode : "original".to_owned(),
            call_outgoing_capture => call.outgoing_capture : "hands_free".to_owned(),
            call_captions_show_source => call.captions_show_source : true,
            call_captions_size => call.captions_size : "small".to_owned(),
            call_disclosure => call.disclosure : true,
            call_barge_in => call.barge_in : true,
            assistant_language => assistant.language : "zh-CN".to_owned(),
            assistant_voice => assistant.voice : "preset".to_owned(),
            assistant_voice_preset => assistant.voice_preset : String::new(),
            virtual_mic_device => audio.virtual_mic_device : String::new(),
            voice_use_clone => voice.use_clone_outgoing : true,
            voice_incoming_preset => voice.incoming_preset : String::new(),
            voice_fallback_preset => voice.fallback_preset : "other".to_owned(),
            models_preload => models.preload : true,
            models_warmup => models.warmup : true,
            models_endpoint => models.endpoint : "modelscope".to_owned(),
            models_max_concurrent => models.max_concurrent : 1,
            models_dir => models.dir : String::new(),
            models_catalogue_url => models.catalogue_url : "https://example.org/other.json".to_owned(),
            models_catalogue_auto_update => models.catalogue_auto_update : false,
            hotkey_assistant_mode => hotkeys.assistant_mode : "toggle".to_owned(),
            save_transcript => privacy.save_transcript : false,
            log_sensitive => privacy.log_sensitive : false,
            debug_enabled => debug.enabled : false,
            debug_show_panel => debug.show_debug_panel : false,
            debug_log_level => debug.log_level : "trace".to_owned(),
            offline => network_policy.offline : false,
            allow_hosts => network_policy.allow_hosts : vec!["10.0.0.2".to_owned(), "nas.local".to_owned()],
        };
        let mut covered = covered;
        covered.sort_unstable();
        let mut listed: Vec<&str> = PLAIN_FIELDS.to_vec();
        listed.sort_unstable();
        assert_eq!(covered, listed, "every plain field needs a case");
    }

    #[test]
    fn the_theme_is_converted_both_ways_and_changes_only_its_key() {
        assert_eq!(theme_from_config("rf"), "RF");
        assert_eq!(theme_from_config("RF"), "RF");
        assert_eq!(theme_from_config(""), "RF");
        assert_eq!(theme_from_config("Ocean"), "Ocean");
        assert_eq!(theme_to_config("RF"), "rf");
        assert_eq!(theme_to_config("Ocean"), "Ocean");

        let original = busy_config();
        let before = ui_settings_from_config(&original);
        let mut after = before.clone();
        after.theme = "RF".into();
        let mut written = original.clone();
        apply_changes(&mut written, &before, &after);
        let mut expected = original.clone();
        expected.ui.theme = "rf".into();
        assert_eq!(written, expected);
    }

    #[test]
    fn echo_mode_is_converted_both_ways_and_keeps_the_legacy_flag_in_step() {
        for (text, mode, control) in [
            ("auto", EchoMode::Auto, true),
            ("always", EchoMode::Always, true),
            ("off", EchoMode::Off, false),
        ] {
            let original = busy_config();
            let before = ui_settings_from_config(&original);
            let mut after = before.clone();
            after.echo_mode = text.into();
            let mut written = original.clone();
            apply_changes(&mut written, &before, &after);
            assert_eq!(written.audio.echo_mode, mode);
            assert_eq!(
                written.audio.echo_control, control,
                "echo_control follows echo_mode"
            );
            let mut expected = original.clone();
            expected.audio.echo_mode = mode;
            expected.normalize();
            assert_eq!(written, expected, "nothing else changes");
        }
    }

    #[test]
    fn the_dialogue_backend_is_converted_to_its_role_selection() {
        let original = busy_config();
        let before = ui_settings_from_config(&original);
        assert_eq!(before.dialogue_provider, "cloud-chat");
        let mut after = before.clone();
        after.dialogue_provider = "openprx".into();
        let mut written = original.clone();
        apply_changes(&mut written, &before, &after);
        let mut expected = original.clone();
        expected
            .providers
            .active
            .dialogue
            .insert(String::new(), "openprx".into());
        assert_eq!(written, expected, "only the dialogue selection changes");
        assert_eq!(ui_settings_from_config(&written).dialogue_provider, "openprx");

        // The empty choice is the local server: no selection is stored.
        let mut local = before.clone();
        local.dialogue_provider = String::new();
        let mut cleared = original.clone();
        apply_changes(&mut cleared, &before, &local);
        assert!(cleared.providers.active.dialogue.is_empty());
        assert_eq!(ui_settings_from_config(&cleared).dialogue_provider, "");

        // A failed save puts the old value back.
        let mut current = after.clone();
        revert_changes(&mut current, &before, &after);
        assert_eq!(current.dialogue_provider, "cloud-chat");
    }

    #[test]
    fn read_only_fields_are_never_written() {
        let original = busy_config();
        let before = ui_settings_from_config(&original);
        let mut after = before.clone();
        after.sample_rate = 8_000;
        after.channels = 2;
        after.voice_profile_id = "someone-else".into();
        after.providers_preset = "cloud".into();
        after.hotkey_session_toggle = "X".into();
        after.hotkey_push_to_talk = "Y".into();
        after.hotkey_interrupt = "Z".into();
        let mut written = original.clone();
        apply_changes(&mut written, &before, &after);
        assert_eq!(written, original);
    }

    #[test]
    fn only_the_changed_field_is_written_so_a_hand_edit_of_another_key_survives() {
        let mut on_disk = busy_config();
        // The window opened with these values ...
        let before = ui_settings_from_config(&on_disk);
        // ... the file was then edited by hand ...
        on_disk.assistant.end_silence_ms = 1_234;
        // ... and the user changed a different setting.
        let mut after = before.clone();
        after.auto_play = true;
        apply_changes(&mut on_disk, &before, &after);
        assert_eq!(on_disk.assistant.end_silence_ms, 1_234);
        assert!(on_disk.audio.auto_play_reply);
    }

    #[test]
    fn a_failed_save_puts_back_only_the_fields_it_changed() {
        let before = ui_settings_from_config(&busy_config());
        let mut after = before.clone();
        after.auto_play = true;
        after.theme = "RF".into();
        // The user has meanwhile changed the language too.
        let mut current = after.clone();
        current.language = "en".into();
        revert_changes(&mut current, &before, &after);
        assert_eq!(current.auto_play, before.auto_play);
        assert_eq!(current.theme, before.theme);
        assert_eq!(current.language, "en", "later edits stay");
        // A field edited again after the failed save is not reverted.
        let mut current = after.clone();
        current.auto_play = false;
        current.auto_play = !before.auto_play;
        revert_changes(&mut current, &before, &after);
        assert_eq!(current.auto_play, before.auto_play);
    }

    #[test]
    fn snapshot_equals_the_state_it_was_taken_from() {
        let ui = ui_settings_from_config(&busy_config());
        assert_eq!(snapshot(&ui), ui);
        let mut other = ui.clone();
        other.offline = !other.offline;
        assert_ne!(snapshot(&other), ui);
    }

    #[test]
    fn the_interface_language_resolves_against_the_system() {
        use prxvc_i18n::Locale::{En, ZhCn};
        assert_eq!(resolve_locale("en", Some("zh-CN")), En);
        assert_eq!(resolve_locale("zh-CN", Some("en-US")), ZhCn);
        assert_eq!(resolve_locale("system", Some("zh-Hans-CN")), ZhCn);
        assert_eq!(resolve_locale("system", Some("zh_TW")), ZhCn);
        assert_eq!(resolve_locale("system", Some("de-DE")), En);
        assert_eq!(resolve_locale("system", None), En);
        assert_eq!(resolve_locale("klingon", Some("zh-CN")), En);
    }

    #[test]
    fn the_window_memory_round_trips_and_ignores_garbage() {
        let geometry = Geometry {
            pos: egui::pos2(10.0, -20.0),
            size: egui::vec2(640.0, 700.0),
        };
        let monitor = egui::vec2(1440.0, 900.0);
        let stored = window_to_config(Some(geometry), Some(monitor), SettingsSection::Permissions);
        assert_eq!(stored.page.as_deref(), Some("permissions"));
        let read = window_from_config(&stored);
        assert_eq!(read.geometry, Some((geometry, Some(monitor))));
        assert_eq!(read.section, Some(SettingsSection::Permissions));

        // Never opened.
        assert_eq!(
            window_from_config(&SettingsWindowConfig::default()),
            SavedWindow::default()
        );
        // A page without a geometry still restores the page.
        let page_only = window_to_config(None, None, SettingsSection::Audio);
        let read = window_from_config(&page_only);
        assert_eq!(read.geometry, None);
        assert_eq!(read.section, Some(SettingsSection::Audio));
        // Garbage is ignored.
        let bad = SettingsWindowConfig {
            x: Some(f32::NAN),
            y: Some(0.0),
            width: Some(100.0),
            height: Some(100.0),
            monitor_width: Some(-1.0),
            monitor_height: Some(900.0),
            page: Some("nope".into()),
        };
        assert_eq!(window_from_config(&bad), SavedWindow::default());
        let no_area = SettingsWindowConfig {
            width: Some(0.0),
            ..stored.clone()
        };
        assert_eq!(window_from_config(&no_area).geometry, None);
        // A bad monitor keeps the geometry but forgets the monitor.
        let bad_monitor = SettingsWindowConfig {
            monitor_width: Some(0.0),
            ..stored
        };
        assert_eq!(
            window_from_config(&bad_monitor).geometry,
            Some((geometry, None))
        );
    }

    #[test]
    fn the_window_memory_survives_a_config_round_trip_through_the_file() {
        let geometry = Geometry {
            pos: egui::pos2(5.0, 6.0),
            size: egui::vec2(600.0, 640.0),
        };
        let mut config = AppConfig::default();
        config.ui.settings_window = window_to_config(Some(geometry), None, SettingsSection::About);
        let dir = tempfile::tempdir().expect("temporary directory");
        let manager = prxvc_config::AppConfigManager::with_dir(dir.path().to_path_buf());
        manager.save(&config).expect("saves");
        let back = manager.load().expect("loads");
        assert_eq!(
            window_from_config(&back.ui.settings_window).geometry,
            Some((geometry, None))
        );
        assert_eq!(back.ui.settings_window.page.as_deref(), Some("about"));
    }

    /// Keys the window shows without writing them (their value comes from elsewhere), or writes
    /// through a converted field.
    const CONVERTED_OR_READ_ONLY: &[&str] = &[
        "ui.theme.",
        "audio.echo_mode.",
        "audio.echo_control.",
        "audio.sample_rate.",
        "audio.channels.",
        "voice.self_profile_id.",
        "providers.preset.",
        "hotkeys.session_toggle.",
        "hotkeys.push_to_talk.",
        "hotkeys.interrupt.",
    ];

    /// Keys of the schema that the settings window does not offer, each with its reason. Anything
    /// not named here and not mapped fails `every_config_key_has_a_decision`.
    const NOT_IN_THE_WINDOW: &[(&str, &str)] = &[
        ("schema_version.", "file format version"),
        ("ui.sounds.", "no sounds exist; not in the design's rows"),
        ("ui.settings_window", "the window's own memory (set by the window, not a row)"),
        ("app.first_run_", "first-run flow state, a later package"),
        ("audio.half_duplex_tail_ms.", "endpointing tuning; not in the design's rows"),
        ("call.start_mode.", "call behaviour beyond the design's rows"),
        ("call.outgoing_when_paused.", "call behaviour beyond the design's rows"),
        ("call.incoming_when_paused.", "call behaviour beyond the design's rows"),
        ("call.outgoing_on_failure.", "call behaviour beyond the design's rows"),
        ("call.incoming_on_failure.", "call behaviour beyond the design's rows"),
        ("call.original_gain_db.", "call behaviour beyond the design's rows"),
        ("call.barge_in_max_lag_ms.", "call behaviour beyond the design's rows"),
        ("call.translate.", "set by the call start command"),
        ("call.disclosure_text_", "the notice's wording: edited in the config file; the built-in text is the default"),
        ("call.output_gain_db.", "call behaviour beyond the design's rows"),
        ("call.stop_grace_ms.", "how long Stop lets the last sentences play; config file only"),
        ("debug.call_trace.", "diagnostic switch for a latency trace; set in the config file or by PRXVC_CALL_TRACE"),
        ("debug.call_trace_text.", "diagnostic switch that adds spoken text to the trace; config file only, off by default"),
        ("pipeline.", "pipeline tuning, shown under Advanced in a later package"),
        ("assistant.end_silence_ms.", "turn-taking tuning; not in the design's rows"),
        ("assistant.history_max_turns.", "conversation bound; not in the design's rows"),
        ("assistant.history_max_tokens.", "conversation bound; not in the design's rows"),
        ("providers.egress_acknowledged", "set by the egress dialog (provider editor package)"),
        ("providers.active", "role choices belong to the provider editor package"),
        ("providers.fallback", "role choices belong to the provider editor package"),
        ("providers.entries", "provider entries belong to the provider editor package"),
        ("models.skipped_updates", "set by the model update dialog (model manager package)"),
    ];

    /// Every leaf of a JSON value as a dotted path with a trailing dot; objects with entries
    /// are descended into, anything else (a scalar, an empty object, an array) is a leaf.
    fn leaves(value: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) if !map.is_empty() => {
                for (key, v) in map {
                    leaves(v, &format!("{path}{key}."), out);
                }
            }
            _ => out.push(path.to_owned()),
        }
    }

    #[test]
    fn every_config_key_has_a_decision() {
        let json = serde_json::to_value(AppConfig::default()).expect("serialises");
        let mut keys = Vec::new();
        leaves(&json, "", &mut keys);
        assert!(keys.len() > 80, "the schema was not walked: {keys:?}");
        let mut undecided = Vec::new();
        for key in &keys {
            let mapped = PLAIN_KEYS.contains(&key.as_str());
            let converted = CONVERTED_OR_READ_ONLY.contains(&key.as_str());
            let excluded = NOT_IN_THE_WINDOW.iter().any(|(prefix, _)| key.starts_with(prefix));
            if !(mapped || converted || excluded) {
                undecided.push(key.clone());
            }
        }
        assert!(
            undecided.is_empty(),
            "these config keys have no row and no reason to have none: {undecided:?}"
        );
        // And the lists name keys that exist.
        for key in PLAIN_KEYS.iter().chain(CONVERTED_OR_READ_ONLY) {
            assert!(keys.contains(&(*key).to_owned()), "{key} is not a key of the schema");
        }
    }
}
