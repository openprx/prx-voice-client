//! Voices: preparing one from a reference recording, and the on-disk cache that keeps the result.
//!
//! A prepared voice is the talker's speaker embedding (speaker-embedding mode, no reference
//! transcript; voice-cloning section 0.1). It is specific to the talker model: a voice prepared
//! for the 1.7B model has a different length than one for the 0.6B model and is useless to it, so
//! the cache is keyed by a hash of the model files and a hash of the reference recording.
//!
//! # Cache layout
//!
//! The cache lives under the voice profile directory the caller passes in (the application's
//! `voices_dir/<profile_id>`; voice-cloning section 5.3):
//!
//! ```text
//! <profile dir>/derived/<engine>-<model_hash>/spk.bin    raw little-endian f32 embedding
//! <profile dir>/derived/<engine>-<model_hash>/meta.json  schema_version, engine, model_hash,
//!                                                         reference_sha256, dim
//! ```
//!
//! A different model gives a different directory, so each loaded model keeps its own entry. A
//! changed reference leaves `meta.json` with a stale `reference_sha256`: the entry is a miss and
//! is overwritten by the next `store`. An entry that fails to parse or whose sizes disagree is
//! renamed aside (`*.bad`), never overwritten, and counts as a miss (CS-2.8.4). Writes are atomic:
//! a temporary file, then a rename (CS-2.8.5).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prxvc_audio::resample::MonoResampler;
use prxvc_media::{Lang, SampleRate};
use prxvc_ports::{PreparedVoice, TtsError, VoiceReference};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::model_hash::hex;
use super::wav::{self, WavError};

/// The engine name carried by prepared voices and used in cache directory names.
pub const ENGINE_ID: &str = "qwentts.cpp";
/// Name of the blob that holds the speaker embedding in a [`PreparedVoice`].
pub const SPK_BLOB: &str = "spk";
/// Output and reference rate of the engine.
pub const ENGINE_RATE: SampleRate = SampleRate::HZ_24000;
/// Shortest reference accepted, in seconds. The design asks for 6 to 12 s; the engine itself
/// works with less, so only a recording too short to carry a voice is refused here.
pub const MIN_REFERENCE_SECS: f64 = 1.0;
/// Longest reference accepted, in seconds.
pub const MAX_REFERENCE_SECS: f64 = 60.0;
/// Version of `meta.json`.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Encode an embedding as little-endian `f32` bytes (the content of `spk.bin` and of the `spk` blob).
pub fn encode_embedding(spk: &[f32]) -> Vec<u8> {
    spk.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Decode an embedding from little-endian `f32` bytes; `None` when the length is not a multiple
/// of four, the data is empty or a value is not finite.
pub fn decode_embedding(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let values: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    values.iter().all(|v| v.is_finite()).then_some(values)
}

/// The handle a prepared voice is known by: the model it was prepared for and the profile.
pub fn handle_of(model_hash: &str, profile_id: &str) -> String {
    let short: String = model_hash.chars().take(12).collect();
    format!("{ENGINE_ID}:{short}:{profile_id}")
}

/// Build the [`PreparedVoice`] of an embedding.
pub fn prepared_voice(profile_id: &str, model_hash: &str, spk: &[f32]) -> PreparedVoice {
    PreparedVoice {
        handle: handle_of(model_hash, profile_id),
        engine: ENGINE_ID.to_owned(),
        model_hash: model_hash.to_owned(),
        blobs: vec![(SPK_BLOB.to_owned(), encode_embedding(spk))],
    }
}

/// The embedding inside a prepared voice, when it was prepared by this engine for `model_hash`.
pub fn embedding_of(voice: &PreparedVoice, model_hash: &str) -> Result<Vec<f32>, TtsError> {
    if voice.engine != ENGINE_ID {
        return Err(TtsError::Internal(format!(
            "voice was prepared by `{}`, not `{ENGINE_ID}`",
            voice.engine
        )));
    }
    if voice.model_hash != model_hash {
        return Err(TtsError::Internal(
            "voice was prepared for a different model and must be prepared again".to_owned(),
        ));
    }
    voice
        .blobs
        .iter()
        .find(|(name, _)| name == SPK_BLOB)
        .and_then(|(_, bytes)| decode_embedding(bytes))
        .ok_or_else(|| {
            TtsError::Internal("prepared voice has no valid speaker embedding".to_owned())
        })
}

/// SHA-256 (hex) of a reference recording: its sample rate and its samples, so the hash does not
/// depend on the container the recording came in.
pub fn reference_hash(reference: &VoiceReference) -> String {
    let mut hasher = Sha256::new();
    hasher.update(reference.rate.hz().to_le_bytes());
    for s in &reference.samples {
        hasher.update(s.to_le_bytes());
    }
    hex(&hasher.finalize())
}

/// The reference as the engine needs it: mono at 24 kHz, validated.
///
/// Resampling uses the workspace resampler (`prxvc_audio::resample::MonoResampler`).
pub fn reference_to_engine_rate(reference: &VoiceReference) -> Result<Vec<f32>, TtsError> {
    let secs = reference
        .rate
        .duration_of(reference.samples.len())
        .as_secs_f64();
    if secs < MIN_REFERENCE_SECS {
        return Err(TtsError::Internal(format!(
            "reference recording is too short ({secs:.2} s, at least {MIN_REFERENCE_SECS} s)"
        )));
    }
    if secs > MAX_REFERENCE_SECS {
        return Err(TtsError::Internal(format!(
            "reference recording is too long ({secs:.1} s, at most {MAX_REFERENCE_SECS} s)"
        )));
    }
    if reference.samples.iter().any(|s| !s.is_finite()) {
        return Err(TtsError::Internal(
            "reference recording holds a sample that is not finite".to_owned(),
        ));
    }
    if reference.rate == ENGINE_RATE {
        return Ok(reference.samples.clone());
    }
    let mut resampler = MonoResampler::new(reference.rate.hz(), ENGINE_RATE.hz())
        .map_err(|e| TtsError::Internal(format!("resampler: {e}")))?;
    resampler
        .convert_block(&reference.samples)
        .map_err(|e| TtsError::Internal(format!("resampling the reference: {e}")))
}

/// Read a reference recording from a WAV file (a bundled preset reference, a profile's
/// `reference.wav`).
pub fn load_reference_wav(
    path: &Path,
    profile_id: &str,
    lang: Lang,
    transcript: Option<String>,
) -> Result<VoiceReference, TtsError> {
    let decoded = wav::read(path).map_err(|e: WavError| TtsError::Internal(e.to_string()))?;
    let rate = SampleRate::new(decoded.sample_rate)
        .map_err(|e| TtsError::Internal(format!("{}: {e}", path.display())))?;
    Ok(VoiceReference {
        profile_id: profile_id.to_owned(),
        samples: decoded.samples,
        rate,
        transcript,
        lang,
    })
}

/// What identifies a cache entry's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheKey {
    /// Hash of the model files the voice is prepared for.
    pub model_hash: String,
    /// Hash of the reference recording ([`reference_hash`]).
    pub reference_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    schema_version: u32,
    engine: String,
    model_hash: String,
    reference_sha256: String,
    dim: usize,
}

