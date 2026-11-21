//! The defaults of a fresh configuration, the explicit selections, fallbacks and the disclosure.

use prxvc_media::{Direction, Lang};
use prxvc_models::ModelId;
use prxvc_ports::{ProviderId, VoiceSelection};

use super::fixtures::*;
use crate::keys::RoleKey;
use crate::problem::{Problem, ProblemKind, Slot};
use crate::resolve::{DEFAULT_MT_PROVIDER, VoiceChoice};
use crate::spec::{DataLeaving, Locality, VoicePlan};

fn kinds(problems: &[Problem]) -> Vec<&ProblemKind> {
    problems.iter().map(|p| &p.kind).collect()
}

#[test]
fn a_fresh_config_resolves_to_the_catalogue_defaults() {
    let rig = Rig::new();
    let plan = rig.registry().resolve_call().expect("the defaults resolve");
    assert_eq!(
        (plan.languages.own, plan.languages.peer),
        (Lang::Zh, Lang::En)
    );
    assert_eq!(plan.vad.primary.id.as_str(), "silero-vad-6.2");
    assert!(plan.warnings.is_empty());

    let out = plan.direction(Direction::Outgoing);
    let inc = plan.direction(Direction::Incoming);
    assert_eq!((out.source, out.target), (Lang::Zh, Lang::En));
    assert_eq!((inc.source, inc.target), (Lang::En, Lang::Zh));
    for d in [out, inc] {
        assert_eq!(d.asr.primary.id.as_str(), "qwen3-asr-0.6b-q8");
        assert!(d.refine.is_none(), "single-pass is the default: no refine model is resolved");
        assert_eq!(d.mt.primary.id.as_str(), DEFAULT_MT_PROVIDER);
        assert_eq!(d.mt.primary.model, "gemma-4-e4b-it-qat-q4-0");
        assert_eq!(d.mt.primary.base_url, "http://127.0.0.1:8080/v1");
        assert_eq!(d.mt.primary.prompt_template.name(), "gemma4");
        assert_eq!(
            d.mt.primary.serves,
            Some(ModelId::new("gemma-4-e4b-it-qat-q4-0"))
        );
        assert_eq!(
            d.mt.primary.params["chat_template_kwargs"]["enable_thinking"],
            false
        );
        assert_eq!(d.mt.primary.params["temperature"], 0);
        assert!(!d.mt.primary.params.contains_key("prompt_template"));
        assert_eq!(d.tts.primary.id.as_str(), TTS);
        assert!(d.tts.fallback.is_none());
    }
    assert_eq!(out.tts.primary.lang, Lang::En);
    assert_eq!(
        out.tts.primary.voice.selection(),
        VoiceSelection::Cloned {
            profile_id: OWNER.into()
        }
    );
    assert_eq!(inc.tts.primary.lang, Lang::Zh);
    assert_eq!(
        inc.tts.primary.voice.selection(),
        VoiceSelection::Preset {
            name: "preset-zh-1".into()
        }
    );
    assert!(matches!(out.tts.primary.voice, VoicePlan::Cloned { .. }));
    // Model fingerprints come from the catalogue's checksums, not from hashing the files.
    assert_eq!(out.tts.primary.fingerprint, rig.fingerprint());
}

#[test]
fn without_the_native_features_the_native_roles_are_typed_not_built_in_problems() {
    let mut rig = Rig::new();
    rig.cfg.pipeline.recognition_mode = "two_pass".into();
    let registry = crate::registry::ProviderRegistry::new(
        &rig.cfg,
        rig.catalogue.clone(),
        rig.models.clone() as _,
        std::sync::Arc::new(crate::credentials::EnvCredentials::with_lookup(|_| None)),
        rig.dirs.clone(),
    )
    .with_hardware(prxvc_models::HardwareInfo::new(
        32,
        prxvc_models::Os::Macos,
        prxvc_models::Arch::Aarch64,
    ))
    .with_system_language(Some(Lang::Zh))
    .with_tts_loader(std::sync::Arc::new(crate::tts_loader::BuiltInTts));
    let problems = registry.resolve_call().err().unwrap_or_default();
    let not_built_in: Vec<_> = problems
        .iter()
        .filter_map(|p| match &p.kind {
            ProblemKind::RuntimeNotBuiltIn { id, runtime } => {
                Some((p.role.unwrap(), id.as_str(), runtime.as_str()))
            }
            _ => None,
        })
        .collect();
    // The recognition runtime is a problem exactly when the feature is off, for every role that
    // uses it, under the catalogue's runtime name.
    let recognition = [
        (RoleKey::Vad, "silero-vad-6.2"),
        (RoleKey::Asr(Lang::Zh), "qwen3-asr-0.6b-q8"),
        (RoleKey::Asr(Lang::En), "qwen3-asr-0.6b-q8"),
        (RoleKey::AsrRefine(Lang::Zh), "qwen3-asr-1.7b-q8"),
    ];
    for (role, id) in recognition {
        assert_eq!(
            not_built_in.contains(&(role, id, "transcribe.cpp")),
            !cfg!(feature = "transcribe"),
            "{role:?} {id}"
        );
    }
    assert_eq!(
        not_built_in.contains(&(RoleKey::Tts(Lang::En), TTS, "qwentts.cpp")),
        !cfg!(feature = "qwentts"),
        "the synthesis runtime is a problem exactly when the feature is off"
    );
    // The translation default is an HTTP endpoint: nothing to build in, so no problem.
    assert!(
        problems
            .iter()
            .all(|p| p.role.is_none_or(|r| r.role() != crate::keys::CallRole::Mt))
    );
}

