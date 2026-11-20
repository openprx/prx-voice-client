//! A fixture catalogue, fake model store, fake loaders and a rig that wires them.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use prxvc_adapters::tts::qwentts::{
    CallControl, CallEnd, EngineError, QwenTts, QwenTtsConfig, SynthCall, SynthEngine,
};
use prxvc_config::AppConfig;
use prxvc_media::{Lang, ManualClock};
use prxvc_models::{Arch, Catalogue, HardwareInfo, ModelId, NotReady, Os, ReadyFile, ReadyModel};
use prxvc_ports::fakes::{
    AsrScript, FakeRefineAsr, FakeStreamingAsr, FakeVadProvider, RefineScript, VadScript,
};
use prxvc_ports::{RefineAsr, StreamingAsr, VadProvider};
use serde_json::{Value, json};

use crate::credentials::EnvCredentials;
use crate::fingerprint::model_fingerprint;
use crate::problem::BuildError;
use crate::recognition::RecognitionLoader;
use crate::registry::ProviderRegistry;
use crate::resolve::VoiceDirs;
use crate::spec::{LocalModelSpec, TtsSpec};
use crate::tts_loader::TtsLoader;

pub const TTS: &str = "qwen3-tts-1.7b-base-q8";
pub const OWNER: &str = "owner-1";

fn sha(c: char) -> String {
    c.to_string().repeat(64)
}

fn file(name: &str, purpose: &str, size: u64, c: char) -> Value {
    json!({"name": name, "purpose": purpose, "size": size, "sha256": sha(c),
           "source": {"kind": "url", "url": format!("https://example.invalid/{name}")}})
}

fn model(
    id: &str,
    role: &str,
    runtime: &str,
    streaming: &str,
    langs: Value,
    rank: Option<(&str, u32)>,
    files: Vec<Value>,
) -> Value {
    json!({
        "id": id, "role": role, "display_name": id,
        "description": {"en": "x", "zh-CN": "x"}, "version_label": "v",
        "languages": langs, "runtime": {"name": runtime}, "files": files,
        "licence": {"code": "MIT", "weights": "MIT", "weights_url": "https://example.invalid"},
        "memory_mb": 100, "streaming": streaming,
        "recommended_for": rank.map(|(b, r)| vec![json!({"badge": b, "rank": r})]).unwrap_or_default(),
    })
}

fn zh_en() -> Value {
    json!([{"kind": "lang", "code": "zh"}, {"kind": "lang", "code": "en"}])
}

/// The embedded catalogue with the recognition runtime of the end state (CT-11), a few models
/// that each fail one check, and voice presets.
pub fn fixture_catalogue(preset_size: u64) -> Catalogue {
    let mut value = serde_json::to_value(Catalogue::embedded()).unwrap();
    let models = value["models"].as_array_mut().unwrap();
    for m in models.iter_mut() {
        if m["id"] == "silero-vad-6.2" {
            m["runtime"] = json!({"name": "transcribe"});
        }
    }
    models.extend([
        model(
            "qwen3-asr-0.6b",
            "asr",
            "transcribe",
            "native",
            zh_en(),
            Some(("recommended", 0)),
            vec![file("asr06.gguf", "model", 10, 'a')],
        ),
        model(
            "qwen3-asr-1.7b",
            "asr",
            "transcribe",
            "segmented",
            zh_en(),
            Some(("smallest", 5)),
            vec![file("asr17.gguf", "model", 10, 'b')],
        ),
        model(
            "zh-only-asr",
            "asr",
            "transcribe",
            "native",
            json!([{"kind": "lang", "code": "zh"}]),
            None,
            vec![file("zhasr.gguf", "model", 10, 'c')],
        ),
        model(
            "future-asr",
            "asr",
            "future.cpp",
            "native",
            zh_en(),
            None,
            vec![file("future.gguf", "model", 10, 'd')],
        ),
        model(
            "batch-tts",
            "tts",
            "qwentts.cpp",
            "none",
            zh_en(),
            None,
            vec![
                file("bt-m.gguf", "model", 10, 'e'),
                file("bt-c.gguf", "codec", 10, 'f'),
            ],
        ),
    ]);
    value["presets"] = json!([
        {"id": "preset-en-1", "model_id": TTS, "language": "en", "display_name": "English 1",
         "file": file("ref.wav", "reference", preset_size, '1'), "licence": "CC0"},
        {"id": "preset-zh-1", "model_id": TTS, "language": "zh", "display_name": "Chinese 1",
         "file": file("ref.wav", "reference", preset_size, '2'), "licence": "CC0"},
    ]);
    // A template that does not stream, to prove its reason is carried through.
    value["providers"].as_array_mut().unwrap().push(json!({
        "id": "batch-mt", "display_name": "Batch MT", "protocol": "openai-chat", "roles": ["mt"],
        "default_base_url": "http://127.0.0.1:9/v1", "local_endpoint_default": true,
        "needs_credential": false, "known_models": [], "streaming": "none", "egress": "text",
        "ineligible_reason": {"en": "Waits for the whole sentence.", "zh-CN": "x"}
    }));
    Catalogue::parse(&value.to_string()).expect("the fixture catalogue is valid")
}