/// A cache operation failed.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The model hash is not usable in a directory name.
    #[error("model hash `{0}` is not a plain identifier")]
    BadModelHash(String),
    /// File system failure.
    #[error("voice cache: {0}")]
    Io(#[from] io::Error),
}

/// The derived-data cache of one voice profile (or preset).
#[derive(Clone, Debug)]
pub struct VoiceCache {
    derived: PathBuf,
}

impl VoiceCache {
    /// A cache under `profile_dir`; nothing is created until [`store`](Self::store).
    pub fn new(profile_dir: impl AsRef<Path>) -> Self {
        Self {
            derived: profile_dir.as_ref().join("derived"),
        }
    }

    /// The directory of the entry for `model_hash`.
    pub fn entry_dir(&self, model_hash: &str) -> Result<PathBuf, CacheError> {
        let plain = !model_hash.is_empty()
            && model_hash
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !plain {
            return Err(CacheError::BadModelHash(model_hash.to_owned()));
        }
        Ok(self.derived.join(format!("{ENGINE_ID}-{model_hash}")))
    }

    /// The cached embedding for `key`, or `None` on a miss: no entry, an entry for another
    /// reference, or an entry that is damaged (renamed aside).
    pub fn lookup(&self, key: &CacheKey) -> Result<Option<Vec<f32>>, CacheError> {
        let dir = self.entry_dir(&key.model_hash)?;
        let (meta_path, spk_path) = (dir.join("meta.json"), dir.join("spk.bin"));
        let (meta_bytes, spk_bytes) = match (fs::read(&meta_path), fs::read(&spk_path)) {
            (Ok(m), Ok(s)) => (m, s),
            (Err(e), _) | (_, Err(e)) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            (Err(e), _) | (_, Err(e)) => return Err(e.into()),
        };
        let parsed = serde_json::from_slice::<CacheMeta>(&meta_bytes).ok();
        let embedding = decode_embedding(&spk_bytes);
        match (parsed, embedding) {
            (Some(meta), Some(spk))
                if meta.schema_version == CACHE_SCHEMA_VERSION
                    && meta.engine == ENGINE_ID
                    && meta.model_hash == key.model_hash
                    && meta.dim == spk.len() =>
            {
                Ok((meta.reference_sha256 == key.reference_sha256).then_some(spk))
            }
            _ => {
                tracing::warn!(dir = %dir.display(), "voice cache entry is damaged; setting it aside");
                for path in [&meta_path, &spk_path] {
                    let mut aside = path.clone().into_os_string();
                    aside.push(".bad");
                    fs::rename(path, aside)?;
                }
                Ok(None)
            }
        }
    }

