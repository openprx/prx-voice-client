//! Each problem type from its own fixture: exactly its typed problem, nothing else.

use prxvc_media::Lang;
use prxvc_models::{FailReason, InstallState, ModelId, NotReady};
use prxvc_ports::ProviderId;

use super::fixtures::*;
use crate::keys::{CallRole, RoleKey};
use crate::plan::RecognitionMode;
use crate::problem::{Problem, ProblemKind, Slot};
use crate::resolve::VoiceChoice;

/// The single problem of a result, asserting there is exactly one.
fn only<T: std::fmt::Debug>(result: Result<T, Vec<Problem>>) -> Problem {
    let mut problems = result.expect_err("expected a problem");
    assert_eq!(problems.len(), 1, "exactly one problem: {problems:?}");
    problems.remove(0)
}

fn pid(id: &str) -> ProviderId {
    ProviderId::new(id)
}

#[test]
fn an_unknown_id_is_a_typed_error_for_every_role() {
    let mut rig = Rig::new();
    rig.cfg.providers.active.vad.insert(String::new(), "nope".into());
    rig.cfg.providers.active.asr.insert("zh".into(), "nope".into());
    rig.cfg.providers.active.asr_refine.insert("zh".into(), "nope".into());
    rig.cfg.providers.active.mt.insert("zh>en".into(), "nope".into());
    rig.cfg.providers.active.tts.insert("en".into(), "nope".into());
    let r = rig.registry();
    let unknown = ProblemKind::UnknownProvider { id: "nope".into() };
    assert_eq!(only(r.resolve_vad()).kind, unknown);
    assert_eq!(only(r.resolve_asr(Lang::Zh)).kind, unknown);
    assert_eq!(only(r.resolve_asr_refine(Lang::Zh)).kind, unknown);
    assert_eq!(only(r.resolve_mt(Lang::Zh, Lang::En)).kind, unknown);
    let p = only(r.resolve_tts(Lang::En, &VoiceChoice::OwnClone));
    assert_eq!((p.kind, p.role, p.slot), (unknown, Some(RoleKey::Tts(Lang::En)), Slot::Primary));
}

#[test]
fn a_provider_of_the_wrong_kind_is_refused_for_the_role() {
    let mut rig = Rig::new();
    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg.providers.active.mt.insert("zh>en".into(), TTS.into());
    rig.cfg.providers.active.tts.insert("en".into(), "openai-mt".into());
    rig.cfg.providers.active.asr.insert("zh".into(), "silero-vad-6.2".into());
    let r = rig.registry();
    let p = only(r.resolve_mt(Lang::Zh, Lang::En));
    assert!(matches!(p.kind, ProblemKind::KindNotValidForRole { role: CallRole::Mt, ref found, .. } if found.contains("Tts")));
    let p = only(r.resolve_tts(Lang::En, &VoiceChoice::OwnClone));
    assert!(matches!(p.kind, ProblemKind::KindNotValidForRole { role: CallRole::Tts, .. }));
    let p = only(r.resolve_asr(Lang::Zh));
    assert!(matches!(p.kind, ProblemKind::KindNotValidForRole { role: CallRole::Asr, .. }));
}

#[test]
fn a_cloud_protocol_without_a_client_is_not_built_in() {
    let mut rig = Rig::new();
    rig.cfg.providers.entries.push(toml_entry(
        "id = \"dg\"\ntemplate = \"deepgram\"\nprotocol = \"deepgram-listen\"\nbase_url = \"wss://api.deepgram.com/v1/listen\"\n",
    ));
    rig.cfg.providers.active.asr.insert("zh".into(), "dg".into());
    let p = only(rig.registry().resolve_asr(Lang::Zh));
    assert_eq!(p.kind, ProblemKind::RuntimeNotBuiltIn { id: pid("dg"), runtime: "deepgram-listen".into() });
}

