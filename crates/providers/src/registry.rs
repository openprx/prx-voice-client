//! The provider registry: the one place that constructs a provider from configuration
//! (CS-2.2.1).
//!
//! Resolution (`resolve_*`) turns configuration, catalogue, model store, credentials and the
//! network policy into typed specs and reports every problem; construction (`build_*`) turns a
//! spec into a boxed port handle over an engine shared between the directions. Neither falls back
//! to a mock or another provider: whatever cannot be served is an error value.

use std::sync::Arc;

use parking_lot::RwLock;
use prxvc_adapters::asr::AsrParams;
use prxvc_adapters::dialogue::{DialogueConfig, OpenAiChatDialogue};
use prxvc_adapters::mt::{OpenAiChatConfig, OpenAiChatMt};
use prxvc_adapters::tts::qwentts::QwenTts;
use prxvc_config::{AppConfig, NetworkPolicy};
use prxvc_media::Lang;
use prxvc_models::{Catalogue, HardwareInfo};
use prxvc_ports::{
    DialogueBackend, RefineAsr, SpeechSynth, StreamingAsr, Translator, VadProvider,
};
use sha2::{Digest, Sha256};

use crate::credentials::CredentialReader;
use crate::egress::{EgressSummary, summarize};
use crate::engines::{Engines, Lease};
use crate::handles::{
    SharedAsr, SharedDialogue, SharedRefine, SharedSynth, SharedTranslator, SharedVad,
};
use crate::keys::CallLanguages;
use crate::model_source::ModelSource;
use crate::plan::CallPlan;
use crate::problem::{BuildError, Problem};
use crate::recognition::{BuiltInRecognition, RecognitionLoader, asr_params};
use crate::resolve::{DialogueChoice, Resolver, VoiceChoice, VoiceDirs};
use crate::runtime::BuiltRuntimes;
use crate::spec::{DialogueSpec, LocalModelSpec, MtSpec, Resolved, TtsSpec, VoicePlan};
use crate::tts_loader::{BuiltInTts, TtsLoader, check_capabilities, register_voice};

/// The result of preparing the owner's voice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoicePrepared {
    /// The profile.
    pub profile_id: String,
    /// Whether the profile's cache already held the voice for this model and recording.
    pub cache_hit: bool,
    /// How long it took, the engine load included when the engine was not yet loaded.
    pub elapsed: std::time::Duration,
}

/// Preparing the owner's voice failed.
#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    /// The model, runtime or profile cannot be resolved.
    #[error("the voice cannot be prepared: {} problem(s)", .0.len())]
    Problems(Vec<Problem>),
    /// The engine could not be loaded or the voice could not be derived.
    #[error(transparent)]
    Build(#[from] BuildError),
}

/// The engines that are alive right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveEngines {
    /// Synthesis engines.
    pub tts: usize,
    /// Translation clients.
    pub mt: usize,
    /// Streaming recognition engines.
    pub asr: usize,
    /// Refine-pass recognition engines.
    pub refine: usize,
    /// Voice activity detection engines.
    pub vad: usize,
    /// Dialogue clients.
    pub dialogue: usize,
}

struct Snapshot {
    cfg: Arc<AppConfig>,
    policy: NetworkPolicy,
}

/// The provider registry. Cheap to share behind an `Arc`; every method takes `&self`.
pub struct ProviderRegistry {
    state: RwLock<Snapshot>,
    catalogue: Arc<Catalogue>,
    models: Arc<dyn ModelSource>,
    creds: Arc<dyn CredentialReader>,
    dirs: VoiceDirs,
    hardware: HardwareInfo,
    system_language: Option<Lang>,
    tts_loader: Arc<dyn TtsLoader>,
    recognition: Arc<dyn RecognitionLoader>,
    tts: Engines<(String, String), QwenTts>,
    mt: Engines<String, OpenAiChatMt>,
    dialogue: Engines<String, OpenAiChatDialogue>,
    asr: Engines<String, Arc<dyn StreamingAsr>>,
    refine: Engines<String, Arc<dyn RefineAsr>>,
    vad: Engines<String, Arc<dyn VadProvider>>,
}

impl ProviderRegistry {
    /// A registry over `config`, the catalogue, the model manager and the credential reader.
    /// `dirs` locates voice profiles and presets.
    pub fn new(
        config: &AppConfig,
        catalogue: Arc<Catalogue>,
        models: Arc<dyn ModelSource>,
        creds: Arc<dyn CredentialReader>,
        dirs: VoiceDirs,
    ) -> Self {
        Self {
            state: RwLock::new(Snapshot {
                policy: NetworkPolicy::from_config(config),
                cfg: Arc::new(config.clone()),
            }),
            catalogue,
            models,
            creds,
            dirs,
            hardware: HardwareInfo::detect(),
            system_language: None,
            tts_loader: Arc::new(BuiltInTts),
            recognition: Arc::new(BuiltInRecognition),
            tts: Engines::default(),
            mt: Engines::default(),
            dialogue: Engines::default(),
            asr: Engines::default(),
            refine: Engines::default(),
            vad: Engines::default(),
        }
    }