/// Model states by id; every catalogue model is ready unless a test says otherwise.
pub struct FakeModels {
    root: PathBuf,
    catalogue: Arc<Catalogue>,
    overrides: Mutex<HashMap<String, NotReady>>,
}

impl FakeModels {
    pub fn set(&self, id: &str, state: NotReady) {
        self.overrides.lock().insert(id.to_owned(), state);
    }
}

impl crate::model_source::ModelSource for FakeModels {
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady> {
        if let Some(state) = self.overrides.lock().get(id.as_str()) {
            return Err(state.clone());
        }
        let entry = self.catalogue.get(id).ok_or(NotReady::NoSelection)?;
        Ok(ReadyModel {
            id: entry.id.clone(),
            revision_key: entry.revision_key(),
            role: entry.role,
            runtime: entry.runtime.name.clone(),
            runtime_params: json!({}),
            files: entry
                .files
                .iter()
                .map(|f| ReadyFile {
                    purpose: f.purpose.clone(),
                    path: self.root.join(id.as_str()).join(&f.name),
                })
                .collect(),
        })
    }
}

/// A short voiced WAV: mono 16-bit at 24 kHz.
pub fn write_wav(path: &Path, seconds: f32) {
    let n = (24_000.0 * seconds) as usize;
    let data: Vec<u8> = (0..n)
        .flat_map(|i| (((i as f32 * 0.05).sin() * 8000.0) as i16).to_le_bytes())
        .collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24_000u32.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b).unwrap();
}

/// The scripted synthesis engine behind the real `QwenTts` adapter.
pub struct FakeEngine {
    pub fail_calls: bool,
    pub dropped: Arc<AtomicUsize>,
}

impl SynthEngine for FakeEngine {
    fn extract_voice(&self, _samples_24k: &[f32]) -> Result<Vec<f32>, EngineError> {
        Ok(vec![1.0; 8])
    }

    fn synthesize(
        &self,
        call: &SynthCall<'_>,
        ctl: &dyn CallControl,
    ) -> Result<CallEnd, EngineError> {
        if self.fail_calls && !call.text.starts_with("Hello, this is a short warm-up") {
            return Err(EngineError::Call {
                status: -1,
                message: "the engine broke".to_owned(),
            });
        }
        for _ in 0..4 {
            ctl.on_chunk(&[0.1; 2400]);
        }
        Ok(CallEnd::Completed)
    }
}

impl Drop for FakeEngine {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// Counts loads and drops of fake synthesis engines. With `fail_first`, the first engine fails
/// every real request (not its warm-up).
#[derive(Default)]
pub struct FakeTtsLoader {
    pub loads: AtomicUsize,
    pub dropped: Arc<AtomicUsize>,
    pub fail_first: AtomicBool,
    pub unavailable: AtomicBool,
}

impl TtsLoader for FakeTtsLoader {
    fn available(&self) -> bool {
        !self.unavailable.load(Ordering::SeqCst)
    }

    fn load(&self, spec: &TtsSpec) -> Result<QwenTts, BuildError> {
        let n = self.loads.fetch_add(1, Ordering::SeqCst);
        let engine = FakeEngine {
            fail_calls: n == 0 && self.fail_first.load(Ordering::SeqCst),
            dropped: Arc::clone(&self.dropped),
        };
        QwenTts::start(
            Arc::new(engine),
            QwenTtsConfig::new(spec.label.clone(), spec.fingerprint.clone()),
        )
        .map_err(|e| BuildError::Start {
            provider: spec.id.clone(),
            detail: e.to_string(),
        })
    }
}

/// Recognition engines from the ports' fakes, counting loads.
#[derive(Default)]
pub struct FakeRecognition {
    pub vad_loads: AtomicUsize,
    pub asr_loads: AtomicUsize,
    pub refine_loads: AtomicUsize,
}

impl RecognitionLoader for FakeRecognition {
    fn available(&self) -> bool {
        true
    }

    fn vad(&self, _spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError> {
        self.vad_loads.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(FakeVadProvider::new(VadScript::probabilities([
            0.0,
        ]))))
    }

    fn asr(&self, _spec: &LocalModelSpec) -> Result<Arc<dyn StreamingAsr>, BuildError> {
        self.asr_loads.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(FakeStreamingAsr::new(
            ManualClock::new(),
            AsrScript::new([], ""),
        )))
    }

    fn refine(&self, _spec: &LocalModelSpec) -> Result<Arc<dyn RefineAsr>, BuildError> {
        self.refine_loads.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(FakeRefineAsr::new(
            ManualClock::new(),
            RefineScript::new("", std::time::Duration::ZERO),
        )))
    }
}