#[test]
fn a_model_that_is_not_installed_names_the_model_for_the_download() {
    let rig = Rig::new();
    let id = ModelId::new("qwen3-asr-0.6b");
    rig.models.set("qwen3-asr-0.6b", NotReady::NotInstalled { id: id.clone(), size: 1234 });
    let p = only(rig.registry().resolve_asr(Lang::Zh));
    assert_eq!(p.kind, ProblemKind::ModelNotInstalled { model: id, size_bytes: 1234 });

    let id = ModelId::new(TTS);
    rig.models.set(TTS, NotReady::NotInstalled { id: id.clone(), size: 99 });
    let p = only(rig.registry().resolve_tts(Lang::En, &VoiceChoice::Preset("preset-en-1".into())));
    assert_eq!(p.kind, ProblemKind::ModelNotInstalled { model: id, size_bytes: 99 });

    let id = ModelId::new("silero-vad-6.2");
    rig.models.set(
        "silero-vad-6.2",
        NotReady::InProgress { id: id.clone(), state: InstallState::Queued },
    );
    assert_eq!(only(rig.registry().resolve_vad()).kind, ProblemKind::ModelInstalling { model: id });
}

#[test]
fn a_model_that_failed_verification_is_not_reported_as_missing() {
    let rig = Rig::new();
    let id = ModelId::new("qwen3-asr-0.6b");
    rig.models.set(
        "qwen3-asr-0.6b",
        NotReady::Failed { id: id.clone(), reason: FailReason::ChecksumMismatch { file: "asr06.gguf".into() } },
    );
    let p = only(rig.registry().resolve_asr(Lang::Zh));
    assert_eq!(p.kind, ProblemKind::ModelFailedVerification { model: id.clone(), file: Some("asr06.gguf".into()) });
    rig.models.set(
        "qwen3-asr-0.6b",
        NotReady::Failed { id: id.clone(), reason: FailReason::Offline },
    );
    assert!(matches!(only(rig.registry().resolve_asr(Lang::Zh)).kind, ProblemKind::ModelFailed { .. }));
}

#[test]
fn a_runtime_that_is_not_built_in_is_its_own_problem() {
    let mut rig = Rig::new();
    rig.cfg.providers.active.asr.insert("zh".into(), "future-asr".into());
    let p = only(rig.registry().resolve_asr(Lang::Zh));
    assert_eq!(p.kind, ProblemKind::RuntimeNotBuiltIn { id: pid("future-asr"), runtime: "future.cpp".into() });
    rig.tts_loader.unavailable.store(true, std::sync::atomic::Ordering::SeqCst);
    let p = only(rig.registry().resolve_tts(Lang::En, &VoiceChoice::Preset("preset-en-1".into())));
    assert_eq!(p.kind, ProblemKind::RuntimeNotBuiltIn { id: pid(TTS), runtime: "qwentts.cpp".into() });
}

#[test]
fn a_provider_that_cannot_stream_is_not_eligible_and_says_why() {
    let mut rig = Rig::new();
    rig.cfg.providers.active.tts.insert("en".into(), "batch-tts".into());
    // The model has no presets either; the two problems are independent and both are reported.
    let problems = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::Preset("preset-en-1".into()))
        .unwrap_err();
    assert_eq!(problems.len(), 2);
    assert!(problems.iter().any(|p| matches!(&p.kind, ProblemKind::NotStreaming { id, .. } if id.as_str() == "batch-tts")));

    rig.cfg.providers.entries.push(toml_entry(
        "id = \"batch\"\ntemplate = \"batch-mt\"\nprotocol = \"openai-chat\"\nmodel = \"m\"\n",
    ));
    rig.cfg.providers.active.mt.insert("zh>en".into(), "batch".into());
    let p = only(rig.registry().resolve_mt(Lang::Zh, Lang::En));
    assert_eq!(
        p.kind,
        ProblemKind::NotStreaming { id: pid("batch"), reason: "Waits for the whole sentence.".into() }
    );
}

