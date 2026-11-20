//! Live check of the whole call plan with the real runtimes (CT-11 and CT-14): resolve the plan of a
//! fixture configuration against the real model files, build every provider of both directions, run
//! each "test provider" function, and free everything. `#[ignore]`d: it needs the models and the GPU.
//! Nothing is played and nothing is recorded; no server is started. When no translation server
//! listens at `PRXVC_MT_URL`, that role reports its classified connection error and the test goes on.
//!
//! ```text
//! cargo test -p prxvc-providers --features "transcribe qwentts" --release --test call_plan_live \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Locations (each overridable by an environment variable):
//!
//! | Variable | Default | Holds |
//! |---|---|---|
//! | `PRXVC_BENCH_ROOT` | `/Users/nn/worker/voice/bench` | the benchmark root |
//! | `PRXVC_ASR_MODELS` | `<root>/asr/models/hf` | Qwen3-ASR and Silero GGUF files, one directory per repository |
//! | `PRXVC_TTS_MODELS` | `<root>/tts/models` | the synthesis talker and codec GGUF files |
//! | `PRXVC_TTS_REFS` | `<root>/tts/refs` | the owner's reference recordings and the stand-in voices |
//! | `PRXVC_MT_URL` | `http://127.0.0.1:8080/v1` | the OpenAI-compatible translation server |
//! | `PRXVC_MT_MODEL` | `gemma-4-e4b-it-qat-q4-0` | the model name sent to it |
#![cfg(all(feature = "transcribe", feature = "qwentts"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_config::{AppConfig, ProviderEntry};
use prxvc_media::{Direction, Lang};
use prxvc_models::{Catalogue, ModelId, NotReady, ReadyFile, ReadyModel};
use prxvc_providers::{
    EnvCredentials, FailureClass, ModelSource, ProviderRegistry, TestReport, VoiceChoice,
    VoiceDirs, test_provider,
};

const TIMEOUT: Duration = Duration::from_secs(60);
const PROFILE: &str = "owner-live";

fn env_path(var: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    std::env::var_os(var).map_or_else(default, PathBuf::from)
}

fn bench_root() -> PathBuf {
    env_path("PRXVC_BENCH_ROOT", || {
        PathBuf::from("/Users/nn/worker/voice/bench")
    })
}

fn asr_models() -> PathBuf {
    env_path("PRXVC_ASR_MODELS", || bench_root().join("asr/models/hf"))
}

fn tts_models() -> PathBuf {
    env_path("PRXVC_TTS_MODELS", || bench_root().join("tts/models"))
}

fn tts_refs() -> PathBuf {
    env_path("PRXVC_TTS_REFS", || bench_root().join("tts/refs"))
}

/// Finds the files a catalogue entry names in the model directories, standing in for the model
/// manager: `<tts>/<name>` or `<asr>/<repository directory>/<name>`.
struct BenchModels {
    catalogue: Arc<Catalogue>,
}

fn locate(name: &str) -> Option<PathBuf> {
    let flat = tts_models().join(name);
    if flat.is_file() {
        return Some(flat);
    }
    fs::read_dir(asr_models())
        .ok()?
        .flatten()
        .map(|d| d.path().join(name))
        .find(|p| p.is_file())
}

impl ModelSource for BenchModels {
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady> {
        let entry = self.catalogue.get(id).ok_or(NotReady::NoSelection)?;
        let mut files = Vec::new();
        for f in &entry.files {
            let Some(path) = locate(&f.name) else {
                return Err(NotReady::NotInstalled {
                    id: id.clone(),
                    size: f.size,
                });
            };
            files.push(ReadyFile {
                purpose: f.purpose.clone(),
                path,
            });
        }
        Ok(ReadyModel {
            id: entry.id.clone(),
            revision_key: entry.revision_key(),
            role: entry.role,
            runtime: entry.runtime.name.clone(),
            runtime_params: serde_json::json!({}),
            files,
        })
    }
}

