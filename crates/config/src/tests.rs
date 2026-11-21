use super::*;
use crate::test_util::{FakeStore, TempDir};
use std::fs;
use std::sync::Arc;

/// The file as it is on a developer machine today (no schema version, no new
/// sections).
const REAL_WORLD: &str = r#"[audio]
sample_rate = 16000
channels = 1
auto_play_reply = true
echo_control = true

[ui]
language = "zh-CN"
show_transcript = true
compact_mode = false

[network]
connect_timeout_ms = 5000
reconnect_attempts = 3
reconnect_delay_ms = 1000
server_url = "http://localhost:3000"
use_mock = true
language = "zh-CN"
asr_provider = "mock"
agent_provider = "mock"
tts_provider = "mock"

[privacy]
save_history = true
save_transcript = true
log_sensitive = false

[debug]
enabled = false
log_level = "info"
show_debug_panel = false
"#;

fn mgr(dir: &TempDir) -> ConfigManager {
    ConfigManager::new(dir.path().to_path_buf())
}

fn write_config(dir: &TempDir, text: &str) {
    fs::write(dir.path().join("config.toml"), text).unwrap();
}

fn read_config(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("config.toml")).unwrap()
}

fn aside_files(dir: &TempDir, marker: &str) -> Vec<std::path::PathBuf> {
    fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().contains(marker))
        .collect()
}

// --- defaults -------------------------------------------------------------

