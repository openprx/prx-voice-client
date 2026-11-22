//! Resolution: from configuration, catalogue, model store, credentials and network policy to typed
//! specifications, collecting every problem instead of stopping at the first.
//!
//! * `mod`: the `Resolver` and the selection rules (primary, fallback, catalogue defaults).
//! * `local`: catalogue models for VAD, recognition and synthesis.
//! * `mt`: provider entries of the OpenAI-compatible chat protocol (translation and dialogue).
//! * `system_mt`: system translation, the translation engine of `call.translation_engine = "system"`.
//! * `dialogue`: the assistant's dialogue backend and the OpenPRX placeholder.
//! * `voice`: the voice of a synthesis spec.
//!
//! Resolution is pure with respect to engines and the network: it reads files' metadata, the
//! credential store and the policy, and builds nothing.

mod dialogue;
mod local;
mod mt;
mod protocol;
mod system_mt;
mod voice;

use std::collections::BTreeSet;

use prxvc_config::{AppConfig, NetworkPolicy, TranslationEngine};
use prxvc_media::Lang;
use prxvc_models::{Badge, Candidate, Catalogue, HardwareInfo, LangKey, Role, Streaming};
use prxvc_ports::ProviderId;

use crate::credentials::CredentialReader;
use crate::keys::{CallRole, RoleKey};
use crate::model_source::ModelSource;
use crate::problem::{Problem, ProblemKind, Slot};
use crate::spec::{LocalModelSpec, MtSpec, Resolved, TtsSpec};

pub use dialogue::{
    DEFAULT_DIALOGUE_PROVIDER, DialogueChoice, DialogueKind, OPENPRX_NAME, OPENPRX_PROVIDER,
};
pub use system_mt::SYSTEM_MT_PROVIDER;
pub use voice::{VoiceChoice, VoiceDirs};

/// The id of the built-in translation provider: a `llama-server` endpoint from the catalogue
/// template of the same name, serving the catalogue's default translation model (PD-2.6.2). It
/// applies when `providers.active.mt` names nothing and no user entry has this id.
pub const DEFAULT_MT_PROVIDER: &str = "local-llama-server";

/// The catalogue template of [`DEFAULT_MT_PROVIDER`].
pub(crate) const DEFAULT_MT_TEMPLATE: &str = "llama-server";

/// Everything resolution reads.
pub(crate) struct Resolver<'a> {
    pub cfg: &'a AppConfig,
    pub catalogue: &'a Catalogue,
    pub models: &'a dyn ModelSource,
    pub creds: &'a dyn CredentialReader,
    pub policy: &'a NetworkPolicy,
    pub hardware: &'a HardwareInfo,
    pub dirs: &'a VoiceDirs,
    pub built: &'a crate::runtime::BuiltRuntimes,
    /// Whether a cloned voice must already be prepared for the model. Preparing it is the one
    /// operation that resolves without.
    pub require_prepared: bool,
}

fn selection_map(sel: &prxvc_config::RoleSelection, role: CallRole) -> &prxvc_config::RoleMap {
    match role {
        CallRole::Vad => &sel.vad,
        CallRole::Asr => &sel.asr,
        CallRole::AsrRefine => &sel.asr_refine,
        CallRole::Mt => &sel.mt,
        CallRole::Tts => &sel.tts,
        CallRole::Dialogue => &sel.dialogue,
    }
}