#[test]
fn explicit_selections_override_the_defaults_and_fallbacks_are_separate_providers() {
    let mut rig = Rig::new();
    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "openai-mt".into());
    rig.cfg
        .providers
        .fallback
        .mt
        .insert("zh>en".into(), DEFAULT_MT_PROVIDER.into());
    rig.cfg
        .providers
        .fallback
        .mt
        .insert("en>zh".into(), "none".into());
    let registry = rig.registry();
    let mt = registry.resolve_mt(Lang::Zh, Lang::En).unwrap();
    assert_eq!(mt.primary.id.as_str(), "openai-mt");
    assert_eq!(mt.primary.host, "api.openai.com");
    assert_eq!(mt.primary.locality, Locality::Remote);
    assert_eq!(mt.primary.data_leaving(), DataLeaving::Text);
    assert_eq!(
        mt.primary.params["temperature"], 0.2,
        "the template's default applies"
    );
    assert_eq!(
        mt.fallback.as_ref().unwrap().id.as_str(),
        DEFAULT_MT_PROVIDER
    );
    let back = registry.resolve_mt(Lang::En, Lang::Zh).unwrap();
    assert_eq!(back.primary.id.as_str(), DEFAULT_MT_PROVIDER);
    assert!(back.fallback.is_none(), "`none` means no fallback");
    assert!(back.warnings.is_empty());
}

#[test]
fn a_secret_never_shows_in_a_spec_dump() {
    let mut rig = Rig::new();
    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "openai-mt".into());
    let spec = rig
        .registry()
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap()
        .primary;
    assert!(spec.secret.is_some());
    assert!(!format!("{spec:?}").contains("sk-test-secret"));
}

#[test]
fn a_catalogue_model_can_serve_translation_through_llama_server() {
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "gemma-4-e4b-it-qat-q4-0".into());
    let spec = rig
        .registry()
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap()
        .primary;
    assert_eq!(spec.id.as_str(), "gemma-4-e4b-it-qat-q4-0");
    assert_eq!(spec.prompt_template.name(), "gemma4");
    rig.models.set(
        "gemma-4-e4b-it-qat-q4-0",
        prxvc_models::NotReady::NotInstalled {
            id: ModelId::new("gemma-4-e4b-it-qat-q4-0"),
            size: 5,
        },
    );
    let Err(problems) = rig.registry().resolve_mt(Lang::Zh, Lang::En) else {
        panic!()
    };
    assert!(matches!(
        &problems[0].kind,
        ProblemKind::ModelNotInstalled { size_bytes: 5, .. }
    ));
}

#[test]
fn the_voice_follows_the_direction_and_the_settings() {
    let mut rig = Rig::new();
    rig.cfg.voice.use_clone_outgoing = false;
    rig.cfg.voice.fallback_preset = "preset-en-1".into();
    rig.cfg.voice.incoming_preset = "preset-zh-1".into();
    let plan = rig.registry().resolve_call().unwrap();
    assert_eq!(
        plan.outgoing.tts.primary.voice.selection(),
        VoiceSelection::Preset {
            name: "preset-en-1".into()
        }
    );
    let one = rig
        .registry()
        .resolve_tts(Lang::Zh, &VoiceChoice::Preset("preset-zh-1".into()))
        .unwrap();
    assert_eq!(
        one.primary.voice.selection(),
        VoiceSelection::Preset {
            name: "preset-zh-1".into()
        }
    );
}