    /// Use `hardware` for the catalogue's recommendations instead of the detected machine.
    #[must_use]
    pub fn with_hardware(mut self, hardware: HardwareInfo) -> Self {
        self.hardware = hardware;
        self
    }

    /// The language of the operating system, used when no call language is configured.
    #[must_use]
    pub fn with_system_language(mut self, lang: Option<Lang>) -> Self {
        self.system_language = lang;
        self
    }

    /// Replace the synthesis loader (the composition root, or a test).
    #[must_use]
    pub fn with_tts_loader(mut self, loader: Arc<dyn TtsLoader>) -> Self {
        self.tts_loader = loader;
        self
    }

    /// Replace the recognition loader (the composition root, or a test).
    #[must_use]
    pub fn with_recognition_loader(mut self, loader: Arc<dyn RecognitionLoader>) -> Self {
        self.recognition = loader;
        self
    }

    /// Take a new configuration (after each settings update). Engines already built are kept by
    /// the handles that hold them; the next `build_*` follows the new configuration.
    pub fn reload(&self, config: &AppConfig) {
        *self.state.write() = Snapshot {
            policy: NetworkPolicy::from_config(config),
            cfg: Arc::new(config.clone()),
        };
    }

    fn built(&self) -> BuiltRuntimes {
        BuiltRuntimes {
            qwentts: self.tts_loader.available(),
            transcribe: self.recognition.available(),
        }
    }

