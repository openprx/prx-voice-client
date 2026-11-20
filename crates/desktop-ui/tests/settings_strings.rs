//! Every string the settings window uses exists in both languages, and the Chinese one is
//! Chinese: keys are collected from the source files that draw the window, plus the keys built
//! from a code (sections, driver errors, pipeline problems, statuses).

mod common;

use prxvc_desktop_ui::SettingsSection;
use prxvc_i18n::{Locale, t};

/// Keys whose Chinese text is legitimately Latin only (names, product names).
const LATIN_ONLY: &[&str] = &[
    "settings.call.lang.en",
    "settings.language.en",
    "settings.models.source.hf",
    "settings.models.source.ms",
    "settings.about.version.value",
];

/// The `settings.*`, `tray.*` and `common.*` string literals of a source text that are keys.
fn keys_in(source: &str) -> Vec<String> {
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            (s.starts_with("settings.") || s.starts_with("tray.") || s.starts_with("common."))
                && !s.ends_with('.')
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
        })
        .map(str::to_owned)
        .collect()
}

fn source_keys() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut keys = Vec::new();
    for entry in std::fs::read_dir(dir).expect("src") {
        let path = entry.expect("entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        if name.starts_with("settings_") || name == "context_menu.rs" {
            keys.extend(keys_in(&std::fs::read_to_string(&path).expect("read")));
        }
    }
    keys
}

fn built_keys() -> Vec<String> {
    let mut keys: Vec<String> = SettingsSection::ALL
        .iter()
        .map(|s| format!("settings.nav.{}", s.key()))
        .collect();
    for code in [
        "bundle_not_found",
        "source_rejected",
        "cancelled",
        "authorization_failed",
        "copy_failed",
        "restart_failed",
        "device_did_not_appear",
        "device_still_present",
        "unsupported",
        "other",
    ] {
        keys.push(format!("settings.driver.error.{code}"));
    }
    for code in [
        "invalid_language",
        "language_not_set",
        "invalid_setting",
        "no_provider",
        "unknown_provider",
        "unknown_protocol",
        "unknown_template",
        "wrong_kind",
        "model_not_installed",
        "model_installing",
        "model_failed_verification",
        "model_failed",
        "model_files_incomplete",
        "runtime_not_built_in",
        "not_streaming",
        "language_unsupported",
        "pair_unsupported",
        "credential_missing",
        "credential_unreadable",
        "endpoint_blocked",
        "invalid_endpoint",
        "invalid_params",
        "voice_profile_not_set",
        "voice_profile_missing",
        "voice_profile_unreadable",
        "voice_not_prepared",
        "voice_prepared_for_other_model",
        "voice_preset_unknown",
        "voice_preset_file_missing",
        "voice_preset_unreadable",
        "voice_preset_language",
        "fallback_is_primary",
        "store_unreadable",
        "other",
    ] {
        keys.push(format!("settings.pipeline.problem.{code}"));
    }
    for preset in ["local", "hybrid", "cloud", "custom"] {
        keys.push(format!("settings.pipeline.preset.{preset}"));
    }
    for lang in ["zh", "en"] {
        keys.push(format!("settings.lang.{lang}"));
    }
    keys.push("settings.status.installed.versioned".to_owned());
    keys.extend(["tray.show", "tray.quit"].map(str::to_owned));
    keys
}

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

#[test]
fn the_files_that_draw_the_window_were_scanned() {
    let keys = source_keys();
    assert!(keys.len() > 250, "only {} keys found", keys.len());
    assert!(keys.iter().any(|k| k == "settings.unavailable"));
}

#[test]
fn every_key_is_translated_into_english_and_chinese() {
    let _guard = common::lock_locale();
    let mut keys = source_keys();
    keys.extend(built_keys());
    keys.sort();
    keys.dedup();
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale.clone());
        for key in &keys {
            let text = t(key);
            assert_ne!(&text, key, "{locale:?}: {key} is missing");
            assert!(!text.trim().is_empty(), "{locale:?}: {key} is empty");
            if locale == Locale::ZhCn && !LATIN_ONLY.contains(&key.as_str()) {
                assert!(
                    has_cjk(&text),
                    "{key} is not translated into Chinese: {text:?}"
                );
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn the_two_languages_use_the_same_placeholders() {
    let _guard = common::lock_locale();
    let mut keys = source_keys();
    keys.extend(built_keys());
    keys.sort();
    keys.dedup();
    let placeholders = |text: &str| {
        (0..6)
            .filter(|i| text.contains(&format!("{{{i}}}")))
            .collect::<Vec<_>>()
    };
    for key in &keys {
        prxvc_i18n::set_locale(Locale::En);
        let en = placeholders(&t(key));
        prxvc_i18n::set_locale(Locale::ZhCn);
        let zh = placeholders(&t(key));
        assert_eq!(en, zh, "{key}");
    }
    prxvc_i18n::set_locale(Locale::En);
}