#[test]
fn defaults_match_the_design() {
    let c = AppConfig::default();
    assert_eq!(c.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(c.audio.sample_rate, 16_000);
    assert_eq!(c.audio.channels, 1);
    assert!(c.audio.auto_play_reply);
    assert!(c.audio.echo_control);
    assert_eq!(c.audio.echo_mode, EchoMode::Auto);
    assert_eq!(c.audio.half_duplex_tail_ms, 300);
    assert_eq!(c.audio.input_device, "");
    assert_eq!(c.audio.output_device, "");
    assert_eq!(c.audio.virtual_mic_device, "");
    assert_eq!(c.ui.language, "en");
    assert_eq!(c.ui.theme, "rf");
    assert!(c.ui.always_on_top);
    assert!(!c.ui.sounds);
    assert_eq!(c.ui.settings_window, SettingsWindowConfig::default());
    assert!(c.privacy.save_history);
    assert!(!c.privacy.save_transcript, "new installs do not save transcripts");
    assert!(!c.privacy.log_sensitive);
    assert_eq!(c.app.mode, "assistant");
    assert!(!c.app.launch_at_login && !c.app.first_run_completed);
    assert_eq!(c.call.local_output_mode, "translation");
    assert_eq!(c.call.outgoing_capture, "hands_free");
    assert!(c.call.captions_show_source && c.call.barge_in);
    assert!(c.voice.use_clone_outgoing);
    assert!(c.hotkeys.enabled);
    assert_eq!(c.hotkeys.session_toggle, "Ctrl+Shift+Space");
    assert_eq!(c.hotkeys.assistant_mode, "toggle");
    assert_eq!(c.models.endpoint, "huggingface");
    assert_eq!(c.models.max_concurrent, 2);
    assert!(c.models.preload && c.models.warmup && !c.models.catalogue_auto_update);
    assert!(c.models.catalogue_url.ends_with("/catalogue.json"));
    assert_eq!(c.providers.preset, "local");
    assert!(c.providers.entries.is_empty());
    assert!(!c.network_policy.offline);
    assert!(c.network_policy.allow_hosts.is_empty());
}

#[test]
fn missing_file_gives_defaults_and_writes_nothing() {
    let dir = TempDir::new("missing");
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(report.config, AppConfig::default());
    assert!(report.notices.is_empty());
    assert!(!dir.path().join("config.toml").exists());
}

#[test]
fn empty_file_loads_with_defaults_but_keeps_old_save_transcript_behaviour() {
    let dir = TempDir::new("empty");
    write_config(&dir, "");
    let c = mgr(&dir).load().unwrap();
    // An existing file predates the new default.
    assert!(c.privacy.save_transcript);
    let mut expected = AppConfig { privacy: PrivacyConfig { save_transcript: true, ..Default::default() }, ..Default::default() };
    // The migration pins the translation engine the owner chose for existing files.
    expected.call.translation_engine = crate::TranslationEngine::migrated_default().as_str().to_owned();
    assert_eq!(c, expected);
}

#[test]
fn partial_sections_load_with_defaults() {
    let dir = TempDir::new("partial");
    write_config(&dir, "[audio]\nsample_rate = 8000\n[ui]\ncompact_mode = true\n[debug]\nenabled = true\n[privacy]\nsave_history = false\n");
    let c = mgr(&dir).load().unwrap();
    assert_eq!(c.audio.sample_rate, 8000);
    assert_eq!(c.audio.channels, 1);
    assert!(c.ui.compact_mode && c.ui.show_transcript);
    assert!(c.debug.enabled);
    assert_eq!(c.debug.log_level, "info");
    assert!(!c.privacy.save_history);
    assert_eq!(c.assistant, AssistantConfig::default());
}

// --- the real-world file --------------------------------------------------

#[test]
fn real_world_file_keeps_every_value_and_migrates() {
    let dir = TempDir::new("real");
    write_config(&dir, REAL_WORLD);
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(report.notices, vec![LoadNotice::Migrated { from: 0, to: CURRENT_SCHEMA_VERSION }]);
    let c = report.config;

    assert_eq!(c.audio.sample_rate, 16_000);
    assert_eq!(c.audio.channels, 1);
    assert!(c.audio.auto_play_reply);
    assert!(c.audio.echo_control);
    assert_eq!(c.audio.echo_mode, EchoMode::Auto, "migrated from echo_control = true");
    assert_eq!(c.assistant.end_silence_ms, DEFAULT_ASSISTANT_END_SILENCE_MS, "the file had no explicit value");

    assert_eq!(c.ui.language, "zh-CN");
    assert!(c.ui.show_transcript);
    assert!(!c.ui.compact_mode);

    assert_eq!(c.assistant.language, "zh-CN", "network.language moved to [assistant]");

    assert!(c.privacy.save_history);
    assert!(c.privacy.save_transcript, "explicit value is unchanged");
    assert!(!c.privacy.log_sensitive);

    assert!(!c.debug.enabled);
    assert_eq!(c.debug.log_level, "info");
    assert!(!c.debug.show_debug_panel);

    assert_eq!(c.schema_version, CURRENT_SCHEMA_VERSION);
    // New sections are defaults.
    assert_eq!(c.hotkeys, HotkeysConfig::default());
    assert_eq!(c.models, ModelsConfig::default());
    // Loading does not write.
    assert_eq!(read_config(&dir), REAL_WORLD);
}

#[test]
fn echo_control_false_in_a_real_file_gives_off() {
    let dir = TempDir::new("echo_off");
    write_config(&dir, &REAL_WORLD.replace("echo_control = true", "echo_control = false"));
    let c = mgr(&dir).load().unwrap();
    assert_eq!(c.audio.echo_mode, EchoMode::Off);
    assert!(!c.audio.echo_control);
}

#[test]
fn echo_control_follows_echo_mode_on_save() {
    let dir = TempDir::new("echo_sync");
    let m = mgr(&dir);
    let c = m.update(|c| c.audio.echo_mode = EchoMode::Off).unwrap();
    assert!(!c.audio.echo_control);
    assert!(read_config(&dir).contains("echo_control = false"));
    assert!(read_config(&dir).contains("echo_mode = \"off\""));
    let c = m.update(|c| c.audio.echo_mode = EchoMode::Always).unwrap();
    assert!(c.audio.echo_control);
    assert!(read_config(&dir).contains("echo_control = true"));
    // Disagreeing keys: echo_mode wins.
    write_config(&dir, "schema_version = 1\n[audio]\necho_control = false\necho_mode = \"always\"\n");
    assert_eq!(m.load().unwrap().audio.echo_mode, EchoMode::Always);
    assert!(m.load().unwrap().audio.echo_control);
    // An unknown mode reads as auto.
    write_config(&dir, "schema_version = 1\n[audio]\necho_mode = \"surprise\"\n");
    assert_eq!(m.load().unwrap().audio.echo_mode, EchoMode::Auto);
}

#[test]
fn round_trip_is_stable() {
    let dir = TempDir::new("roundtrip");
    write_config(&dir, REAL_WORLD);
    let m = mgr(&dir);
    let first = m.load().unwrap();
    m.save(&first).unwrap();
    let second = m.load().unwrap();
    assert_eq!(first, second);
    let text_one = read_config(&dir);
    m.save(&second).unwrap();
    assert_eq!(read_config(&dir), text_one, "saving twice writes the same bytes");
    assert!(m.load_report().unwrap().notices.is_empty(), "a saved file is current");
}

#[test]
fn round_trip_with_every_new_section_filled() {
    let dir = TempDir::new("roundtrip_full");
    let m = mgr(&dir);
    let mut c = AppConfig::default();
    c.ui.settings_window = SettingsWindowConfig {
        x: Some(10.0),
        y: Some(20.5),
        width: Some(640.0),
        height: Some(480.0),
        monitor_width: Some(1920.0),
        monitor_height: Some(1080.0),
        page: Some("audio".into()),
    };
    c.models.skipped_updates = vec!["qwen3-asr-1.7b-q8@abc123".into()];
    c.models.max_concurrent = 3;
    c.providers.active.vad.insert(String::new(), "silero-vad-6.2".into());
    c.providers.active.asr.insert("zh".into(), "qwen3-asr-0.6b-q8".into());
    c.providers.active.mt.insert("zh>en".into(), "hy-mt2-7b-q4km".into());
    c.providers.fallback.mt.insert("zh>en".into(), "none".into());
    let mut params = toml::Table::new();
    params.insert("temperature".into(), toml::Value::Float(0.2));
    c.providers.entries.push(ProviderEntry {
        id: "openai-mt".into(),
        template: "openai-chat".into(),
        display_name: "OpenAI".into(),
        protocol: "openai-chat".into(),
        base_url: "https://api.openai.com/v1".into(),
        model: "gpt-4.1-mini".into(),
        credential: "keychain:openai-mt".into(),
        local_endpoint: false,
        params,
        ..Default::default()
    });
    c.network_policy.offline = true;
    c.network_policy.allow_hosts = vec!["192.168.1.20".into()];
    m.save(&c).unwrap();
    assert_eq!(m.load().unwrap(), c);
}

#[test]
fn max_concurrent_is_clamped() {
    let dir = TempDir::new("clamp");
    write_config(&dir, "schema_version = 1\n[models]\nmax_concurrent = 9\n");
    assert_eq!(mgr(&dir).load().unwrap().models.max_concurrent, 3);
    write_config(&dir, "schema_version = 1\n[models]\nmax_concurrent = 0\n");
    assert_eq!(mgr(&dir).load().unwrap().models.max_concurrent, 1);
}

#[test]
fn unknown_keys_are_tolerated_on_read_and_dropped_on_write() {
    let dir = TempDir::new("unknown");
    write_config(&dir, &format!("{REAL_WORLD}\n[future]\nthing = 1\n"));
    let m = mgr(&dir);
    let c = m.load().unwrap();
    assert_eq!(c.ui.language, "zh-CN");
    m.save(&c).unwrap();
    assert!(!read_config(&dir).contains("future"));
}

// --- new-install default versus existing files -----------------------------

#[test]
fn save_transcript_new_install_versus_existing_file() {
    // New install: no file, defaults, and the first write records `false`.
    let fresh = TempDir::new("st_fresh");
    let m = mgr(&fresh);
    assert!(!m.load().unwrap().privacy.save_transcript);
    m.update(|_| {}).unwrap();
    assert!(read_config(&fresh).contains("save_transcript = false"));
    assert!(!m.load().unwrap().privacy.save_transcript);

    // Existing file without the key: keeps the old behaviour (true), and the
    // first write pins it so that it stays true.
    let old = TempDir::new("st_old");
    write_config(&old, "[privacy]\nsave_history = true\n");
    let m = mgr(&old);
    assert!(m.load().unwrap().privacy.save_transcript);
    m.update(|_| {}).unwrap();
    assert!(read_config(&old).contains("save_transcript = true"));
    assert!(m.load().unwrap().privacy.save_transcript);

    // Existing file with the key set to false: unchanged.
    let off = TempDir::new("st_off");
    write_config(&off, "[privacy]\nsave_transcript = false\n");
    assert!(!mgr(&off).load().unwrap().privacy.save_transcript);
}

// --- unreadable files ------------------------------------------------------

#[test]
fn unparseable_file_is_kept_byte_for_byte_and_defaults_are_used() {
    let dir = TempDir::new("unparse");
    let garbage: &[u8] = b"[audio\nsample_rate = = oops\n\xe2\x82\xac";
    fs::write(dir.path().join("config.toml"), garbage).unwrap();
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(report.config, AppConfig::default());
    let [LoadNotice::UnreadableFileKept { kept_as, .. }] = report.notices.as_slice() else {
        panic!("{:?}", report.notices)
    };
    assert!(kept_as.file_name().unwrap().to_string_lossy().starts_with("config.toml.unreadable-"));
    assert_eq!(fs::read(kept_as).unwrap(), garbage);
    assert!(!dir.path().join("config.toml").exists());
}

#[test]
fn file_with_a_wrong_type_is_kept_aside_too() {
    let dir = TempDir::new("wrongtype");
    let text = "[audio]\nsample_rate = \"fast\"\n";
    write_config(&dir, text);
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(report.config, AppConfig::default());
    let kept = aside_files(&dir, "unreadable");
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read_to_string(&kept[0]).unwrap(), text);
}