#[test]
fn a_segmented_model_streams_only_in_segmented_mode() {
    let mut rig = Rig::new();
    rig.cfg.providers.active.asr.insert("zh".into(), "qwen3-asr-1.7b".into());
    let p = only(rig.registry().resolve_asr(Lang::Zh));
    assert!(matches!(p.kind, ProblemKind::NotStreaming { .. }));
    rig.cfg.pipeline.recognition_mode = "segmented".into();
    assert!(rig.registry().resolve_asr(Lang::Zh).is_ok());
    assert_eq!(RecognitionMode::Segmented, rig.registry().resolve_call().unwrap().recognition);
}

#[test]
fn an_unsupported_language_or_pair_is_a_typed_problem() {
    let mut rig = Rig::new();
    rig.cfg.providers.active.asr.insert("en".into(), "zh-only-asr".into());
    let p = only(rig.registry().resolve_asr(Lang::En));
    assert_eq!(p.kind, ProblemKind::LanguageUnsupported { id: pid("zh-only-asr"), lang: Lang::En });
    let p = only(rig.registry().resolve_mt(Lang::Zh, Lang::Zh));
    assert_eq!(p.kind, ProblemKind::PairUnsupported { id: pid("local-llama-server"), from: Lang::Zh, to: Lang::Zh });
}

#[test]
fn credentials_are_missing_unreadable_or_read() {
    let mut rig = Rig::new();
    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg.providers.active.mt.insert("zh>en".into(), "openai-mt".into());
    let r = |rig: &Rig| rig.registry().resolve_mt(Lang::Zh, Lang::En);

    rig.env.lock().clear();
    assert_eq!(only(r(&rig)).kind, ProblemKind::CredentialMissing { entry: "openai-mt".into() });

    rig.cfg.providers.entries[0].credential = String::new();
    assert_eq!(only(r(&rig)).kind, ProblemKind::CredentialMissing { entry: "openai-mt".into() });

    rig.cfg.providers.entries[0].credential = "keychain:openai-mt".into();
    assert!(matches!(only(r(&rig)).kind, ProblemKind::CredentialUnreadable { .. }));

    rig.cfg.providers.entries[0].credential = "sk-pasted-into-the-wrong-field".into();
    let p = only(r(&rig));
    assert!(matches!(p.kind, ProblemKind::CredentialUnreadable { .. }));
    assert!(!p.to_string().contains("sk-pasted"), "a malformed reference is not echoed");

    rig.cfg.providers.entries[0].credential = "env:OPENAI_KEY".into();
    rig.env.lock().insert("OPENAI_KEY".into(), "sk-ok".into());
    assert!(r(&rig).unwrap().primary.secret.is_some());
}

#[test]
fn the_offline_policy_blocks_a_remote_endpoint_but_not_the_local_server() {
    let mut rig = Rig::new();
    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg.providers.active.mt.insert("zh>en".into(), "openai-mt".into());
    rig.cfg.network_policy.offline = true;
    let r = rig.registry();
    let p = only(r.resolve_mt(Lang::Zh, Lang::En));
    assert_eq!(p.kind, ProblemKind::EndpointBlocked { entry: "openai-mt".into(), host: "api.openai.com".into() });
    assert!(r.resolve_mt(Lang::En, Lang::Zh).is_ok(), "the default is a loopback server");
}

#[test]
fn invalid_parameters_are_reported_against_the_template_schema() {
    let mut rig = Rig::new();
    rig.cfg.providers.entries.push(toml_entry(
        "id = \"ls\"\ntemplate = \"llama-server\"\nprotocol = \"openai-chat\"\nmodel = \"m\"\n\
         params = { temperature = 5 }\n",
    ));
    rig.cfg.providers.active.mt.insert("zh>en".into(), "ls".into());
    let p = only(rig.registry().resolve_mt(Lang::Zh, Lang::En));
    assert!(matches!(&p.kind, ProblemKind::InvalidParams { param, .. } if param == "temperature"));

    rig.cfg.providers.entries[0].params = "messages = []\nprompt_template = \"bogus\"\nidle_timeout_ms = 3\n"
        .parse()
        .unwrap();
    let mut params: Vec<_> = rig
        .registry()
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap_err()
        .into_iter()
        .filter_map(|p| match p.kind {
            ProblemKind::InvalidParams { param, .. } => Some(param),
            _ => None,
        })
        .collect();
    params.sort();
    assert_eq!(params, ["idle_timeout_ms", "messages", "prompt_template"]);
}