    fn with_resolver<R>(&self, f: impl FnOnce(&Resolver<'_>) -> R) -> R {
        self.with_resolver_opts(true, f)
    }

    fn with_resolver_opts<R>(
        &self,
        require_prepared: bool,
        f: impl FnOnce(&Resolver<'_>) -> R,
    ) -> R {
        let state = self.state.read();
        let built = self.built();
        let resolver = Resolver {
            cfg: &state.cfg,
            catalogue: &self.catalogue,
            models: self.models.as_ref(),
            creds: self.creds.as_ref(),
            policy: &state.policy,
            hardware: &self.hardware,
            dirs: &self.dirs,
            built: &built,
            require_prepared,
        };
        f(&resolver)
    }

    // ---- resolution ------------------------------------------------------------------------

    /// The two call languages from the configuration.
    pub fn call_languages(&self) -> Result<CallLanguages, Vec<Problem>> {
        self.with_resolver(|r| r.languages(self.system_language))
    }

    /// Resolve the voice activity detector.
    pub fn resolve_vad(&self) -> Result<Resolved<LocalModelSpec>, Vec<Problem>> {
        self.with_resolver(|r| r.vad())
    }

    /// Resolve streaming recognition of `lang`.
    pub fn resolve_asr(&self, lang: Lang) -> Result<Resolved<LocalModelSpec>, Vec<Problem>> {
        self.with_resolver(|r| r.asr(lang))
    }

    /// Resolve the refine pass for `lang`.
    pub fn resolve_asr_refine(&self, lang: Lang) -> Result<Resolved<LocalModelSpec>, Vec<Problem>> {
        self.with_resolver(|r| r.refine(lang))
    }

    /// Resolve translation from `source` to `target`.
    pub fn resolve_mt(&self, source: Lang, target: Lang) -> Result<Resolved<MtSpec>, Vec<Problem>> {
        self.with_resolver(|r| r.mt(source, target))
    }

    /// Resolve the assistant's dialogue backend (`providers.active.dialogue`; the built-in local
    /// server when nothing is selected). The OpenPRX placeholder resolves to a typed problem.
    pub fn resolve_dialogue(&self) -> Result<Resolved<DialogueSpec>, Vec<Problem>> {
        self.with_resolver(|r| r.dialogue())
    }

    /// The backends the settings can offer for the dialogue.
    pub fn dialogue_choices(&self) -> Vec<DialogueChoice> {
        self.with_resolver(|r| r.dialogue_choices())
    }

    /// Resolve synthesis of `lang` with `voice`.
    pub fn resolve_tts(
        &self,
        lang: Lang,
        voice: &VoiceChoice,
    ) -> Result<Resolved<TtsSpec>, Vec<Problem>> {
        self.with_resolver(|r| r.tts(lang, voice))
    }

    /// Resolve everything a call needs, for both directions: the full set, or every problem.
    pub fn resolve_call(&self) -> Result<CallPlan, Vec<Problem>> {
        self.with_resolver(|r| r.call(self.system_language))
    }

    /// Resolve the roles of the given directions only (a call that translates one direction
    /// needs nothing of the other): the full set, or every problem.
    pub fn resolve_directions(
        &self,
        directions: &[prxvc_media::Direction],
    ) -> Result<crate::plan::DirectionsPlan, Vec<Problem>> {
        self.with_resolver(|r| r.directions(self.system_language, directions))
    }

    /// What leaves the machine under `plan`.
    pub fn egress_summary(&self, plan: &CallPlan) -> EgressSummary {
        summarize(plan)
    }

    // ---- construction ----------------------------------------------------------------------

    /// Build the voice activity detector of `spec`. The engine is shared by the directions; each
    /// opens its own session from the handle.
    pub fn build_vad(&self, spec: &LocalModelSpec) -> Result<Box<dyn VadProvider>, BuildError> {
        let lease = self
            .vad
            .acquire(spec.id.as_str().to_owned(), || self.recognition.vad(spec))?;
        lease.engine().descriptor().require_live()?;
        Ok(Box::new(SharedVad { lease }))
    }

    /// The recogniser parameters of the current configuration.
    fn asr_params(&self) -> AsrParams {
        asr_params(&self.state.read().cfg.pipeline)
    }

    /// Build the streaming recogniser of `spec`: one engine per model and parameter set, loaded and
    /// warmed up by the first build, a handle per direction. The model must produce partial results
    /// while audio arrives.
    pub fn build_asr(&self, spec: &LocalModelSpec) -> Result<Box<dyn StreamingAsr>, BuildError> {
        let params = self.asr_params();
        let lease = self.asr.acquire(recognition_key(spec, &params), || {
            self.recognition.asr(spec, &params)
        })?;
        let descriptor = lease.engine().descriptor();
        descriptor.require_partials()?;
        if let Some(lang) = spec.lang {
            descriptor.require_lang(lang)?;
        }
        Ok(Box::new(SharedAsr { lease }))
    }

    /// Build the refine-pass recogniser of `spec`.
    pub fn build_asr_refine(
        &self,
        spec: &LocalModelSpec,
    ) -> Result<Box<dyn RefineAsr>, BuildError> {
        let params = self.asr_params();
        let lease = self.refine.acquire(recognition_key(spec, &params), || {
            self.recognition.refine(spec, &params)
        })?;
        let descriptor = lease.engine().descriptor();
        descriptor.require_live()?;
        if let Some(lang) = spec.lang {
            descriptor.require_lang(lang)?;
        }
        Ok(Box::new(SharedRefine { lease }))
    }

    /// Build the translator of `spec`. The offline policy is checked again first: a blocked
    /// endpoint fails with [`BuildError::Blocked`] before anything is created, let alone
    /// connected.
    pub fn build_mt(&self, spec: &MtSpec) -> Result<Box<dyn Translator>, BuildError> {
        if let Err(blocked) = self.state.read().policy.check(&spec.base_url) {
            return Err(match blocked {
                prxvc_config::Blocked::Offline { host } => BuildError::Blocked {
                    provider: spec.id.clone(),
                    host,
                },
                prxvc_config::Blocked::MalformedUrl { detail, .. } => BuildError::InvalidEndpoint {
                    provider: spec.id.clone(),
                    detail,
                },
            });
        }
        let lease = self.mt.acquire(mt_key(spec), || start_mt(spec))?;
        let descriptor = lease.engine().descriptor();
        descriptor.require_native()?;
        descriptor.require_pair(spec.source, spec.target)?;
        Ok(Box::new(SharedTranslator { lease }))
    }

    /// Build the dialogue backend of `spec`. The offline policy is checked again first. The client
    /// is shared by every handle built from the same spec and holds the conversations.
    pub fn build_dialogue(&self, spec: &DialogueSpec) -> Result<Box<dyn DialogueBackend>, BuildError> {
        if let Err(blocked) = self.state.read().policy.check(&spec.base_url) {
            return Err(match blocked {
                prxvc_config::Blocked::Offline { host } => BuildError::Blocked {
                    provider: spec.id.clone(),
                    host,
                },
                prxvc_config::Blocked::MalformedUrl { detail, .. } => BuildError::InvalidEndpoint {
                    provider: spec.id.clone(),
                    detail,
                },
            });
        }
        let lease = self.dialogue.acquire(dialogue_key(spec), || start_dialogue(spec))?;
        lease.engine().descriptor().require_native()?;
        Ok(Box::new(SharedDialogue { lease }))
    }

    /// Build the synthesiser of `spec` with its voice registered. The engine is shared by every
    /// spec with the same model: the first build loads and warms it up, the later ones only add
    /// their voice.
    pub fn build_tts(&self, spec: &TtsSpec) -> Result<Box<dyn SpeechSynth>, BuildError> {
        let key = (spec.id.as_str().to_owned(), spec.fingerprint.clone());
        let lease = self.tts_engine(spec, key)?;
        register_voice(lease.engine(), spec)?;
        Ok(Box::new(SharedSynth { lease }))
    }

    /// Prepare the owner's cloned voice for the configured synthesis model of `lang` (the step
    /// after enrolment, and after a change of model): loads the shared engine when needed, derives
    /// the voice from the profile's reference recording and stores it in the profile's cache, from
    /// where resolution then finds it. The only operation that resolves a voice that is not yet
    /// prepared.
    pub fn prepare_cloned_voice(&self, lang: Lang) -> Result<VoicePrepared, PrepareError> {
        let resolved = self
            .with_resolver_opts(false, |r| r.tts(lang, &VoiceChoice::OwnClone))
            .map_err(PrepareError::Problems)?;
        let started = std::time::Instant::now();
        let spec = resolved.primary;
        let key = (spec.id.as_str().to_owned(), spec.fingerprint.clone());
        let lease = self.tts_engine(&spec, key)?;
        let cache_hit = register_voice(lease.engine(), &spec)?.unwrap_or(false);
        let VoicePlan::Cloned { profile_id, .. } = &spec.voice else {
            unreachable!("the owner's voice resolves to a cloned voice")
        };
        Ok(VoicePrepared {
            profile_id: profile_id.clone(),
            cache_hit,
            elapsed: started.elapsed(),
        })
    }

    fn tts_engine(
        &self,
        spec: &TtsSpec,
        key: (String, String),
    ) -> Result<Lease<QwenTts>, BuildError> {
        if !self.tts_loader.available() {
            return Err(BuildError::NotBuiltIn {
                provider: spec.id.clone(),
                runtime: spec.runtime.name().to_owned(),
            });
        }
        self.tts.acquire(key, || {
            let engine = self.tts_loader.load(spec)?;
            check_capabilities(&engine, spec)?;
            Ok(engine)
        })
    }

    /// The engines alive right now: handles keep them alive, dropping the last handle frees one.
    pub fn live_engines(&self) -> LiveEngines {
        LiveEngines {
            tts: self.tts.live(),
            mt: self.mt.live(),
            dialogue: self.dialogue.live(),
            asr: self.asr.live(),
            refine: self.refine.live(),
            vad: self.vad.live(),
        }
    }
}

/// The key of a recognition engine: the model and the parameters it was started with, so that a
/// changed setting builds a new engine for the next call instead of reusing a live one.
fn recognition_key(spec: &LocalModelSpec, params: &AsrParams) -> String {
    format!("{}|{:?}", spec.id, params)
}

fn mt_key(spec: &MtSpec) -> String {
    let secret = spec
        .secret
        .as_ref()
        .map(|s| format!("{:x}", Sha256::digest(s.expose().as_bytes())))
        .unwrap_or_default();
    format!(
        "{}|{}|{}|{}|{}|{:?}|{}",
        spec.id,
        spec.base_url,
        spec.model,
        serde_json::Value::Object(spec.params.clone()),
        spec.prompt_template,
        spec.timeouts,
        secret
    )
}

fn start_mt(spec: &MtSpec) -> Result<OpenAiChatMt, BuildError> {
    let mut cfg = OpenAiChatConfig::new(&spec.base_url, &spec.model)
        .with_provider_id(spec.id.as_str())
        .with_params(spec.params.clone())
        .with_timeouts(spec.timeouts)
        .with_template(spec.prompt_template);
    if let Some(secret) = &spec.secret {
        cfg = cfg.with_api_key(secret.expose());
    }
    OpenAiChatMt::new(cfg).map_err(|e| BuildError::Start {
        provider: spec.id.clone(),
        detail: e.to_string(),
    })
}

fn dialogue_key(spec: &DialogueSpec) -> String {
    let secret = spec
        .secret
        .as_ref()
        .map(|s| format!("{:x}", Sha256::digest(s.expose().as_bytes())))
        .unwrap_or_default();
    format!(
        "{}|{}|{}|{}|{:?}|{:?}|{}",
        spec.id,
        spec.base_url,
        spec.model,
        serde_json::Value::Object(spec.params.clone()),
        spec.timeouts,
        spec.history,
        secret
    )
}

fn start_dialogue(spec: &DialogueSpec) -> Result<OpenAiChatDialogue, BuildError> {
    let mut cfg = DialogueConfig::new(&spec.base_url, &spec.model)
        .with_provider_id(spec.id.as_str())
        .with_params(spec.params.clone())
        .with_timeouts(spec.timeouts)
        .with_history(spec.history);
    if let Some(secret) = &spec.secret {
        cfg = cfg.with_api_key(secret.expose());
    }
    OpenAiChatDialogue::new(cfg).map_err(|e| BuildError::Start {
        provider: spec.id.clone(),
        detail: e.to_string(),
    })
}