#[test]
fn non_utf8_file_is_kept_aside() {
    let dir = TempDir::new("nonutf8");
    fs::write(dir.path().join("config.toml"), [0xff, 0xfe, 0x00]).unwrap();
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(report.notices.len(), 1);
    assert_eq!(fs::read(&aside_files(&dir, "unreadable")[0]).unwrap(), [0xff, 0xfe, 0x00]);
}

#[test]
fn saving_never_overwrites_an_unreadable_file() {
    // A fresh manager that never called load (the desktop app builds one per
    // save) must not destroy the user's file either.
    let dir = TempDir::new("save_unreadable");
    let garbage = b"this is not toml = [";
    fs::write(dir.path().join("config.toml"), garbage).unwrap();
    mgr(&dir).save(&AppConfig::default()).unwrap();
    let kept = aside_files(&dir, "unreadable");
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read(&kept[0]).unwrap(), garbage);
    assert!(mgr(&dir).load().unwrap().privacy.save_history);
}

#[test]
fn update_after_an_unreadable_file_keeps_it_and_writes_a_new_one() {
    let dir = TempDir::new("update_unreadable");
    let garbage = b"=== broken";
    fs::write(dir.path().join("config.toml"), garbage).unwrap();
    let c = mgr(&dir).update(|c| c.ui.compact_mode = true).unwrap();
    assert!(c.ui.compact_mode);
    assert_eq!(fs::read(&aside_files(&dir, "unreadable")[0]).unwrap(), garbage);
    assert!(mgr(&dir).load().unwrap().ui.compact_mode);
}

