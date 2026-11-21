//! Tests of the call-translation keys (added with schema version 2): defaults, loading
//! files of every earlier version, and the round trip.

use super::*;
use crate::test_util::TempDir;
use std::fs;

/// A file as written by schema version 1 (before the call-translation keys).
const V1_FILE: &str = r#"schema_version = 1

[audio]
echo_mode = "always"
input_device = "usb-mic"

[call]
self_language = "zh"
peer_language = "en"
target_app = "net.whatsapp.WhatsApp"
local_output_mode = "both"
barge_in = false

[providers.active.asr]
zh = "qwen3-asr-0.6b-q8"
"#;

fn mgr(dir: &TempDir) -> ConfigManager {
    ConfigManager::new(dir.path().to_path_buf())
}

fn write_config(dir: &TempDir, text: &str) {
    fs::write(dir.path().join("config.toml"), text).unwrap();
}

#[test]
fn schema_version_is_three() {
    assert_eq!(CURRENT_SCHEMA_VERSION, 3);
    assert_eq!(AppConfig::default().schema_version, 3);
}

#[test]
fn new_call_keys_default_to_the_design_values() {
    let c = AppConfig::default().call;
    assert_eq!(c.start_mode, "manual");
    assert_eq!(c.outgoing_when_paused, "original");
    assert_eq!(c.incoming_when_paused, "original");
    assert_eq!(
        c.outgoing_on_failure, "original",
        "decision D-P3: the remote never faces a dead microphone"
    );
    assert_eq!(c.incoming_on_failure, "original");
    assert_eq!(c.original_gain_db, -12.0);
    assert_eq!(c.barge_in_max_lag_ms, 3_000);
}

#[test]
fn pipeline_section_defaults_to_the_design_values() {
    let p = AppConfig::default().pipeline;
    assert_eq!(p.recognition_mode, "two_pass");
    assert_eq!((p.clause_pause_ms, p.segment_end_pause_ms), (300, 600));
    assert_eq!((p.long_segment_after_s, p.long_segment_pause_ms), (8, 300));
    assert_eq!((p.max_clause_s, p.max_segment_s), (8, 25));
    assert_eq!(p.vad_threshold, 0.4);
    assert_eq!((p.min_speech_ms, p.pad_ms), (160, 200));
    assert_eq!(p.early_commit, "chunk");
    assert_eq!((p.early_chunk_chars, p.early_chunk_words), (8, 6));
    assert_eq!(p.early_max_wait_ms, 2_500);
    assert!(!p.split_target_clauses && !p.speculative_mt);
    assert_eq!(p.mt_context_clauses, 4);
    assert_eq!((p.jitter_buffer_ms, p.max_backlog_ms), (150, 15_000));
    assert!(p.backlog_skipping);
    assert_eq!(p.max_lag_ms, 20_000);
    assert!(p.catch_up && p.trim_silence);
    assert_eq!((p.catch_up_after_ms, p.silence_keep_ms), (2_000, 60));
    assert_eq!(p.catch_up_max_speed, 1.25);
    assert!(p.adaptive_preroll);
    assert_eq!(p.preroll_max_ms, 1_500);
    // The recogniser's cadence and stability keys default to the adapter's own values.
    assert_eq!(
        (p.asr_redecode_interval_ms, p.asr_first_decode_after_ms),
        (400, 480)
    );
    assert_eq!((p.asr_stable_agreement, p.asr_stable_tail_guard), (4, 6));
    // Filler and quiet-sound gates of the first translated call.
    assert!(p.drop_filler);
    assert_eq!(p.min_speech_dbfs, -75.0);
    // The language rules default on.
    assert!(p.skip_source_in_target && p.retry_wrong_language);
}

#[test]
fn the_spoken_notice_is_on_by_default_in_every_file_and_its_texts_are_empty() {
    let c = AppConfig::default();
    assert!(c.call.disclosure);
    assert!(c.call.disclosure_text_en.is_empty() && c.call.disclosure_text_zh.is_empty());
    let old: AppConfig = toml::from_str("[call]\nself_language = \"zh\"\n").unwrap();
    assert!(old.call.disclosure, "a file written before the key existed gets it on");
    let off: AppConfig = toml::from_str("[call]\ndisclosure = false\n").unwrap();
    assert!(!off.call.disclosure);
}

#[test]
fn the_language_rules_default_on_in_an_old_file_and_can_be_switched_off() {
    let c: AppConfig = toml::from_str("[pipeline]\nmax_segment_s = 20\n").unwrap();
    assert!(c.pipeline.skip_source_in_target && c.pipeline.retry_wrong_language);
    let c: AppConfig = toml::from_str(
        "[pipeline]\nskip_source_in_target = false\nretry_wrong_language = false\n",
    )
    .unwrap();
    assert!(!c.pipeline.skip_source_in_target && !c.pipeline.retry_wrong_language);
}

#[test]
fn a_file_without_the_filler_keys_gets_the_gates_on_and_can_switch_them_off() {
    let c: AppConfig = toml::from_str("[pipeline]\nmax_segment_s = 20\n").unwrap();
    assert!(c.pipeline.drop_filler);
    assert_eq!(c.pipeline.min_speech_dbfs, -75.0);
    let c: AppConfig =
        toml::from_str("[pipeline]\ndrop_filler = false\nmin_speech_dbfs = -100.0\n").unwrap();
    assert!(!c.pipeline.drop_filler);
    assert_eq!(c.pipeline.min_speech_dbfs, -100.0);
}