#[test]
fn a_bad_address_or_protocol_or_template_is_typed() {
    let mut rig = Rig::new();
    for (entry, expect) in [
        ("id = \"a\"\nprotocol = \"openai-chat\"\nbase_url = \"ftp://x\"\nmodel = \"m\"\n",
         ProblemKind::InvalidEndpoint { entry: "a".into(), detail: "`ftp://x` is not an http(s) address".into() }),
        ("id = \"a\"\nprotocol = \"carrier-pigeon\"\nbase_url = \"http://x\"\nmodel = \"m\"\n",
         ProblemKind::UnknownProtocol { entry: "a".into(), protocol: "carrier-pigeon".into() }),
        ("id = \"a\"\ntemplate = \"nope\"\nprotocol = \"openai-chat\"\nbase_url = \"http://127.0.0.1:1\"\nmodel = \"m\"\n",
         ProblemKind::UnknownTemplate { entry: "a".into(), template: "nope".into() }),
    ] {
        rig.cfg.providers.entries = vec![toml_entry(entry)];
        rig.cfg.providers.active.mt.insert("zh>en".into(), "a".into());
        assert_eq!(only(rig.registry().resolve_mt(Lang::Zh, Lang::En)).kind, expect);
    }
}

#[test]
fn voice_problems_are_typed() {
    let mut rig = Rig::new();
    let tts = |rig: &Rig, v: VoiceChoice| rig.registry().resolve_tts(Lang::En, &v);

    rig.cfg.voice.self_profile_id.clear();
    assert_eq!(only(tts(&rig, VoiceChoice::OwnClone)).kind, ProblemKind::VoiceProfileNotSet);

    rig.cfg.voice.self_profile_id = "ghost".into();
    let p = only(tts(&rig, VoiceChoice::OwnClone));
    assert!(matches!(p.kind, ProblemKind::VoiceProfileMissing { ref profile_id, .. } if profile_id == "ghost"));
    rig.cfg.voice.self_profile_id = "../etc".into();
    assert!(matches!(only(tts(&rig, VoiceChoice::OwnClone)).kind, ProblemKind::VoiceProfileMissing { .. }));

    rig.write_profile("fresh", false);
    rig.cfg.voice.self_profile_id = "fresh".into();
    assert_eq!(
        only(tts(&rig, VoiceChoice::OwnClone)).kind,
        ProblemKind::VoiceNotPrepared { profile_id: "fresh".into() }
    );

    rig.prepare_profile("fresh", &"c".repeat(64));
    let p = only(tts(&rig, VoiceChoice::OwnClone));
    assert_eq!(
        p.kind,
        ProblemKind::VoicePreparedForOtherModel {
            profile_id: "fresh".into(),
            prepared_for: vec!["c".repeat(12)],
            current: rig.fingerprint().chars().take(12).collect(),
        }
    );

    std::fs::write(rig.dirs.profiles.join("fresh/profile.json"), "{ not json").unwrap();
    assert!(matches!(only(tts(&rig, VoiceChoice::OwnClone)).kind, ProblemKind::VoiceProfileUnreadable { .. }));

    assert_eq!(
        only(tts(&rig, VoiceChoice::Preset("no-such-preset".into()))).kind,
        ProblemKind::VoicePresetUnknown { name: "no-such-preset".into() }
    );
    // A preset exists per language: the Chinese one cannot speak English.
    assert!(matches!(
        only(tts(&rig, VoiceChoice::Preset("preset-zh-1".into()))).kind,
        ProblemKind::VoicePresetUnknown { .. }
    ));
    std::fs::remove_file(rig.dirs.presets.join("preset-en-1/ref.wav")).unwrap();
    assert!(matches!(
        only(tts(&rig, VoiceChoice::Preset("preset-en-1".into()))).kind,
        ProblemKind::VoicePresetFileMissing { ref preset, .. } if preset == "preset-en-1"
    ));
}