#[test]
fn a_newer_schema_is_loaded_and_backed_up_before_a_write() {
    let dir = TempDir::new("newer");
    let text = "schema_version = 7\n[ui]\nlanguage = \"zh-CN\"\nfuture_key = 1\n";
    write_config(&dir, text);
    let m = mgr(&dir);
    assert_eq!(m.load().unwrap().ui.language, "zh-CN");
    m.update(|c| c.ui.compact_mode = true).unwrap();
    let copies = aside_files(&dir, ".v7-");
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read_to_string(&copies[0]).unwrap(), text);
    assert!(read_config(&dir).contains(&format!("schema_version = {CURRENT_SCHEMA_VERSION}")));
}

// --- atomic writes ---------------------------------------------------------

#[test]
fn a_stale_temp_file_does_not_corrupt_the_config() {
    let dir = TempDir::new("stale_tmp");
    let m = mgr(&dir);
    m.save(&AppConfig::default()).unwrap();
    let good = read_config(&dir);
    // A write that was interrupted before the rename.
    fs::write(dir.path().join("config.toml.tmp"), "[audio]\nsample_rate = 4").unwrap();
    assert_eq!(read_config(&dir), good);
    assert_eq!(m.load().unwrap(), AppConfig::default());
    assert!(aside_files(&dir, "unreadable").is_empty());
    // The next save overwrites the stale file and leaves none behind.
    m.update(|c| c.ui.compact_mode = true).unwrap();
    assert!(!dir.path().join("config.toml.tmp").exists());
    assert!(m.load().unwrap().ui.compact_mode);
}