    /// Store `spk` for `key`, replacing the entry of the same model.
    pub fn store(&self, key: &CacheKey, spk: &[f32]) -> Result<(), CacheError> {
        let dir = self.entry_dir(&key.model_hash)?;
        fs::create_dir_all(&dir)?;
        let meta = CacheMeta {
            schema_version: CACHE_SCHEMA_VERSION,
            engine: ENGINE_ID.to_owned(),
            model_hash: key.model_hash.clone(),
            reference_sha256: key.reference_sha256.clone(),
            dim: spk.len(),
        };
        let meta_json = serde_json::to_vec_pretty(&meta).map_err(io::Error::other)?;
        // The embedding first: an entry whose `meta.json` is missing is a miss, one whose
        // `meta.json` is new always has its embedding.
        write_atomic(&dir.join("spk.bin"), &encode_embedding(spk))?;
        write_atomic(&dir.join("meta.json"), &meta_json)?;
        Ok(())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        // The rename failed: the temporary copy is useless. Failing to remove it changes nothing
        // the user sees, and the rename error is the one returned.
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(model: &str, reference: &str) -> CacheKey {
        CacheKey {
            model_hash: model.to_owned(),
            reference_sha256: reference.to_owned(),
        }
    }

    fn reference(rate: u32, secs: f32) -> VoiceReference {
        let rate = SampleRate::new(rate).expect("rate");
        VoiceReference {
            profile_id: "me".to_owned(),
            samples: (0..rate.samples_in(std::time::Duration::from_secs_f32(secs)))
                .map(|i| (i as f32 * 0.01).sin() * 0.3)
                .collect(),
            rate,
            transcript: None,
            lang: Lang::En,
        }
    }

    #[test]
    fn embeddings_round_trip_and_garbage_is_refused() {
        let spk = vec![0.5, -1.25, 3.0];
        assert_eq!(decode_embedding(&encode_embedding(&spk)), Some(spk));
        assert_eq!(decode_embedding(&[]), None);
        assert_eq!(decode_embedding(&[1, 2, 3]), None);
        assert_eq!(decode_embedding(&f32::NAN.to_le_bytes()), None);
    }

    #[test]
    fn a_prepared_voice_is_bound_to_its_model() {
        let v = prepared_voice("me", "abcdef0123456789", &[1.0, 2.0]);
        assert_eq!(v.engine, ENGINE_ID);
        assert_eq!(v.handle, "qwentts.cpp:abcdef012345:me");
        assert_eq!(
            embedding_of(&v, "abcdef0123456789").expect("spk"),
            vec![1.0, 2.0]
        );
        assert!(embedding_of(&v, "other-model").is_err());
        let mut foreign = v.clone();
        foreign.engine = "someone-else".to_owned();
        assert!(embedding_of(&foreign, "abcdef0123456789").is_err());
        let mut empty = v;
        empty.blobs.clear();
        assert!(embedding_of(&empty, "abcdef0123456789").is_err());
    }

    #[test]
    fn the_reference_hash_follows_samples_and_rate() {
        let a = reference(24_000, 2.0);
        let mut b = a.clone();
        assert_eq!(reference_hash(&a), reference_hash(&b));
        b.samples[10] += 0.001;
        assert_ne!(reference_hash(&a), reference_hash(&b));
        let mut c = a.clone();
        c.rate = SampleRate::HZ_16000;
        assert_ne!(reference_hash(&a), reference_hash(&c));
        assert_eq!(reference_hash(&a).len(), 64);
    }

    #[test]
    fn references_are_resampled_to_the_engine_rate_and_validated() {
        let same = reference(24_000, 2.0);
        assert_eq!(reference_to_engine_rate(&same).expect("same").len(), 48_000);
        let r48 = reference(48_000, 2.0);
        let out = reference_to_engine_rate(&r48).expect("resampled");
        assert_eq!(out.len(), 48_000, "two seconds stay two seconds at 24 kHz");
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(
            reference_to_engine_rate(&reference(24_000, 0.5)).is_err(),
            "too short"
        );
        assert!(
            reference_to_engine_rate(&reference(8_000, 61.0)).is_err(),
            "too long"
        );
        let mut nan = same;
        nan.samples[3] = f32::NAN;
        assert!(reference_to_engine_rate(&nan).is_err());
    }

    #[test]
    fn a_stored_voice_is_found_again_and_a_hit_needs_both_hashes_to_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = VoiceCache::new(dir.path());
        let k = key("m1", "r1");
        assert_eq!(cache.lookup(&k).expect("lookup"), None, "empty cache");
        cache.store(&k, &[0.25, 0.5]).expect("store");
        assert_eq!(cache.lookup(&k).expect("lookup"), Some(vec![0.25, 0.5]));
        assert!(
            dir.path().join("derived/qwentts.cpp-m1/spk.bin").is_file(),
            "layout of voice-cloning section 5.3"
        );
        assert_eq!(
            cache.lookup(&key("m1", "r2")).expect("lookup"),
            None,
            "reference changed"
        );
        assert_eq!(
            cache.lookup(&key("m2", "r1")).expect("lookup"),
            None,
            "model changed"
        );
    }

