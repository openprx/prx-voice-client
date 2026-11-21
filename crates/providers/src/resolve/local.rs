//! Catalogue models run by an in-process runtime: VAD, recognition and synthesis.

use prxvc_media::Lang;
use prxvc_models::{FailReason, ModelEntry, ModelId, NotReady, ReadyModel, Role, Streaming};
use prxvc_ports::ProviderId;

use super::protocol::Protocol;
use super::voice::VoiceChoice;
use super::{Resolver, languages_cover};
use crate::fingerprint::model_fingerprint;
use crate::keys::CallRole;
use crate::problem::ProblemKind;
use crate::recognition::{ASR_FILE_PURPOSE, VAD_FILE_PURPOSE};
use crate::runtime::Runtime;
use crate::spec::{LocalModelSpec, TtsFiles, TtsSpec};

const NO_STREAM_REASON: &str = "The model returns only whole utterances and does not stream.";
/// A recogniser that returns one result per segment: it can refine a clause, it cannot be the
/// live recogniser.
const FINALS_ONLY_REASON: &str = "The model returns only a final result per segment and does not produce partial results while audio arrives; it can serve the refine pass.";
/// Translation and synthesis must stream incrementally.
const NOT_INCREMENTAL_REASON: &str = "The model does not stream incrementally while input arrives.";

/// The catalogue role that serves a call role.
fn catalogue_role(role: CallRole) -> Role {
    match role {
        CallRole::Vad => Role::Vad,
        CallRole::Asr | CallRole::AsrRefine => Role::Asr,
        CallRole::Mt | CallRole::Dialogue => Role::Mt,
        CallRole::Tts => Role::Tts,
    }
}

/// Whether a protocol that serves `served` can serve `asked` (the refine pass is recognition).
fn role_matches(served: CallRole, asked: CallRole) -> bool {
    served == asked || (served == CallRole::Asr && asked == CallRole::AsrRefine)
}

/// The problem that explains why the model manager has no ready model.
pub(crate) fn not_ready_problem(id: &ProviderId, nr: NotReady) -> ProblemKind {
    match nr {
        NotReady::NoSelection => ProblemKind::ModelFailed {
            model: ModelId::new(id.as_str()),
            detail: "the model manager does not know this model".to_owned(),
        },
        NotReady::NotInstalled { id, size } => ProblemKind::ModelNotInstalled {
            model: id,
            size_bytes: size,
        },
        NotReady::InProgress { id, .. } => ProblemKind::ModelInstalling { model: id },
        NotReady::Failed {
            id,
            reason: FailReason::ChecksumMismatch { file },
        } => ProblemKind::ModelFailedVerification {
            model: id,
            file: Some(file),
        },
        NotReady::Failed { id, reason } => ProblemKind::ModelFailed {
            model: id,
            detail: reason.to_string(),
        },
        NotReady::UnsupportedRuntime { id, runtime } => ProblemKind::RuntimeNotBuiltIn {
            id: ProviderId::new(id.as_str()),
            runtime,
        },
    }
}