#[test]
fn call_languages_come_from_configuration_or_are_derived() {
    let mut rig = Rig::new();
    rig.cfg.call.self_language = "en".into();
    let l = rig.registry().call_languages().unwrap();
    assert_eq!((l.own, l.peer), (Lang::En, Lang::Zh));
    rig.cfg.call.peer_language = "en".into();
    rig.cfg.call.self_language = "zh-CN".into();
    let l = rig.registry().call_languages().unwrap();
    assert_eq!((l.own, l.peer), (Lang::Zh, Lang::En));
    rig.cfg.call.self_language = "fr".into();
    let Err(p) = rig.registry().call_languages() else {
        panic!()
    };
    assert!(matches!(p[0].kind, ProblemKind::InvalidLanguage { .. }));
    rig.cfg.call.self_language.clear();
    rig.cfg.call.peer_language.clear();
    let l = rig.registry().call_languages().unwrap();
    assert_eq!(
        (l.own, l.peer),
        (Lang::Zh, Lang::En),
        "derived from the interface language"
    );
    rig.cfg.ui.language = "system".into();
    let l = rig
        .registry()
        .with_system_language(Some(Lang::En))
        .call_languages()
        .unwrap();
    assert_eq!(
        (l.own, l.peer),
        (Lang::En, Lang::Zh),
        "derived from the system language"
    );
    let registry = rig.registry().with_system_language(None);
    let Err(p) = registry.call_languages() else {
        panic!()
    };
    assert!(matches!(p[0].kind, ProblemKind::LanguageNotSet { .. }));
}

#[test]
fn the_disclosure_lists_what_leaves_the_machine() {
    let mut rig = Rig::new();
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let local = registry.egress_summary(&plan);
    assert!(!local.leaves_this_computer());
    assert!(local.rows.iter().all(|r| r.data == DataLeaving::Nothing));

    rig.openai_entry("openai-mt", "https://api.openai.com/v1");
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "openai-mt".into());
    let registry = rig.registry();
    let summary = registry.egress_summary(&registry.resolve_call().unwrap());
    assert!(summary.leaves_this_computer());
    let text: Vec<_> = summary.rows_sending(DataLeaving::Text).collect();
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].host.as_deref(), Some("api.openai.com"));
    assert_eq!(text[0].provider, ProviderId::new("openai-mt"));
    assert_eq!(summary.rows_sending(DataLeaving::Audio).count(), 0);
}

#[test]
fn a_fallback_that_cannot_be_resolved_is_a_warning_while_the_primary_works() {
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .fallback
        .tts
        .insert("en".into(), "nope".into());
    let plan = rig.registry().resolve_call().expect("the primary resolves");
    assert_eq!(plan.warnings.len(), 1);
    assert_eq!(plan.warnings[0].slot, Slot::Fallback);
    assert_eq!(plan.warnings[0].role, Some(RoleKey::Tts(Lang::En)));
    assert_eq!(
        kinds(&plan.warnings),
        [&ProblemKind::UnknownProvider { id: "nope".into() }]
    );
    assert!(plan.outgoing.tts.fallback.is_none());
}

#[test]
fn a_valid_fallback_is_resolved_and_validated_like_the_primary() {
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .fallback
        .tts
        .insert("en".into(), "qwen3-tts-0.6b-base-q8".into());
    rig.prepare_profile(
        OWNER,
        &crate::fingerprint::model_fingerprint(
            rig.catalogue
                .get(&ModelId::new("qwen3-tts-0.6b-base-q8"))
                .unwrap(),
        ),
    );
    let tts = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::OwnClone)
        .unwrap();
    let fb = tts.fallback.expect("the 0.6B model is the fallback");
    assert_eq!(fb.id.as_str(), "qwen3-tts-0.6b-base-q8");
    assert_ne!(fb.fingerprint, tts.primary.fingerprint);
    assert!(tts.warnings.is_empty());
}

#[test]
fn the_defaults_of_the_call_roles_are_the_same_on_every_machine() {
    for (gb, os, arch) in [
        (8, prxvc_models::Os::Macos, prxvc_models::Arch::Aarch64),
        (64, prxvc_models::Os::Linux, prxvc_models::Arch::X86_64),
        (16, prxvc_models::Os::Windows, prxvc_models::Arch::X86_64),
    ] {
        let rig = Rig::new();
        let registry = rig
            .registry()
            .with_hardware(prxvc_models::HardwareInfo::new(gb, os, arch));
        for lang in [Lang::Zh, Lang::En] {
            assert_eq!(
                registry.resolve_asr(lang).unwrap().primary.id.as_str(),
                "qwen3-asr-0.6b-q8"
            );
            assert_eq!(
                registry
                    .resolve_asr_refine(lang)
                    .unwrap()
                    .primary
                    .id
                    .as_str(),
                "qwen3-asr-1.7b-q8"
            );
        }
        assert_eq!(
            registry.resolve_vad().unwrap().primary.id.as_str(),
            "silero-vad-6.2"
        );
    }
}
