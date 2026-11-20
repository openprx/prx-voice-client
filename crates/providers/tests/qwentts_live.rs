//! Live check of the TTS provider with the real runtime on Metal (CT-14). `#[ignore]`d: it needs
//! the models and the GPU, and nothing is played.
//!
//! ```text
//! cargo test -p prxvc-providers --features qwentts --release --test qwentts_live \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `PRXVC_BENCH_ROOT` is the workspace `bench` directory (default
//! `/Users/nn/worker/voice/bench`): the models are read in place from `<root>/tts/models`
//! (`qwen-talker-1.7b-base-Q8_0.gguf`, `qwen-tokenizer-12hz-Q8_0.gguf`) and the owner's recording
//! from `<root>/tts/refs/en_owner.wav`. The configuration is a fixture that selects the catalogue's
//! 1.7B model and a profile built from that recording in a temporary directory.
#![cfg(feature = "qwentts")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_config::AppConfig;
use prxvc_media::Lang;
use prxvc_models::{Catalogue, ModelId, NotReady, ReadyFile, ReadyModel};
use prxvc_providers::{
    EnvCredentials, ModelSource, ProviderRegistry, VoiceChoice, VoiceDirs, test_provider,
};

const TTS: &str = "qwen3-tts-1.7b-base-q8";

fn bench_root() -> PathBuf {
    std::env::var_os("PRXVC_BENCH_ROOT").map_or_else(
        || PathBuf::from("/Users/nn/worker/voice/bench"),
        PathBuf::from,
    )
}

/// The bench models, standing in for the model manager.
struct BenchModels {
    catalogue: Arc<Catalogue>,
}

impl ModelSource for BenchModels {
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady> {
        let entry = self.catalogue.get(id).ok_or(NotReady::NoSelection)?;
        let dir = bench_root().join("tts/models");
        let files = [
            ("model", "qwen-talker-1.7b-base-Q8_0.gguf"),
            ("codec", "qwen-tokenizer-12hz-Q8_0.gguf"),
        ]
        .map(|(purpose, name)| ReadyFile {
            purpose: purpose.to_owned(),
            path: dir.join(name),
        });
        assert!(
            files.iter().all(|f| f.path.is_file()),
            "models not found in {}",
            dir.display()
        );
        Ok(ReadyModel {
            id: entry.id.clone(),
            revision_key: entry.revision_key(),
            role: entry.role,
            runtime: entry.runtime.name.clone(),
            runtime_params: serde_json::json!({}),
            files: files.to_vec(),
        })
    }
}

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1e3)
}

#[test]
#[ignore = "needs the TTS models, the owner's recording and the GPU"]
fn the_real_tts_provider_builds_and_speaks_in_the_owners_voice() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("voices/owner-live");
    fs::create_dir_all(&profile).unwrap();
    let reference = bench_root().join("tts/refs/en_owner.wav");
    fs::copy(&reference, profile.join("reference.wav")).expect("the owner's recording");
    fs::write(
        profile.join("profile.json"),
        r#"{"schema_version":1,"id":"owner-live","language":"en","x_vector_only":true}"#,
    )
    .unwrap();

    let mut cfg = AppConfig::default();
    cfg.voice.self_profile_id = "owner-live".into();
    cfg.call.self_language = "zh".into();
    cfg.call.peer_language = "en".into();
    cfg.providers.active.tts.insert("en".into(), TTS.into());
    let catalogue = Arc::new(Catalogue::embedded());
    let registry = ProviderRegistry::new(
        &cfg,
        Arc::clone(&catalogue),
        Arc::new(BenchModels { catalogue }),
        Arc::new(EnvCredentials::from_process()),
        VoiceDirs {
            profiles: tmp.path().join("voices"),
            presets: tmp.path().join("presets"),
        },
    );

    // The voice is not prepared yet: resolution says so, preparing fixes it.
    let problems = registry
        .resolve_tts(Lang::En, &VoiceChoice::OwnClone)
        .unwrap_err();
    println!("before preparing: {}", problems[0]);
    let t = Instant::now();
    let prepared = registry
        .prepare_cloned_voice(Lang::En)
        .expect("prepare the voice");
    println!(
        "prepare_cloned_voice (engine load, warm-up, voice): {} (cache hit: {})",
        ms(t.elapsed()),
        prepared.cache_hit
    );

    let spec = registry
        .resolve_tts(Lang::En, &VoiceChoice::OwnClone)
        .expect("resolves once prepared")
        .primary;
    let t = Instant::now();
    let provider = registry.build_tts(&spec).expect("build the provider");
    println!(
        "build_tts (engine load, warm-up, voice from cache): {}",
        ms(t.elapsed())
    );
    assert_eq!(registry.live_engines().tts, 1);

    for round in 1..=3 {
        let report = test_provider::test_synth(
            &*provider,
            Lang::En,
            spec.voice.selection(),
            Duration::from_secs(30),
        );
        println!(
            "test provider #{round}: ok={} first audio={} total={} ({})",
            report.ok,
            report.first_response.map_or_else(|| "-".into(), ms),
            ms(report.total),
            report.detail
        );
        assert!(report.ok, "{report:?}");
    }
    drop(provider);
    assert_eq!(
        registry.live_engines().tts,
        0,
        "the engine is freed with its last handle"
    );
}