impl Resolver<'_> {
    /// The catalogue entry for `id`, or the problem that says why it cannot serve `role`.
    fn model_entry(&self, id: &str, role: CallRole) -> Result<&ModelEntry, ProblemKind> {
        let pid = ProviderId::new(id);
        let Some(entry) = self.catalogue.get(&ModelId::new(id)) else {
            return Err(
                match self.cfg.providers.entries.iter().find(|e| e.id == id) {
                    // A protocol that serves the role has no client in this build (the cloud
                    // recognition and synthesis clients are written when the owner wants one).
                    Some(e)
                        if Protocol::parse(&e.protocol)
                            .is_some_and(|p| role_matches(p.role(), role)) =>
                    {
                        ProblemKind::RuntimeNotBuiltIn {
                            id: pid,
                            runtime: e.protocol.clone(),
                        }
                    }
                    Some(e) => ProblemKind::KindNotValidForRole {
                        id: pid,
                        role,
                        found: format!("an API provider entry ({})", e.protocol),
                    },
                    None => ProblemKind::UnknownProvider { id: id.to_owned() },
                },
            );
        };
        if entry.role != catalogue_role(role) {
            return Err(ProblemKind::KindNotValidForRole {
                id: pid,
                role,
                found: format!("a {:?} model", entry.role),
            });
        }
        Ok(entry)
    }

    /// The streaming requirement of `role` against what the entry offers.
    ///
    /// * Voice activity detection runs on every kind.
    /// * Streaming recognition needs partial results while audio arrives: `native` or `redecode`.
    /// * The refine pass takes any model that is not whole-utterance only.
    /// * Translation and synthesis need incremental (`native`) streaming.
    fn streaming_problem(&self, entry: &ModelEntry, role: CallRole) -> Option<ProblemKind> {
        let reason = match (entry.streaming, role) {
            (_, CallRole::Vad) => None,
            (Streaming::None, _) => Some(NO_STREAM_REASON),
            (Streaming::Segmented, CallRole::Asr) => Some(FINALS_ONLY_REASON),
            (Streaming::Redecode | Streaming::Segmented, CallRole::Mt | CallRole::Tts) => {
                Some(NOT_INCREMENTAL_REASON)
            }
            _ => None,
        }?;
        Some(ProblemKind::NotStreaming {
            id: ProviderId::new(entry.id.as_str()),
            reason: reason.to_owned(),
        })
    }

    /// The checks every local model passes: runtime, streaming, language and readiness. All
    /// independent problems are returned together.
    fn check_local(
        &self,
        entry: &ModelEntry,
        role: CallRole,
        lang: Option<Lang>,
    ) -> (Vec<ProblemKind>, Runtime, Option<ReadyModel>) {
        let id = ProviderId::new(entry.id.as_str());
        let mut problems = Vec::new();
        let runtime = Runtime::parse(&entry.runtime.name);
        if !runtime.is_built_in(self.built) {
            problems.push(ProblemKind::RuntimeNotBuiltIn {
                id: id.clone(),
                runtime: runtime.name().to_owned(),
            });
        }
        problems.extend(self.streaming_problem(entry, role));
        if let Some(lang) = lang
            && !languages_cover(&entry.languages, lang)
        {
            problems.push(ProblemKind::LanguageUnsupported {
                id: id.clone(),
                lang,
            });
        }
        let ready = match self.models.ready(&entry.id) {
            Ok(ready) => Some(ready),
            Err(nr) => {
                let p = not_ready_problem(&id, nr);
                if !problems.contains(&p) {
                    problems.push(p);
                }
                None
            }
        };
        (problems, runtime, ready)
    }

    /// A VAD or recognition model.
    pub(super) fn local_model(
        &self,
        id: &str,
        role: CallRole,
        lang: Option<Lang>,
    ) -> Result<LocalModelSpec, Vec<ProblemKind>> {
        let entry = self.model_entry(id, role).map_err(|p| vec![p])?;
        let (mut problems, runtime, ready) = self.check_local(entry, role, lang);
        if runtime == Runtime::Transcribe
            && let Some(model) = &ready
        {
            let purpose = if role == CallRole::Vad {
                VAD_FILE_PURPOSE
            } else {
                ASR_FILE_PURPOSE
            };
            if model.file(purpose).is_none() {
                problems.push(ProblemKind::ModelFilesIncomplete {
                    model: entry.id.clone(),
                    purpose: purpose.to_owned(),
                });
            }
        }
        match ready {
            Some(model) if problems.is_empty() => Ok(LocalModelSpec {
                id: ProviderId::new(id),
                lang,
                runtime,
                model,
                streaming: entry.streaming,
            }),
            _ => Err(problems),
        }
    }

    /// A synthesis model with the voice `voice`.
    pub(super) fn tts_model(
        &self,
        id: &str,
        lang: Lang,
        voice: &VoiceChoice,
    ) -> Result<TtsSpec, Vec<ProblemKind>> {
        let entry = self.model_entry(id, CallRole::Tts).map_err(|p| vec![p])?;
        let (mut problems, runtime, ready) = self.check_local(entry, CallRole::Tts, Some(lang));
        let fingerprint = model_fingerprint(entry);
        let mut files = None;
        if let Some(model) = &ready {
            let find = |purpose: &str| model.file(purpose).map(std::path::Path::to_path_buf);
            match (find("model"), find("codec")) {
                (Some(talker), Some(codec)) => files = Some(TtsFiles { talker, codec }),
                (talker, _) => problems.push(ProblemKind::ModelFilesIncomplete {
                    model: entry.id.clone(),
                    purpose: if talker.is_none() { "model" } else { "codec" }.to_owned(),
                }),
            }
        }
        let voice_plan = self.voice_plan(entry, &runtime, &fingerprint, lang, voice);
        let voice_plan = match voice_plan {
            Ok(plan) => Some(plan),
            Err(mut p) => {
                problems.append(&mut p);
                None
            }
        };
        match (ready, files, voice_plan) {
            (Some(model), Some(files), Some(voice)) if problems.is_empty() => Ok(TtsSpec {
                id: ProviderId::new(id),
                label: entry.display_name.clone(),
                lang,
                runtime,
                model,
                files,
                fingerprint,
                voice,
            }),
            _ => Err(problems),
        }
    }
}
