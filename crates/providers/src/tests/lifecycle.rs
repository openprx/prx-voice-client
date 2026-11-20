//! Engine sharing and lifecycle over fake engines: construction, sharing between the directions,
//! release, retirement after a failure.

use std::sync::atomic::Ordering;
use std::time::Duration;

use prxvc_media::Lang;
use prxvc_ports::SpeechSynth;

use super::fixtures::*;
use crate::problem::BuildError;
use crate::test_provider::{FailureClass, test_synth};

const T: Duration = Duration::from_secs(10);

fn speak(tts: &dyn SpeechSynth, spec: &crate::spec::TtsSpec) -> crate::test_provider::TestReport {
    test_synth(tts, spec.lang, spec.voice.selection(), T)
}

#[test]
fn both_directions_share_one_synthesis_engine_and_closing_frees_it() {
    let rig = Rig::new();
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let out = registry.build_tts(&plan.outgoing.tts.primary).unwrap();
    let inc = registry.build_tts(&plan.incoming.tts.primary).unwrap();
    assert_eq!(
        rig.tts_loader.loads.load(Ordering::SeqCst),
        1,
        "one engine for two directions"
    );
    assert_eq!(registry.live_engines().tts, 1);

    // Each direction speaks with its own voice on the shared engine.
    assert!(speak(&*out, &plan.outgoing.tts.primary).ok);
    assert!(speak(&*inc, &plan.incoming.tts.primary).ok);

    drop(out);
    assert_eq!(
        registry.live_engines().tts,
        1,
        "the incoming handle still holds it"
    );
    assert_eq!(rig.tts_loader.dropped.load(Ordering::SeqCst), 0);
    drop(inc);
    assert_eq!(registry.live_engines().tts, 0);
    assert_eq!(
        rig.tts_loader.dropped.load(Ordering::SeqCst),
        1,
        "the engine was freed"
    );

    // Entering call mode again loads it again.
    let _again = registry.build_tts(&plan.outgoing.tts.primary).unwrap();
    assert_eq!(rig.tts_loader.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn a_failed_engine_is_retired_and_rebuilt() {
    let rig = Rig::new();
    rig.tts_loader.fail_first.store(true, Ordering::SeqCst);
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let spec = &plan.outgoing.tts.primary;
    let first = registry.build_tts(spec).unwrap();
    let report = speak(&*first, spec);
    assert!(!report.ok, "{report:?}");
    assert_eq!(
        report.error.as_ref().unwrap().class,
        FailureClass::BadResponse
    );

    // The failure retired the engine: the next build makes a new one that works.
    let second = registry.build_tts(spec).unwrap();
    assert_eq!(rig.tts_loader.loads.load(Ordering::SeqCst), 2);
    assert!(speak(&*second, spec).ok);
    drop(first);
    assert_eq!(
        rig.tts_loader.dropped.load(Ordering::SeqCst),
        1,
        "the retired engine goes with its last handle"
    );
    drop(second);
    assert_eq!(rig.tts_loader.dropped.load(Ordering::SeqCst), 2);
}

#[test]
fn a_load_that_fails_leaves_nothing_behind_and_can_be_retried() {
    let rig = Rig::new();
    rig.tts_loader.unavailable.store(true, Ordering::SeqCst);
    let registry = rig.registry();
    let spec = {
        rig.tts_loader.unavailable.store(false, Ordering::SeqCst);
        registry.resolve_call().unwrap().outgoing.tts.primary
    };
    rig.tts_loader.unavailable.store(true, Ordering::SeqCst);
    let Err(err) = registry.build_tts(&spec) else {
        panic!("the runtime is gone")
    };
    assert!(matches!(err, BuildError::NotBuiltIn { .. }));
    assert_eq!(registry.live_engines().tts, 0);
    rig.tts_loader.unavailable.store(false, Ordering::SeqCst);
    assert!(registry.build_tts(&spec).is_ok());
}

#[test]
fn a_missing_reference_recording_is_a_typed_voice_error_at_build() {
    let rig = Rig::new();
    let registry = rig.registry();
    let spec = registry.resolve_call().unwrap().outgoing.tts.primary;
    std::fs::remove_file(rig.dirs.profiles.join(OWNER).join("reference.wav")).unwrap();
    let Err(err) = registry.build_tts(&spec) else {
        panic!()
    };
    assert!(matches!(err, BuildError::Voice { .. }), "{err:?}");
}

#[test]
fn one_translation_client_serves_both_directions() {
    let rig = Rig::new();
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let a = registry.build_mt(&plan.outgoing.mt.primary).unwrap();
    let b = registry.build_mt(&plan.incoming.mt.primary).unwrap();
    assert_eq!(registry.live_engines().mt, 1);
    assert_eq!(a.descriptor().model, "gemma-4-e4b-it-qat-q4-0");
    assert!(a.descriptor().require_pair(Lang::Zh, Lang::En).is_ok());
    drop((a, b));
    assert_eq!(registry.live_engines().mt, 0);
}

#[test]
fn recognition_and_vad_engines_are_shared_the_same_way() {
    let rig = Rig::new();
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let out_asr = registry.build_asr(&plan.outgoing.asr.primary).unwrap();
    let in_asr = registry.build_asr(&plan.incoming.asr.primary).unwrap();
    let v1 = registry.build_vad(&plan.vad.primary).unwrap();
    let v2 = registry.build_vad(&plan.vad.primary).unwrap();
    let refine = registry
        .build_asr_refine(&plan.outgoing.refine.as_ref().unwrap().primary)
        .unwrap();
    assert_eq!(rig.recognition.asr_loads.load(Ordering::SeqCst), 1);
    assert_eq!(rig.recognition.vad_loads.load(Ordering::SeqCst), 1);
    assert_eq!(rig.recognition.refine_loads.load(Ordering::SeqCst), 1);
    let live = registry.live_engines();
    assert_eq!((live.asr, live.vad, live.refine), (1, 1, 1));
    assert!(
        v1.open().is_ok() && v2.open().is_ok(),
        "each direction opens its own engine"
    );
    drop((out_asr, in_asr, v1, v2, refine));
    let live = registry.live_engines();
    assert_eq!((live.asr, live.vad, live.refine), (0, 0, 0));
}

fn bare_registry(rig: &Rig) -> crate::registry::ProviderRegistry {
    crate::registry::ProviderRegistry::new(
        &rig.cfg,
        rig.catalogue.clone(),
        rig.models.clone() as _,
        std::sync::Arc::new(crate::credentials::EnvCredentials::with_lookup(|_| None)),
        rig.dirs.clone(),
    )
}

#[cfg(not(feature = "transcribe"))]
#[test]
fn the_default_binary_builds_no_recognition_provider() {
    let rig = Rig::new();
    let plan = rig.registry().resolve_call().unwrap();
    // A registry with the binary's own loaders: the recognition adapters are not built in.
    let bare = bare_registry(&rig);
    let Err(err) = bare.build_vad(&plan.vad.primary) else {
        panic!()
    };
    assert!(
        matches!(err, BuildError::NotBuiltIn { ref runtime, .. } if runtime == "transcribe.cpp")
    );
    let Err(err) = bare.build_asr(&plan.outgoing.asr.primary) else {
        panic!()
    };
    assert!(matches!(err, BuildError::NotBuiltIn { .. }));
    let Err(err) = bare.build_asr_refine(&plan.outgoing.refine.as_ref().unwrap().primary) else {
        panic!()
    };
    assert!(matches!(err, BuildError::NotBuiltIn { .. }));
    assert_eq!(bare.live_engines(), crate::registry::LiveEngines::default());
}

/// With the feature on, the binary's loaders try to load the model file: the fixture's files do
/// not exist, so the build fails with the loader's own start error, never a stand-in.
#[cfg(feature = "transcribe")]
#[test]
fn the_native_binary_loads_the_model_file_and_reports_a_missing_one() {
    let rig = Rig::new();
    let plan = rig.registry().resolve_call().unwrap();
    let bare = bare_registry(&rig);
    let Err(err) = bare.build_vad(&plan.vad.primary) else {
        panic!("the fixture file does not exist")
    };
    assert!(matches!(err, BuildError::Start { .. }), "{err}");
    let Err(err) = bare.build_asr(&plan.outgoing.asr.primary) else {
        panic!("the fixture file does not exist")
    };
    assert!(matches!(err, BuildError::Start { .. }), "{err}");
    assert_eq!(bare.live_engines(), crate::registry::LiveEngines::default());
}

#[test]
fn an_open_stream_keeps_its_engine_alive_after_the_handle_is_dropped() {
    let rig = Rig::new();
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let handle = registry.build_asr(&plan.outgoing.asr.primary).unwrap();
    let stream = handle
        .open(Lang::Zh, prxvc_ports::AsrContext::default())
        .unwrap();
    drop(handle);
    assert_eq!(registry.live_engines().asr, 1, "the stream holds a lease");
    drop(stream);
    assert_eq!(registry.live_engines().asr, 0, "freed with the last stream");
}

#[test]
fn an_engine_failure_seen_by_a_stream_retires_the_engine_and_the_next_build_loads_a_new_one() {
    use prxvc_media::MonoTime;
    let rig = Rig::new();
    rig.recognition.fail_first_asr.store(true, Ordering::SeqCst);
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let first = registry.build_asr(&plan.outgoing.asr.primary).unwrap();
    let mut stream = first
        .open(Lang::Zh, prxvc_ports::AsrContext::default())
        .unwrap();
    assert!(stream.feed(&[0.0; 160], MonoTime::ZERO).is_err());
    let second = registry.build_asr(&plan.incoming.asr.primary).unwrap();
    assert_eq!(rig.recognition.asr_loads.load(Ordering::SeqCst), 2);
    let mut ok = second
        .open(Lang::En, prxvc_ports::AsrContext::default())
        .unwrap();
    assert!(ok.feed(&[0.0; 160], MonoTime::ZERO).unwrap().is_some());
    drop((first, stream));
    assert_eq!(
        registry.live_engines().asr,
        1,
        "the retired engine is freed"
    );
}

#[test]
fn the_recogniser_is_loaded_with_the_pipeline_section_and_a_changed_setting_loads_a_new_engine() {
    let mut rig = Rig::new();
    rig.cfg.pipeline.asr_redecode_interval_ms = 500;
    rig.cfg.pipeline.max_segment_s = 20;
    let registry = rig.registry();
    let plan = registry.resolve_call().unwrap();
    let a = registry.build_asr(&plan.outgoing.asr.primary).unwrap();
    let params = rig.recognition.last_params.lock().clone().unwrap();
    assert_eq!(params.redecode_interval, Duration::from_millis(500));
    assert_eq!(params.max_segment, Duration::from_secs(20));
    let b = registry.build_asr(&plan.incoming.asr.primary).unwrap();
    assert_eq!(
        rig.recognition.asr_loads.load(Ordering::SeqCst),
        1,
        "shared"
    );
    let mut changed = rig.cfg.clone();
    changed.pipeline.asr_redecode_interval_ms = 600;
    registry.reload(&changed);
    let c = registry.build_asr(&plan.outgoing.asr.primary).unwrap();
    assert_eq!(
        rig.recognition.asr_loads.load(Ordering::SeqCst),
        2,
        "the next call follows the new setting"
    );
    drop((a, b, c));
}

#[test]
fn a_recogniser_that_gives_only_finals_is_refused_at_build() {
    use prxvc_ports::fakes::{AsrScript, FakeStreamingAsr};
    struct FinalsOnly;
    impl crate::recognition::RecognitionLoader for FinalsOnly {
        fn available(&self) -> bool {
            true
        }
        fn vad(
            &self,
            _s: &crate::spec::LocalModelSpec,
        ) -> Result<std::sync::Arc<dyn prxvc_ports::VadProvider>, BuildError> {
            unreachable!()
        }
        fn asr(
            &self,
            _s: &crate::spec::LocalModelSpec,
            _p: &prxvc_adapters::asr::AsrParams,
        ) -> Result<std::sync::Arc<dyn prxvc_ports::StreamingAsr>, BuildError> {
            let d = prxvc_ports::ProviderDescriptor::new(
                "x",
                "x",
                prxvc_ports::StreamingKind::Segmented,
            );
            Ok(std::sync::Arc::new(
                FakeStreamingAsr::new(prxvc_media::ManualClock::new(), AsrScript::new([], ""))
                    .with_descriptor(d),
            ))
        }
        fn refine(
            &self,
            _s: &crate::spec::LocalModelSpec,
            _p: &prxvc_adapters::asr::AsrParams,
        ) -> Result<std::sync::Arc<dyn prxvc_ports::RefineAsr>, BuildError> {
            unreachable!()
        }
    }
    let rig = Rig::new();
    let registry = rig
        .registry()
        .with_recognition_loader(std::sync::Arc::new(FinalsOnly));
    let plan = registry.resolve_call().unwrap();
    let Err(err) = registry.build_asr(&plan.outgoing.asr.primary) else {
        panic!("a finals-only engine must not serve the live role")
    };
    assert!(matches!(err, BuildError::Capability(_)), "{err}");
}

#[test]
fn the_binary_loader_reports_the_feature() {
    use crate::tts_loader::{BuiltInTts, TtsLoader};
    assert_eq!(BuiltInTts.available(), cfg!(feature = "qwentts"));
}

#[test]
fn preparing_the_owners_voice_makes_an_unprepared_profile_resolve() {
    let mut rig = Rig::new();
    rig.write_profile("fresh", false);
    rig.cfg.voice.self_profile_id = "fresh".into();
    let registry = rig.registry();
    assert!(
        registry.resolve_call().is_err(),
        "an unprepared voice is a problem"
    );

    let first = registry.prepare_cloned_voice(Lang::En).unwrap();
    assert_eq!(
        (first.profile_id.as_str(), first.cache_hit),
        ("fresh", false)
    );
    assert!(
        registry.resolve_call().is_ok(),
        "the prepared voice now resolves"
    );
    let second = registry.prepare_cloned_voice(Lang::En).unwrap();
    assert!(second.cache_hit, "the cache served the second preparation");
    assert_eq!(
        registry.live_engines().tts,
        0,
        "preparing does not keep the engine alive"
    );
    assert_eq!(rig.tts_loader.dropped.load(Ordering::SeqCst), 2);
}