fn configured(map: &prxvc_config::RoleMap, key: &str) -> Option<String> {
    map.get(key)
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

impl Resolver<'_> {
    fn runtime_names(&self) -> Vec<&str> {
        let names: BTreeSet<&str> = self
            .catalogue
            .models()
            .iter()
            .map(|m| m.runtime.name.as_str())
            .collect();
        names.into_iter().collect()
    }

    /// The catalogue's default for a role, when it has one. The runtime filter admits every
    /// runtime the catalogue names: a default this build cannot run is reported as such, not
    /// skipped in favour of something else.
    fn default_id(&self, key: RoleKey) -> Option<String> {
        let runtimes = self.runtime_names();
        let recommended = |role: Role, key: &LangKey| {
            self.catalogue
                .recommended(role, key, self.hardware, &runtimes)
                .map(|e| e.id.as_str().to_owned())
        };
        match key {
            RoleKey::Vad => recommended(Role::Vad, &LangKey::Any),
            RoleKey::Asr(l) => recommended(Role::Asr, &LangKey::Lang(l.code().to_owned())),
            RoleKey::Tts(l) => recommended(Role::Tts, &LangKey::Lang(l.code().to_owned())),
            RoleKey::Mt { .. } => Some(DEFAULT_MT_PROVIDER.to_owned()),
            RoleKey::Dialogue => Some(DEFAULT_DIALOGUE_PROVIDER.to_owned()),
            RoleKey::AsrRefine(l) => {
                let candidates = self.catalogue.candidates(
                    Role::Asr,
                    &LangKey::Lang(l.code().to_owned()),
                    self.hardware,
                    &runtimes,
                );
                // The refine model is the most accurate recogniser the call can run: a model
                // that re-decodes (it has a call adapter) and carries the `most_accurate` badge,
                // else the first such model. A finals-only model is never a default: it is
                // chosen explicitly or not at all.
                let redecode = |c: &&Candidate<'_>| c.entry.streaming == Streaming::Redecode;
                candidates
                    .iter()
                    .filter(redecode)
                    .find(|c| c.badge == Some(Badge::MostAccurate))
                    .or_else(|| candidates.iter().find(redecode))
                    .map(|c| c.entry.id.as_str().to_owned())
            }
        }
    }

    /// The primary and the fallback of `key`, each resolved by `one`.
    ///
    /// `Err` holds every problem, the fallback's included; `Ok` holds the fallback's problems as
    /// warnings.
    fn resolve_pair<S>(
        &self,
        key: RoleKey,
        one: impl Fn(&str) -> Result<S, Vec<ProblemKind>>,
    ) -> Result<Resolved<S>, Vec<Problem>> {
        self.resolve_pair_with(key, None, one)
    }

    /// Like [`Self::resolve_pair`], with the primary id decided by the caller (translation: the
    /// engine choice) instead of `providers.active` and the catalogue default.
    fn resolve_pair_with<S>(
        &self,
        key: RoleKey,
        primary: Option<String>,
        one: impl Fn(&str) -> Result<S, Vec<ProblemKind>>,
    ) -> Result<Resolved<S>, Vec<Problem>> {
        let role = key.role();
        let config_key = key.config_key();
        let wrap = |slot: Slot, kinds: Vec<ProblemKind>| -> Vec<Problem> {
            kinds
                .into_iter()
                .map(|kind| Problem {
                    role: Some(key),
                    slot,
                    kind,
                })
                .collect()
        };

        let primary_id = primary
            .or_else(|| configured(selection_map(&self.cfg.providers.active, role), &config_key))
            .or_else(|| self.default_id(key));
        let fallback_id = configured(
            selection_map(&self.cfg.providers.fallback, role),
            &config_key,
        )
        .filter(|id| !id.eq_ignore_ascii_case("none"));

        let primary = match &primary_id {
            Some(id) => one(id).map_err(|k| wrap(Slot::Primary, k)),
            None => Err(wrap(Slot::Primary, vec![ProblemKind::NoProviderSelected])),
        };
        let fallback = fallback_id.as_ref().map(|id| {
            if Some(id) == primary_id.as_ref() {
                return Err(wrap(
                    Slot::Fallback,
                    vec![ProblemKind::FallbackIsPrimary {
                        id: ProviderId::new(id.clone()),
                    }],
                ));
            }
            one(id).map_err(|k| wrap(Slot::Fallback, k))
        });

        match (primary, fallback) {
            (Ok(primary), None) => Ok(Resolved {
                primary,
                fallback: None,
                warnings: Vec::new(),
            }),
            (Ok(primary), Some(Ok(fb))) => Ok(Resolved {
                primary,
                fallback: Some(fb),
                warnings: Vec::new(),
            }),
            (Ok(primary), Some(Err(warnings))) => Ok(Resolved {
                primary,
                fallback: None,
                warnings,
            }),
            (Err(mut problems), fb) => {
                if let Some(Err(more)) = fb {
                    problems.extend(more);
                }
                Err(problems)
            }
        }
    }

    /// The voice activity detector.
    pub fn vad(&self) -> Result<Resolved<LocalModelSpec>, Vec<Problem>> {
        self.resolve_pair(RoleKey::Vad, |id| self.local_model(id, CallRole::Vad, None))
    }

    /// Streaming recognition of `lang`.
    pub fn asr(&self, lang: Lang) -> Result<Resolved<LocalModelSpec>, Vec<Problem>> {
        self.resolve_pair(RoleKey::Asr(lang), |id| {
            self.local_model(id, CallRole::Asr, Some(lang))
        })
    }

    /// The refine pass for `lang`.
    pub fn refine(&self, lang: Lang) -> Result<Resolved<LocalModelSpec>, Vec<Problem>> {
        self.resolve_pair(RoleKey::AsrRefine(lang), |id| {
            self.local_model(id, CallRole::AsrRefine, Some(lang))
        })
    }

    /// Translation from `source` to `target`.
    ///
    /// `call.translation_engine` decides the primary: `system` is system translation whatever
    /// `providers.active.mt` says (that selection is kept for `llm`), `llm` is the selected
    /// language-model provider. A configured fallback may name either kind.
    pub fn mt(&self, source: Lang, target: Lang) -> Result<Resolved<MtSpec>, Vec<Problem>> {
        let key = RoleKey::Mt { source, target };
        let raw = &self.cfg.call.translation_engine;
        let Some(engine) = TranslationEngine::parse(raw) else {
            return Err(vec![Problem {
                role: Some(key),
                slot: Slot::Primary,
                kind: ProblemKind::InvalidSetting {
                    key: "call.translation_engine".to_owned(),
                    value: raw.clone(),
                },
            }]);
        };
        let primary = (engine == TranslationEngine::System).then(|| SYSTEM_MT_PROVIDER.to_owned());
        // `providers.active.mt` names a language-model provider. Naming system translation there
        // while the engine is `llm` would be a second source of truth for the same choice.
        let active = configured(&self.cfg.providers.active.mt, &key.config_key());
        if engine == TranslationEngine::Llm && active.as_deref() == Some(SYSTEM_MT_PROVIDER) {
            return Err(vec![Problem {
                role: Some(key),
                slot: Slot::Primary,
                kind: ProblemKind::InvalidSetting {
                    key: "providers.active.mt".to_owned(),
                    value: SYSTEM_MT_PROVIDER.to_owned(),
                },
            }]);
        }
        self.resolve_pair_with(key, primary, |id| {
            if id == SYSTEM_MT_PROVIDER {
                self.system_mt_provider(source, target)
            } else {
                self.mt_provider(id, source, target)
            }
        })
    }

    /// Synthesis of `lang` with `voice`.
    pub fn tts(&self, lang: Lang, voice: &VoiceChoice) -> Result<Resolved<TtsSpec>, Vec<Problem>> {
        self.resolve_pair(RoleKey::Tts(lang), |id| self.tts_model(id, lang, voice))
    }
}

/// `Any` matches every language; a language entry matches its code.
pub(crate) fn languages_cover(specs: &[prxvc_models::LanguageSpec], lang: Lang) -> bool {
    specs.iter().any(|s| match s {
        prxvc_models::LanguageSpec::Any => true,
        prxvc_models::LanguageSpec::Lang { code } => code == lang.code(),
        prxvc_models::LanguageSpec::Pair { .. } => false,
    })
}