#[test]
fn an_unknown_recognition_mode_and_an_empty_role_are_typed() {
    let mut rig = Rig::new();
    rig.cfg.pipeline.recognition_mode = "telepathy".into();
    let problems = rig.registry().resolve_call().unwrap_err();
    assert!(problems.iter().any(|p| p.role.is_none()
        && p.kind == ProblemKind::InvalidSetting { key: "pipeline.recognition_mode".into(), value: "telepathy".into() }));

    // A catalogue without a recognition model leaves the role without a default.
    let mut value = serde_json::to_value(&*rig.catalogue).unwrap();
    value["models"].as_array_mut().unwrap().retain(|m| m["role"] != "vad");
    let cat = prxvc_models::Catalogue::parse(&value.to_string()).unwrap();
    let registry = crate::registry::ProviderRegistry::new(
        &prxvc_config::AppConfig::default(),
        std::sync::Arc::new(cat),
        rig.models.clone() as _,
        std::sync::Arc::new(crate::credentials::EnvCredentials::with_lookup(|_| None)),
        rig.dirs.clone(),
    );
    assert_eq!(only(registry.resolve_vad()).kind, ProblemKind::NoProviderSelected);
}

#[test]
fn three_independent_problems_are_reported_together() {
    let mut rig = Rig::new();
    // 1: the VAD model is not installed.
    let vad = ModelId::new("silero-vad-6.2");
    rig.models.set("silero-vad-6.2", NotReady::NotInstalled { id: vad.clone(), size: 7 });
    // 2: the outgoing translation endpoint is remote and offline mode is on.
    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg.providers.active.mt.insert("zh>en".into(), "openai-mt".into());
    rig.cfg.network_policy.offline = true;
    // 3: the owner's voice profile does not exist.
    rig.cfg.voice.self_profile_id = "ghost".into();

    let problems = rig.registry().resolve_call().unwrap_err();
    assert_eq!(problems.len(), 3, "{problems:#?}");
    assert!(problems.iter().any(|p| p.role == Some(RoleKey::Vad)
        && p.kind == ProblemKind::ModelNotInstalled { model: vad.clone(), size_bytes: 7 }));
    assert!(problems.iter().any(|p| matches!(&p.kind, ProblemKind::EndpointBlocked { host, .. } if host == "api.openai.com")));
    assert!(problems.iter().any(|p| matches!(&p.kind, ProblemKind::VoiceProfileMissing { .. })));
}

#[test]
fn a_failing_primary_reports_its_fallback_problems_too() {
    let mut rig = Rig::new();
    rig.cfg.providers.active.tts.insert("en".into(), "nope".into());
    rig.cfg.providers.fallback.tts.insert("en".into(), "also-nope".into());
    let problems = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::OwnClone)
        .unwrap_err();
    assert_eq!(problems.len(), 2);
    assert_eq!(problems[0].slot, Slot::Primary);
    assert_eq!(problems[1].slot, Slot::Fallback);
}

#[test]
fn a_fallback_equal_to_the_primary_is_refused() {
    let mut rig = Rig::new();
    rig.cfg.providers.fallback.mt.insert("zh>en".into(), "local-llama-server".into());
    let r = rig.registry().resolve_mt(Lang::Zh, Lang::En).unwrap();
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(r.warnings[0].kind, ProblemKind::FallbackIsPrimary { id: pid("local-llama-server") });
    assert!(r.fallback.is_none());
}