#[test]
fn a_file_without_the_recogniser_keys_gets_their_defaults_and_keeps_its_own_values() {
    let c: AppConfig =
        toml::from_str("[pipeline]\nmax_segment_s = 20\nasr_redecode_interval_ms = 500\n").unwrap();
    assert_eq!(c.pipeline.max_segment_s, 20);
    assert_eq!(c.pipeline.asr_redecode_interval_ms, 500);
    assert_eq!(c.pipeline.asr_first_decode_after_ms, 480);
    assert_eq!(c.pipeline.asr_stable_agreement, 4);
    assert_eq!(c.pipeline.asr_stable_tail_guard, 6);
}

#[test]
fn asr_refine_is_a_role_map_keyed_by_language_and_empty_by_default() {
    let c = AppConfig::default();
    assert!(c.providers.active.asr_refine.is_empty());
    assert!(c.providers.fallback.asr_refine.is_empty());
    let parsed: AppConfig =
        toml::from_str("[providers.active.asr_refine]\nzh = \"qwen3-asr-1.7b\"\n").unwrap();
    assert_eq!(
        parsed
            .providers
            .active
            .asr_refine
            .get("zh")
            .map(String::as_str),
        Some("qwen3-asr-1.7b")
    );
}

#[test]
fn a_version_1_file_keeps_every_value_and_gains_the_new_keys() {
    let dir = TempDir::new("ct_v1");
    write_config(&dir, V1_FILE);
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(
        report.notices,
        vec![LoadNotice::Migrated { from: 1, to: 3 }]
    );
    let c = report.config;
    assert_eq!(c.schema_version, 3);

    assert_eq!(c.audio.echo_mode, EchoMode::Always);
    assert_eq!(c.audio.input_device, "usb-mic");
    assert_eq!(c.call.self_language, "zh");
    assert_eq!(c.call.target_app, "net.whatsapp.WhatsApp");
    assert_eq!(c.call.local_output_mode, "both");
    assert!(!c.call.barge_in, "an explicit old value is not overwritten");
    assert_eq!(
        c.providers.active.asr.get("zh").map(String::as_str),
        Some("qwen3-asr-0.6b-q8")
    );

    assert_eq!(c.call.start_mode, "manual");
    assert_eq!(c.call.outgoing_on_failure, "original");
    assert_eq!(c.call.original_gain_db, -12.0);
    assert_eq!(c.call.barge_in_max_lag_ms, 3_000);
    assert_eq!(c.pipeline, PipelineConfig::default());
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        V1_FILE,
        "loading does not write"
    );
}

#[test]
fn values_a_user_set_under_the_current_version_survive_a_load_and_save() {
    let dir = TempDir::new("ct_v2_values");
    write_config(
        &dir,
        "schema_version = 3\n[call]\nstart_mode = \"on_call\"\noutgoing_on_failure = \"silence\"\n\
         original_gain_db = -6.5\n[pipeline]\nclause_pause_ms = 450\nrecognition_mode = \"streaming\"\n",
    );
    let m = mgr(&dir);
    let report = m.load_report().unwrap();
    assert!(
        report.notices.is_empty(),
        "a current file needs no migration"
    );
    let c = report.config;
    assert_eq!(c.call.start_mode, "on_call");
    assert_eq!(c.call.outgoing_on_failure, "silence");
    assert_eq!(c.call.original_gain_db, -6.5);
    assert_eq!(c.pipeline.clause_pause_ms, 450);
    assert_eq!(c.pipeline.recognition_mode, "streaming");
    assert_eq!(
        c.pipeline.segment_end_pause_ms, 600,
        "keys left out take their defaults"
    );
    m.save(&c).unwrap();
    assert_eq!(m.load().unwrap(), c);
}

#[test]
fn the_whole_chain_takes_an_unversioned_file_to_the_current_version() {
    let dir = TempDir::new("ct_chain");
    write_config(
        &dir,
        "[audio]\necho_control = false\n[call]\nbarge_in = false\n",
    );
    let report = mgr(&dir).load_report().unwrap();
    assert_eq!(
        report.notices,
        vec![LoadNotice::Migrated { from: 0, to: 3 }]
    );
    let c = report.config;
    assert_eq!(c.audio.echo_mode, EchoMode::Off, "step of version 0 -> 1");
    assert!(c.privacy.save_transcript, "step of version 0 -> 1");
    assert!(!c.call.barge_in);
    assert_eq!(c.call.start_mode, "manual", "step of version 1 -> 2");
    assert_eq!(c.assistant, AssistantConfig::default(), "step of version 2 -> 3 adds nothing");
}

#[test]
fn round_trip_with_the_new_keys_filled_is_stable() {
    let dir = TempDir::new("ct_roundtrip");
    let m = mgr(&dir);
    let mut c = AppConfig::default();
    c.call.start_mode = "on_call".into();
    c.call.outgoing_when_paused = "silence".into();
    c.call.incoming_when_paused = "silence".into();
    c.call.outgoing_on_failure = "silence".into();
    c.call.incoming_on_failure = "silence".into();
    c.call.original_gain_db = -18.0;
    c.call.barge_in_max_lag_ms = 1_500;
    c.pipeline.recognition_mode = "streaming".into();
    c.pipeline.vad_threshold = 0.55;
    c.pipeline.early_commit = "punctuation".into();
    c.pipeline.max_backlog_ms = 9_000;
    c.providers
        .active
        .asr_refine
        .insert("zh".into(), "qwen3-asr-1.7b".into());
    c.providers
        .fallback
        .asr_refine
        .insert("en".into(), "none".into());
    m.save(&c).unwrap();
    assert_eq!(m.load().unwrap(), c);
    let first = fs::read_to_string(dir.path().join("config.toml")).unwrap();
    m.save(&m.load().unwrap()).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        first
    );
}