    #[test]
    fn each_model_keeps_its_own_entry_and_a_new_reference_replaces_the_old_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = VoiceCache::new(dir.path());
        cache.store(&key("big", "r1"), &[1.0; 4]).expect("store");
        cache.store(&key("small", "r1"), &[2.0; 2]).expect("store");
        assert_eq!(
            cache.lookup(&key("big", "r1")).expect("l"),
            Some(vec![1.0; 4])
        );
        assert_eq!(
            cache.lookup(&key("small", "r1")).expect("l"),
            Some(vec![2.0; 2])
        );
        cache.store(&key("big", "r2"), &[3.0; 4]).expect("store");
        assert_eq!(cache.lookup(&key("big", "r1")).expect("l"), None);
        assert_eq!(
            cache.lookup(&key("big", "r2")).expect("l"),
            Some(vec![3.0; 4])
        );
        assert_eq!(
            cache.lookup(&key("small", "r1")).expect("l"),
            Some(vec![2.0; 2])
        );
    }

    #[test]
    fn a_damaged_entry_is_set_aside_and_is_a_miss() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = VoiceCache::new(dir.path());
        let k = key("m", "r");
        cache.store(&k, &[1.0, 2.0]).expect("store");
        let entry = cache.entry_dir("m").expect("dir");
        fs::write(entry.join("spk.bin"), [1u8, 2, 3]).expect("corrupt");
        assert_eq!(cache.lookup(&k).expect("lookup"), None);
        assert!(
            entry.join("spk.bin.bad").is_file(),
            "kept aside, not overwritten"
        );
        assert!(entry.join("meta.json.bad").is_file());
        cache.store(&k, &[1.0, 2.0]).expect("store again");
        assert_eq!(cache.lookup(&k).expect("lookup"), Some(vec![1.0, 2.0]));
    }

    #[test]
    fn a_size_mismatch_between_meta_and_embedding_is_damage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = VoiceCache::new(dir.path());
        let k = key("m", "r");
        cache.store(&k, &[1.0, 2.0]).expect("store");
        let entry = cache.entry_dir("m").expect("dir");
        fs::write(entry.join("spk.bin"), encode_embedding(&[1.0, 2.0, 3.0])).expect("write");
        assert_eq!(cache.lookup(&k).expect("lookup"), None);
    }

    #[test]
    fn a_model_hash_that_is_not_an_identifier_is_refused() {
        let cache = VoiceCache::new("/nonexistent");
        assert!(matches!(
            cache.entry_dir("../x"),
            Err(CacheError::BadModelHash(_))
        ));
        assert!(matches!(
            cache.entry_dir(""),
            Err(CacheError::BadModelHash(_))
        ));
    }

    #[test]
    fn a_wav_file_becomes_a_reference() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ref.wav");
        let pcm: Vec<i16> = (0..32_000).map(|i| ((i % 100) as i16) * 100).collect();
        fs::write(
            &path,
            super::super::wav::tests_support::pcm16_file(16_000, 1, &pcm),
        )
        .expect("write");
        let r =
            load_reference_wav(&path, "preset-a", Lang::En, Some("text".to_owned())).expect("ref");
        assert_eq!(
            (r.rate, r.samples.len(), r.profile_id.as_str()),
            (SampleRate::HZ_16000, 32_000, "preset-a")
        );
        assert!(load_reference_wav(&dir.path().join("none.wav"), "x", Lang::En, None).is_err());
    }
}