#[test]
fn save_creates_the_directory_and_leaves_no_temp_file() {
    let dir = TempDir::new("mkdir");
    let nested = dir.path().join("a").join("b");
    let m = ConfigManager::new(nested.clone());
    m.save(&AppConfig::default()).unwrap();
    assert!(nested.join("config.toml").is_file());
    assert!(!nested.join("config.toml.tmp").exists());
}

#[test]
fn two_updates_on_different_keys_both_persist() {
    let dir = TempDir::new("two_updates");
    write_config(&dir, REAL_WORLD);
    let m = mgr(&dir);
    m.update(|c| c.ui.compact_mode = true).unwrap();
    // A second manager: the way the app builds one per write.
    let c = mgr(&dir).update(|c| c.audio.auto_play_reply = false).unwrap();
    assert!(c.ui.compact_mode && !c.audio.auto_play_reply);
    let loaded = mgr(&dir).load().unwrap();
    assert!(loaded.ui.compact_mode);
    assert!(!loaded.audio.auto_play_reply);
    assert_eq!(loaded.ui.language, "zh-CN");
    assert_eq!(loaded.assistant.language, "zh-CN");
}

#[test]
fn a_migrated_file_loses_the_retired_keys_on_its_next_save() {
    let dir = TempDir::new("retired_keys");
    write_config(&dir, REAL_WORLD);
    let c = mgr(&dir).update(|c| c.ui.compact_mode = true).unwrap();
    assert_eq!(c.assistant.language, "zh-CN");
    let text = read_config(&dir);
    assert!(!text.contains("[network]"), "{text}");
    assert!(!text.contains("use_mock"), "{text}");
    assert!(!text.contains("server_url"), "{text}");
    assert!(text.contains("[assistant]"), "{text}");
    assert!(text.contains(&format!("schema_version = {CURRENT_SCHEMA_VERSION}")), "{text}");
}

// --- secrets ---------------------------------------------------------------

const WITH_KEY: &str = "[[providers.entries]]\nid = \"openai-mt\"\ntemplate = \"openai-chat\"\nbase_url = \"https://api.openai.com/v1\"\napi_key = \"sk-test-123\"\n";

#[test]
fn api_key_is_moved_to_the_store_and_the_file_rewritten() {
    let dir = TempDir::new("secret_moved");
    write_config(&dir, WITH_KEY);
    let store = Arc::new(FakeStore::default());
    let m = mgr(&dir).with_credential_store(store.clone());
    let report = m.load_report().unwrap();
    assert!(report.notices.contains(&LoadNotice::SecretsMoved { entry_ids: vec!["openai-mt".into()] }));
    assert_eq!(
        store.saved.lock().unwrap().as_slice(),
        [("openai-mt".to_owned(), "sk-test-123".to_owned())]
    );
    let entry = &report.config.providers.entries[0];
    assert_eq!(entry.credential, "keychain:openai-mt");
    assert!(entry.pending_api_key.is_none());
    // The file was rewritten immediately and no longer holds the key.
    let text = read_config(&dir);
    assert!(!text.contains("sk-test-123") && !text.contains("api_key"), "{text}");
    assert!(text.contains("keychain:openai-mt"));
    // A second load has nothing left to do.
    let again = m.load_report().unwrap();
    assert!(again.notices.is_empty());
    assert_eq!(again.config, report.config);
    assert_eq!(store.saved.lock().unwrap().len(), 1);
}