/// The embedded catalogue with voice presets built from the stand-in recordings (the embedded
/// catalogue ships none yet), for each synthesis model.
fn catalogue_with_presets(presets_dir: &Path) -> Catalogue {
    let embedded = Catalogue::embedded();
    let mut value = serde_json::to_value(&embedded).unwrap();
    let mut presets = Vec::new();
    for model in embedded.models() {
        if model.role != prxvc_models::Role::Tts {
            continue;
        }
        for (lang, source) in [("zh", "zh_ref.wav"), ("en", "en_ref.wav")] {
            let id = format!("live-{lang}-{}", model.id);
            let dir = presets_dir.join(&id);
            fs::create_dir_all(&dir).unwrap();
            let target = dir.join(source);
            fs::copy(tts_refs().join(source), &target).expect("the stand-in voice recording");
            presets.push(serde_json::json!({
                "id": id, "model_id": model.id, "language": lang,
                "display_name": format!("Stand-in {lang}"),
                "file": {
                    "name": source, "purpose": "reference",
                    "size": fs::metadata(&target).unwrap().len(),
                    "sha256": "0".repeat(64),
                    "source": {"kind": "url", "url": "https://example.invalid/stand-in.wav"}
                },
                "licence": "CC0"
            }));
        }
    }
    value["presets"] = serde_json::Value::Array(presets);
    Catalogue::parse(&value.to_string()).expect("the fixture catalogue is valid")
}

/// The owner's profile: the English recording of the owner (the outgoing direction speaks English).
fn write_owner_profile(profiles: &Path) {
    let dir = profiles.join(PROFILE);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(tts_refs().join("en_owner.wav"), dir.join("reference.wav"))
        .expect("the owner's recording");
    fs::write(
        dir.join("profile.json"),
        r#"{"schema_version":1,"id":"owner-live","language":"en","x_vector_only":true}"#,
    )
    .unwrap();
}

/// The fixture configuration: Chinese on this side, English on the other, the owner's voice, and the
/// translation server of `PRXVC_MT_URL`.
fn fixture_config() -> (AppConfig, String) {
    let url = std::env::var("PRXVC_MT_URL").unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into());
    let model =
        std::env::var("PRXVC_MT_MODEL").unwrap_or_else(|_| "gemma-4-e4b-it-qat-q4-0".into());
    let mut cfg = AppConfig::default();
    cfg.call.self_language = "zh".into();
    cfg.call.peer_language = "en".into();
    cfg.voice.self_profile_id = PROFILE.into();
    cfg.providers.entries.push(ProviderEntry {
        id: "live-mt".into(),
        template: "llama-server".into(),
        protocol: "openai-chat".into(),
        base_url: url.clone(),
        model,
        local_endpoint: true,
        ..ProviderEntry::default()
    });
    for pair in ["zh>en", "en>zh"] {
        cfg.providers
            .active
            .mt
            .insert(pair.into(), "live-mt".into());
    }
    (cfg, url)
}

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1e3)
}

fn processes() -> usize {
    let out = std::process::Command::new("/bin/ps")
        .arg("-A")
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout).lines().count()
}

fn show(label: &str, report: &TestReport) {
    println!(
        "  test {label}: ok={} first={} total={} detail={:?} error={:?}",
        report.ok,
        report.first_response.map_or_else(|| "-".into(), ms),
        ms(report.total),
        report.detail,
        report.error
    );
}

