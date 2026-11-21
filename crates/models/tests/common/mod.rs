//! Shared test support: the fault server, catalogue builders and helpers.
#![allow(dead_code)]

pub mod server;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prxvc_models::catalogue::{Catalogue, FileSource, ModelEntry};
use prxvc_models::download::{BackoffPolicy, EngineSettings, Sleeper};
use sha2::{Digest, Sha256};

#[allow(unused_imports)]
pub use server::{Behaviour, FaultServer};

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Deterministic pseudo-random bytes.
pub fn blob(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect()
}

pub struct ModelSpec {
    pub id: String,
    pub role: &'static str,
    pub runtime: &'static str,
    pub files: Vec<(String, Vec<u8>)>,
}

impl ModelSpec {
    pub fn single(id: &str, size: usize) -> ModelSpec {
        ModelSpec {
            id: id.into(),
            role: "asr",
            runtime: "transcribe.cpp",
            files: vec![(
                format!("{id}.bin"),
                blob(size, id.len() as u64 + size as u64),
            )],
        }
    }

    pub fn multi(id: &str, sizes: &[usize]) -> ModelSpec {
        ModelSpec {
            id: id.into(),
            role: "tts",
            runtime: "qwentts.cpp",
            files: sizes
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        format!("{id}-{i}.bin"),
                        blob(*s, (i + 7) as u64 * *s as u64),
                    )
                })
                .collect(),
        }
    }
}

fn revision_of(id: &str) -> String {
    sha256_hex(id.as_bytes())[..40].to_string()
}

fn entry_json(spec: &ModelSpec) -> serde_json::Value {
    let lang = match spec.role {
        "vad" => serde_json::json!({"kind": "any"}),
        "mt" => serde_json::json!({"kind": "pair", "source": "zh", "target": "en"}),
        _ => serde_json::json!({"kind": "lang", "code": "en"}),
    };
    let files: Vec<_> = spec
        .files
        .iter()
        .enumerate()
        .map(|(i, (name, data))| {
            serde_json::json!({
                "name": name,
                "purpose": if i == 0 { "model".to_string() } else { format!("extra{i}") },
                "size": data.len(), "sha256": sha256_hex(data),
                "source": {"kind": "huggingface", "repo": format!("t/{}", spec.id),
                           "revision": revision_of(&spec.id), "path": name}
            })
        })
        .collect();
    serde_json::json!({
        "id": spec.id, "role": spec.role, "display_name": spec.id,
        "description": {"en": "test", "zh-CN": "test"}, "version_label": "t",
        "languages": [lang], "runtime": {"name": spec.runtime}, "files": files,
        "licence": {"code": "MIT", "weights": "MIT", "weights_url": "https://example.org"},
        "memory_mb": 1, "streaming": "native",
        "recommended_for": [{"badge": "recommended", "rank": 0}]
    })
}

pub fn catalogue_of(specs: &[ModelSpec]) -> Catalogue {
    let json = serde_json::json!({
        "schema_version": 1, "catalogue_version": 2026112001u64, "min_app_version": "0.1.0",
        "models": specs.iter().map(entry_json).collect::<Vec<_>>()
    });
    Catalogue::parse(&json.to_string()).expect("test catalogue is valid")
}

pub fn entry_of<'a>(cat: &'a Catalogue, id: &str) -> &'a ModelEntry {
    cat.get(&prxvc_models::ModelId::new(id))
        .expect("entry exists")
}

/// Path under which the fault server serves file `idx` of `entry`.
pub fn server_path(entry: &ModelEntry, idx: usize) -> String {
    match &entry.files[idx].source {
        FileSource::Huggingface {
            repo,
            revision,
            path,
            ..
        } => format!("/{repo}/resolve/{revision}/{path}"),
        FileSource::Url { .. } => unreachable!("tests use huggingface sources"),
    }
}

/// Register every file of every spec with the server.
pub fn serve(server: &FaultServer, cat: &Catalogue, specs: &[ModelSpec]) {
    for spec in specs {
        let entry = entry_of(cat, &spec.id);
        for (i, (_, data)) in spec.files.iter().enumerate() {
            server.add_file(&server_path(entry, i), data.clone());
        }
    }
}

pub type Delays = Arc<Mutex<Vec<Duration>>>;

/// A sleeper that records requested delays and returns at once.
pub fn recording_sleeper() -> (Sleeper, Delays) {
    let delays: Delays = Arc::default();
    let d2 = delays.clone();
    (Arc::new(move |d, _| d2.lock().unwrap().push(d)), delays)
}

/// Engine settings for tests: small pieces, plenty of "free space", recorded
/// backoff instead of sleeping.
pub fn test_settings(piece: u64) -> (EngineSettings, Delays) {
    let (sleeper, delays) = recording_sleeper();
    let settings = EngineSettings {
        sleeper,
        piece_size: piece,
        backoff: BackoffPolicy::default(),
        free_space: Some(Arc::new(|_: &Path| Ok(u64::MAX / 2))),
        ..EngineSettings::default()
    };
    (settings, delays)
}

pub fn wait_until(what: &str, timeout: Duration, mut cond: impl FnMut() -> bool) {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}