/// Everything a test needs: a temp directory with a prepared owner profile and preset files, the
/// fixture catalogue, fake models, a mutable configuration and fake loaders.
pub struct Rig {
    /// Keeps the temporary directory alive for the rig's lifetime.
    #[allow(dead_code)]
    pub dir: tempfile::TempDir,
    pub cfg: AppConfig,
    pub catalogue: Arc<Catalogue>,
    pub models: Arc<FakeModels>,
    pub dirs: VoiceDirs,
    pub tts_loader: Arc<FakeTtsLoader>,
    pub recognition: Arc<FakeRecognition>,
    pub env: Arc<Mutex<HashMap<String, String>>>,
}

impl Rig {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let dirs = VoiceDirs {
            profiles: dir.path().join("voices"),
            presets: dir.path().join("presets"),
        };
        let preset_ref = dir.path().join("presets/preset-en-1/ref.wav");
        write_wav(&preset_ref, 1.5);
        let size = fs::metadata(&preset_ref).unwrap().len();
        fs::create_dir_all(dirs.presets.join("preset-zh-1")).unwrap();
        fs::copy(&preset_ref, dirs.presets.join("preset-zh-1/ref.wav")).unwrap();
        let catalogue = Arc::new(fixture_catalogue(size));
        let models = Arc::new(FakeModels {
            root: dir.path().join("models"),
            catalogue: Arc::clone(&catalogue),
            overrides: Mutex::new(HashMap::new()),
        });
        let mut cfg = AppConfig::default();
        cfg.voice.self_profile_id = OWNER.to_owned();
        // The owner's interface is Chinese: the call languages derive to zh (own) and en (peer).
        cfg.ui.language = "zh-CN".to_owned();
        let rig = Self {
            dir,
            cfg,
            catalogue,
            models,
            dirs,
            tts_loader: Arc::new(FakeTtsLoader::default()),
            recognition: Arc::new(FakeRecognition::default()),
            env: Arc::new(Mutex::new(HashMap::new())),
        };
        rig.write_profile(OWNER, true);
        rig
    }

    /// The owner's profile: its files, and a voice prepared for the TTS model when `prepared`.
    pub fn write_profile(&self, id: &str, prepared: bool) {
        let dir = self.dirs.profiles.join(id);
        write_wav(&dir.join("reference.wav"), 3.0);
        fs::write(
            dir.join("profile.json"),
            json!({"schema_version": 1, "id": id, "language": "en", "transcript": "hello there"})
                .to_string(),
        )
        .unwrap();
        if prepared {
            self.prepare_profile(id, &self.fingerprint());
        }
    }

    /// A derived entry for `fingerprint` in the profile.
    pub fn prepare_profile(&self, id: &str, fingerprint: &str) {
        use prxvc_adapters::tts::qwentts::{CacheKey, VoiceCache};
        VoiceCache::new(self.dirs.profiles.join(id))
            .store(
                &CacheKey {
                    model_hash: fingerprint.to_owned(),
                    reference_sha256: "0".repeat(64),
                },
                &[1.0; 8],
            )
            .unwrap();
    }

    pub fn fingerprint(&self) -> String {
        model_fingerprint(self.catalogue.get(&ModelId::new(TTS)).unwrap())
    }

    pub fn registry(&self) -> ProviderRegistry {
        let env = Arc::clone(&self.env);
        ProviderRegistry::new(
            &self.cfg,
            Arc::clone(&self.catalogue),
            Arc::clone(&self.models) as _,
            Arc::new(EnvCredentials::with_lookup(move |v| {
                env.lock().get(v).cloned()
            })),
            self.dirs.clone(),
        )
        .with_hardware(HardwareInfo::new(32, Os::Macos, Arch::Aarch64))
        .with_system_language(Some(Lang::Zh))
        .with_tts_loader(Arc::clone(&self.tts_loader) as _)
        .with_recognition_loader(Arc::clone(&self.recognition) as _)
    }

    /// An OpenAI-style entry with a credential in the fake environment.
    pub fn openai_entry(&mut self, id: &str, base_url: &str) {
        self.env
            .lock()
            .insert("OPENAI_KEY".to_owned(), "sk-test-secret".to_owned());
        let entry = toml_entry(&format!(
            "id = \"{id}\"\ntemplate = \"openai\"\nprotocol = \"openai-chat\"\nbase_url = \"{base_url}\"\n\
             model = \"gpt-4.1-mini\"\ncredential = \"env:OPENAI_KEY\"\nlocal_endpoint = false\n"
        ));
        self.cfg.providers.entries.push(entry);
    }
}

/// A provider entry from TOML text, parsed by the configuration crate.
pub fn toml_entry(text: &str) -> prxvc_config::ProviderEntry {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.toml"),
        format!("[[providers.entries]]\n{text}"),
    )
    .unwrap();
    let config = prxvc_config::ConfigManager::new(dir.path().to_path_buf())
        .load_report()
        .unwrap()
        .config;
    config.providers.entries[0].clone()
}