#[test]
fn without_a_store_the_key_is_preserved_through_loads_and_saves() {
    let dir = TempDir::new("secret_kept");
    write_config(&dir, WITH_KEY);
    let m = mgr(&dir);
    let report = m.load_report().unwrap();
    assert!(report.notices.iter().any(|n| matches!(n, LoadNotice::SecretsNotMoved { .. })));
    let entry = &report.config.providers.entries[0];
    assert_eq!(entry.pending_api_key.as_ref().unwrap().expose(), "sk-test-123");
    assert!(!format!("{:?}", report.config).contains("sk-test-123"), "Debug output is redacted");
    // The file is untouched by the load.
    assert!(read_config(&dir).contains("sk-test-123"));
    // An unrelated update must not lose the key.
    m.update(|c| c.ui.compact_mode = true).unwrap();
    assert!(read_config(&dir).contains("sk-test-123"));
    // Once a store exists, the next load moves it.
    let store = Arc::new(FakeStore::default());
    let m2 = mgr(&dir).with_credential_store(store.clone());
    let report = m2.load_report().unwrap();
    assert!(report.notices.contains(&LoadNotice::SecretsMoved { entry_ids: vec!["openai-mt".into()] }));
    assert!(!read_config(&dir).contains("sk-test-123"));
    assert_eq!(store.saved.lock().unwrap().len(), 1);
}

#[test]
fn a_stray_secret_without_a_store_gets_a_backup_copy() {
    let dir = TempDir::new("secret_stray");
    let text = "[providers.active]\nsecret = \"s3\"\n";
    write_config(&dir, text);
    let m = mgr(&dir);
    m.load().unwrap();
    m.load().unwrap();
    let copies = aside_files(&dir, ".secrets-");
    assert_eq!(copies.len(), 1, "identical copies are not repeated");
    assert!(fs::read_to_string(&copies[0]).unwrap().contains("s3"));
}

#[test]
fn manager_without_credential_store_has_no_effect_on_files_without_keys() {
    let dir = TempDir::new("nokey");
    write_config(&dir, REAL_WORLD);
    let report = mgr(&dir).load_report().unwrap();
    assert!(!report.notices.iter().any(|n| matches!(n, LoadNotice::SecretsNotMoved { .. } | LoadNotice::SecretsMoved { .. })));
    assert!(aside_files(&dir, ".secrets-").is_empty());
}

// --- AppConfigManager ------------------------------------------------------

#[test]
fn app_config_manager_with_dir_round_trip() {
    let dir = TempDir::new("acm");
    let acm = AppConfigManager::with_dir(dir.path().to_path_buf());
    assert_eq!(acm.config_dir(), dir.path());
    let mut cfg = AppConfig::default();
    cfg.audio.sample_rate = 44_100;
    cfg.ui.language = String::from("ja");
    acm.save(&cfg).unwrap();
    let loaded = acm.load().unwrap();
    assert_eq!(loaded.audio.sample_rate, 44_100);
    assert_eq!(loaded.ui.language, "ja");
    let updated = acm.update(|c| c.debug.enabled = true).unwrap();
    assert!(updated.debug.enabled && acm.load().unwrap().debug.enabled);
}

#[test]
fn app_config_manager_default_dir_uses_the_app_id() {
    // Only resolves the path; nothing is read or written.
    let acm = AppConfigManager::with_default_dir().unwrap();
    assert_eq!(acm.config_dir(), paths::default_config_dir().unwrap());
    assert!(acm.config_dir().to_string_lossy().contains("prx-voice-client"));
}

#[test]
fn old_files_without_the_turn_fields_still_load() {
    let old = "[audio]\nsample_rate = 8000\necho_control = false\n";
    let dir = TempDir::new("turn_fields");
    write_config(&dir, old);
    let c = mgr(&dir).load().unwrap();
    assert_eq!(c.audio.sample_rate, 8000);
    assert!(!c.audio.echo_control);
    assert_eq!(c.assistant, AssistantConfig::default());
}
