//! Loading the in-process synthesis engine, and putting a voice on it.

use prxvc_adapters::tts::qwentts::{QwenTts, VoiceCache, load_reference_wav};
use prxvc_ports::SpeechSynth;

use crate::problem::BuildError;
use crate::spec::{TtsSpec, VoicePlan};

/// Loads the synthesis engine of a spec. One call loads one engine, with its warm-up; the
/// registry shares the result between the directions and calls this only when no live engine
/// exists.
pub trait TtsLoader: Send + Sync {
    /// Whether `qwentts.cpp` can be loaded by this loader.
    fn available(&self) -> bool;
    /// Load the engine of `spec` (blocks for the model load and the warm-up).
    fn load(&self, spec: &TtsSpec) -> Result<QwenTts, BuildError>;
}

/// The loader of this binary: the native engine with the cargo feature `qwentts`, otherwise a
/// typed "not built in" error.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltInTts;

impl TtsLoader for BuiltInTts {
    fn available(&self) -> bool {
        cfg!(feature = "qwentts")
    }

    #[cfg(feature = "qwentts")]
    fn load(&self, spec: &TtsSpec) -> Result<QwenTts, BuildError> {
        use prxvc_adapters::tts::qwentts::QwenTtsConfig;
        let cfg = QwenTtsConfig::new(spec.label.clone(), spec.fingerprint.clone());
        QwenTts::load_native(&spec.files.talker, &spec.files.codec, cfg).map_err(|e| {
            BuildError::Start {
                provider: spec.id.clone(),
                detail: e.to_string(),
            }
        })
    }

    #[cfg(not(feature = "qwentts"))]
    fn load(&self, spec: &TtsSpec) -> Result<QwenTts, BuildError> {
        Err(BuildError::NotBuiltIn {
            provider: spec.id.clone(),
            runtime: spec.runtime.name().to_owned(),
        })
    }
}

/// Prepare and register the voice of `spec` on `engine`, through the profile's (or preset's)
/// on-disk cache: a prepared voice costs no engine call.
/// Returns whether the cache served a cloned voice (`None` for a preset).
pub(crate) fn register_voice(engine: &QwenTts, spec: &TtsSpec) -> Result<Option<bool>, BuildError> {
    let voice_err = |detail: String| BuildError::Voice {
        provider: spec.id.clone(),
        detail,
    };
    match &spec.voice {
        VoicePlan::Cloned {
            profile_id,
            profile_dir,
            reference,
            transcript,
            lang,
        } => {
            let reference = load_reference_wav(reference, profile_id, *lang, transcript.clone())
                .map_err(|e| voice_err(e.to_string()))?;
            engine
                .prepare_voice_cached(&VoiceCache::new(profile_dir), &reference)
                .map(|(_, hit)| Some(hit))
                .map_err(|e| voice_err(e.to_string()))
        }
        VoicePlan::Preset {
            name,
            dir,
            reference,
            lang,
        } => {
            let reference = load_reference_wav(reference, &format!("preset-{name}"), *lang, None)
                .map_err(|e| voice_err(e.to_string()))?;
            engine
                .register_preset(name, &reference, Some(&VoiceCache::new(dir)))
                .map(|_| None)
                .map_err(|e| voice_err(e.to_string()))
        }
    }
}

/// Check what the loaded engine says about itself against the spec.
pub(crate) fn check_capabilities(engine: &QwenTts, spec: &TtsSpec) -> Result<(), BuildError> {
    let d = engine.descriptor();
    d.require_native()?;
    d.require_lang(spec.lang)?;
    Ok(())
}