#[test]
#[ignore = "needs the recognition, VAD and synthesis models, the owner's recordings and the GPU"]
fn the_real_call_plan_resolves_builds_tests_and_frees() {
    let before = processes();
    let tmp = tempfile::tempdir().unwrap();
    let dirs = VoiceDirs {
        profiles: tmp.path().join("voices"),
        presets: tmp.path().join("presets"),
    };
    write_owner_profile(&dirs.profiles);
    let catalogue = Arc::new(catalogue_with_presets(&dirs.presets));
    let (cfg, mt_url) = fixture_config();
    let registry = ProviderRegistry::new(
        &cfg,
        Arc::clone(&catalogue),
        Arc::new(BenchModels { catalogue }),
        Arc::new(EnvCredentials::from_process()),
        dirs,
    );

    // The owner's voice is prepared once per synthesis model, as after enrolment.
    let t = Instant::now();
    let prepared = registry
        .prepare_cloned_voice(Lang::En)
        .expect("prepare the owner's voice");
    println!(
        "prepare_cloned_voice: {} (cache hit: {})",
        ms(t.elapsed()),
        prepared.cache_hit
    );
    let live = registry.live_engines();
    assert_eq!(
        (live.tts, live.asr, live.vad),
        (0, 0, 0),
        "preparing leaves no engine behind"
    );

    let t = Instant::now();
    let plan = registry.resolve_call().expect("the call plan resolves");
    println!("resolve_call: {}", ms(t.elapsed()));
    println!("recognition mode: {:?}", plan.recognition);
    println!("vad: {}", plan.vad.primary.id);
    for d in [Direction::Outgoing, Direction::Incoming] {
        let p = plan.direction(d);
        println!(
            "{d:?} {}>{}: asr={} refine={} mt={} ({}) tts={}",
            p.source,
            p.target,
            p.asr.primary.id,
            p.refine
                .as_ref()
                .map_or_else(|| "-".into(), |r| r.primary.id.to_string()),
            p.mt.primary.id,
            p.mt.primary.model,
            p.tts.primary.id,
        );
    }
    println!("mt endpoint: {mt_url}");

    // Build every provider of both directions.
    let timed = |what: &str, t: Instant| println!("build {what}: {}", ms(t.elapsed()));
    let t = Instant::now();
    let vad = registry.build_vad(&plan.vad.primary).expect("vad");
    timed("vad", t);
    let mut asr = Vec::new();
    let mut refine = Vec::new();
    let mut mt = Vec::new();
    let mut tts = Vec::new();
    for d in [Direction::Outgoing, Direction::Incoming] {
        let p = plan.direction(d);
        let t = Instant::now();
        asr.push((p.source, registry.build_asr(&p.asr.primary).expect("asr")));
        timed(&format!("asr {d:?}"), t);
        if let Some(r) = &p.refine {
            let t = Instant::now();
            refine.push((
                p.source,
                registry.build_asr_refine(&r.primary).expect("refine"),
            ));
            timed(&format!("refine {d:?}"), t);
        }
        let t = Instant::now();
        mt.push((
            p.source,
            p.target,
            registry.build_mt(&p.mt.primary).expect("mt"),
        ));
        timed(&format!("mt {d:?}"), t);
        let t = Instant::now();
        tts.push((
            p.target,
            p.tts.primary.voice.selection(),
            registry.build_tts(&p.tts.primary).expect("tts"),
        ));
        timed(&format!("tts {d:?}"), t);
    }
    let live = registry.live_engines();
    println!("live engines after building both directions: {live:?}");
    assert_eq!(live.vad, 1, "one VAD engine for both directions");
    assert_eq!(live.asr, 1, "one recognition engine for both directions");
    assert_eq!(live.tts, 1, "one synthesis engine for both directions");
    assert_eq!(live.mt, 1, "one translation client for both directions");

    // Run each test provider function.
    let r = test_provider::test_vad(&*vad);
    show("vad", &r);
    assert!(r.ok, "{r:?}");
    for (lang, handle) in &asr {
        let r = test_provider::test_recogniser(&**handle, *lang, TIMEOUT);
        show(&format!("recogniser {lang}"), &r);
        assert!(r.ok, "{r:?}");
    }
    for (lang, handle) in &refine {
        let r = test_provider::test_refiner(&**handle, *lang, TIMEOUT);
        show(&format!("refiner {lang}"), &r);
        assert!(r.ok, "{r:?}");
    }
    let mut mt_unreachable = false;
    for (source, target, handle) in &mt {
        let r = test_provider::test_translator(&**handle, *source, *target, TIMEOUT);
        show(&format!("translator {source}>{target}"), &r);
        match &r.error {
            None => assert!(r.ok, "{r:?}"),
            Some(e) => {
                assert_eq!(
                    e.class,
                    FailureClass::Unreachable,
                    "only a missing server may fail this role: {e:?}"
                );
                mt_unreachable = true;
            }
        }
    }
    for round in 1..=2 {
        for (lang, voice, handle) in &tts {
            let r = test_provider::test_synth(&**handle, *lang, voice.clone(), TIMEOUT);
            show(&format!("synthesiser {lang} #{round}"), &r);
            assert!(r.ok, "{r:?}");
        }
    }
    if mt_unreachable {
        println!("translation: no server at {mt_url}; the role reported its classified error");
    }

    // The preset voice of the incoming direction is also a plain resolution result.
    let preset = registry
        .resolve_tts(Lang::Zh, &VoiceChoice::Preset(String::new()))
        .expect("the stand-in preset resolves");
    println!("incoming preset: {:?}", preset.primary.voice.selection());

    // Free everything: the last handle of each engine frees it.
    drop((vad, asr, refine, mt, tts));
    let live = registry.live_engines();
    println!("live engines after dropping every handle: {live:?}");
    assert_eq!(
        (live.vad, live.asr, live.refine, live.tts, live.mt),
        (0, 0, 0, 0, 0)
    );
    drop(registry);
    let after = processes();
    println!("ps -A line count: before {before}, after {after}");
    assert!(
        after <= before + 5,
        "no helper process may be left behind: {before} -> {after}"
    );
}
