//! The speech pipeline as the provider registry resolves it: one row per role and key, what leaves the machine, and every problem.
//!
//! The filler asks the registry (`prxvc-providers`) for `resolve_call()` and `egress_summary`.
//! Resolution is pure data: it touches no model and no network. The registry is always built
//! into the app; only the speech runtimes are behind the `qwentts` and `transcribe` features, and
//! a build without one reports the typed problem `RuntimeNotBuiltIn`, which the page shows with
//! its explanation. The mapping from specs and problems to the plain view models is separate from
//! the registry call so that it is tested on hand-built plans.

use std::sync::Arc;

use prxvc_config::AppConfig;
use prxvc_desktop_ui::settings_data::{
    AssistantData, DataLeavingView, DialogueChoiceView, DialogueKindView, LocalityView,
    PipelineData, PipelineProblem, PipelineRole, PipelineRow,
};
use prxvc_media::Lang;
use prxvc_models::Catalogue;
use prxvc_providers::{
    CallPlan, CallRole, DataLeaving, EgressSummary, EnvCredentials, Locality, Problem, ProblemKind,
    ProviderRegistry, Resolved, RoleKey, Slot, VoiceDirs,
};

use crate::call_kit::config::paths_for;
use crate::call_kit::models::StoreModels;

/// The registry of `config` over the model store, or the data that says why there is none.
fn open_registry(config: &AppConfig) -> Result<ProviderRegistry, PipelineData> {
    let paths = paths_for(config).map_err(store_problem)?;
    let catalogue = Arc::new(Catalogue::embedded());
    let models = StoreModels::open(paths.models_dir.clone(), Arc::clone(&catalogue))
        .map_err(store_problem)?;
    Ok(ProviderRegistry::new(
        config,
        catalogue,
        Arc::new(models),
        Arc::new(EnvCredentials::from_process()),
        VoiceDirs::from_paths(&paths),
    ))
}

/// Resolve the call for `config` and describe it.
pub(crate) fn probe(config: &AppConfig) -> PipelineData {
    let registry = match open_registry(config) {
        Ok(r) => r,
        Err(data) => return data,
    };
    match registry.resolve_call() {
        Ok(plan) => describe_plan(&plan, &registry.egress_summary(&plan)),
        Err(problems) => describe_problems(&problems),
    }
}

/// Resolve the assistant for `config`: the backends to choose from, each with what is wrong with
/// it, and the chain of the configured one.
pub(crate) fn probe_assistant(config: &AppConfig) -> AssistantData {
    let registry = match open_registry(config) {
        Ok(r) => r,
        Err(chain) => {
            return AssistantData {
                backends: Vec::new(),
                chain,
            };
        }
    };
    let system = sys_locale::get_locale();
    let lang = crate::assistant::params::language_of(config, system.as_deref());
    let choice = crate::assistant::params::voice_choice(config);
    let backends = registry
        .dialogue_choices()
        .into_iter()
        .map(|c| {
            let mut one = config.clone();
            one.providers.active.dialogue.insert(String::new(), c.id.clone());
            registry.reload(&one);
            let problem = match registry.resolve_dialogue() {
                Err(problems) => problems.first().map(problem_view),
                Ok(resolved) => unreachable_problem(&resolved.primary.base_url, resolved.primary.locality),
            };
            DialogueChoiceView {
                id: c.id,
                name: c.display_name,
                kind: match c.kind {
                    prxvc_providers::DialogueKind::LocalServer => DialogueKindView::LocalServer,
                    prxvc_providers::DialogueKind::Entry => DialogueKindView::Entry,
                    prxvc_providers::DialogueKind::OpenPrx => DialogueKindView::OpenPrx,
                },
                problem,
            }
        })
        .collect();
    registry.reload(config);
    let chain = describe_assistant(&registry, lang, &choice);
    AssistantData { backends, chain }
}

/// The problem of a dialogue server that is on this computer or network and does not answer.
fn unreachable_problem(base_url: &str, locality: Locality) -> Option<PipelineProblem> {
    crate::call_kit::providers::probe_address(base_url, locality)
        .err()
        .map(|reason| PipelineProblem {
            subject: Some((PipelineRole::Dialogue, String::new())),
            fallback: false,
            code: "server_not_answering".to_owned(),
            args: vec![base_url.to_owned(), reason],
        })
}

/// The assistant's chain as the registry resolves it (speech detection, recognition, the
/// dialogue backend and the voice), or every problem that keeps it from resolving.
fn describe_assistant(
    registry: &ProviderRegistry,
    lang: Lang,
    choice: &prxvc_providers::VoiceChoice,
) -> PipelineData {
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let vad = take(registry.resolve_vad(), &mut problems, &mut warnings);
    let asr = take(registry.resolve_asr(lang), &mut problems, &mut warnings);
    let dialogue = take(registry.resolve_dialogue(), &mut problems, &mut warnings);
    let tts = take(registry.resolve_tts(lang, choice), &mut problems, &mut warnings);
    let (Some(vad), Some(asr), Some(dialogue), Some(tts)) = (vad, asr, dialogue, tts) else {
        return describe_problems(&problems);
    };
    let mut views = vec![local_row(RoleKey::Vad, &vad.primary.id.to_string(), &runtime_name(&vad.primary.runtime))];
    views.push(local_row(
        RoleKey::Asr(lang),
        asr.primary.id.as_str(),
        &runtime_name(&asr.primary.runtime),
    ));
    views.push(PipelineRow {
        role: PipelineRole::Dialogue,
        key: String::new(),
        provider: dialogue.primary.display_name.clone(),
        target: format!("{} \u{b7} {}", dialogue.primary.base_url, dialogue.primary.model),
        data: data_view(dialogue.primary.data_leaving()),
        locality: locality_view(dialogue.primary.locality),
        host: Some(dialogue.primary.host.clone()),
        fallback: dialogue.fallback.as_ref().map(|s| s.display_name.clone()),
    });
    views.push(local_row(RoleKey::Tts(lang), &tts.primary.label, tts.primary.id.as_str()));
    let leaves = views
        .iter()
        .any(|r| r.data != DataLeavingView::Nothing && r.locality != LocalityView::ThisComputer);
    PipelineData::Resolved {
        rows: views,
        leaves_this_computer: leaves,
        warnings: warnings.iter().map(problem_view).collect(),
    }
}

fn local_row(key: RoleKey, provider: &str, target: &str) -> PipelineRow {
    PipelineRow {
        role: role_of(key),
        key: key.config_key(),
        provider: provider.to_owned(),
        target: target.to_owned(),
        data: DataLeavingView::Nothing,
        locality: LocalityView::ThisComputer,
        host: None,
        fallback: None,
    }
}

fn take<S>(
    result: Result<Resolved<S>, Vec<Problem>>,
    problems: &mut Vec<Problem>,
    warnings: &mut Vec<Problem>,
) -> Option<Resolved<S>> {
    match result {
        Ok(mut resolved) => {
            warnings.append(&mut resolved.warnings);
            Some(resolved)
        }
        Err(mut more) => {
            problems.append(&mut more);
            None
        }
    }
}

fn store_problem(detail: String) -> PipelineData {
    PipelineData::Problems(vec![PipelineProblem {
        subject: None,
        fallback: false,
        code: "store_unreadable".into(),
        args: vec![detail],
    }])
}

/// The translated name of a language, for use inside a sentence.
fn lang_name(lang: Lang) -> String {
    let key = format!("settings.lang.{}", lang.code());
    let text = prxvc_i18n::t(&key);
    if text == key {
        lang.code().to_owned()
    } else {
        text
    }
}

fn role_of(key: RoleKey) -> PipelineRole {
    match key.role() {
        CallRole::Vad => PipelineRole::Vad,
        CallRole::Asr => PipelineRole::Recognition,
        CallRole::AsrRefine => PipelineRole::Refine,
        CallRole::Mt => PipelineRole::Translation,
        CallRole::Tts => PipelineRole::Voice,
        CallRole::Dialogue => PipelineRole::Dialogue,
    }
}

fn data_view(d: DataLeaving) -> DataLeavingView {
    match d {
        DataLeaving::Nothing => DataLeavingView::Nothing,
        DataLeaving::Text => DataLeavingView::Text,
        DataLeaving::Audio => DataLeavingView::Audio,
    }
}

fn locality_view(l: Locality) -> LocalityView {
    match l {
        Locality::ThisComputer => LocalityView::ThisComputer,
        Locality::LocalNetwork => LocalityView::LocalNetwork,
        Locality::Remote => LocalityView::Remote,
    }
}

/// A row for `key` served by the provider `id` (shown as `name` at `target`), with what leaves
/// taken from the disclosure.
fn row(
    key: RoleKey,
    id: &str,
    name: &str,
    target: &str,
    fallback: Option<&str>,
    summary: &EgressSummary,
) -> PipelineRow {
    let egress = summary
        .rows
        .iter()
        .find(|r| r.role == key && r.provider.as_str() == id);
    PipelineRow {
        role: role_of(key),
        key: key.config_key(),
        provider: name.to_owned(),
        target: target.to_owned(),
        data: egress.map_or(DataLeavingView::Nothing, |r| data_view(r.data)),
        locality: egress.map_or(LocalityView::ThisComputer, |r| locality_view(r.locality)),
        host: egress.and_then(|r| r.host.clone()),
        fallback: fallback.map(str::to_owned),
    }
}

/// Describe a resolved call (pure).
pub(crate) fn describe_plan(plan: &CallPlan, summary: &EgressSummary) -> PipelineData {
    let mut rows = Vec::new();
    let vad_fallback = plan.vad.fallback.as_ref().map(|s| s.id.to_string());
    rows.push(row(
        RoleKey::Vad,
        plan.vad.primary.id.as_str(),
        plan.vad.primary.id.as_str(),
        &runtime_name(&plan.vad.primary.runtime),
        vad_fallback.as_deref(),
        summary,
    ));
    for d in [&plan.outgoing, &plan.incoming] {
        push_local(&mut rows, RoleKey::Asr(d.source), &d.asr, summary);
        if let Some(refine) = &d.refine {
            push_local(&mut rows, RoleKey::AsrRefine(d.source), refine, summary);
        }
        let key = RoleKey::Mt {
            source: d.source,
            target: d.target,
        };
        let fallback = d.mt.fallback.as_ref().map(|s| s.display_name.clone());
        rows.push(row(
            key,
            d.mt.primary.id.as_str(),
            &d.mt.primary.display_name,
            &d.mt.primary.base_url,
            fallback.as_deref(),
            summary,
        ));
        let fallback = d.tts.fallback.as_ref().map(|s| s.label.clone());
        rows.push(row(
            RoleKey::Tts(d.target),
            d.tts.primary.id.as_str(),
            &d.tts.primary.label,
            d.tts.primary.id.as_str(),
            fallback.as_deref(),
            summary,
        ));
    }
    rows.dedup();
    PipelineData::Resolved {
        rows,
        leaves_this_computer: summary.leaves_this_computer(),
        warnings: plan.warnings.iter().map(problem_view).collect(),
    }
}

fn push_local(
    rows: &mut Vec<PipelineRow>,
    key: RoleKey,
    resolved: &Resolved<prxvc_providers::LocalModelSpec>,
    summary: &EgressSummary,
) {
    let fallback = resolved.fallback.as_ref().map(|s| s.id.to_string());
    rows.push(row(
        key,
        resolved.primary.id.as_str(),
        resolved.primary.id.as_str(),
        &runtime_name(&resolved.primary.runtime),
        fallback.as_deref(),
        summary,
    ));
}

fn runtime_name(runtime: &prxvc_providers::Runtime) -> String {
    use prxvc_providers::Runtime;
    match runtime {
        Runtime::LlamaServer => "llama-server".to_owned(),
        Runtime::QwenTts => "qwentts.cpp".to_owned(),
        Runtime::Transcribe => "transcribe.cpp".to_owned(),
        Runtime::OnnxRuntime => "onnxruntime".to_owned(),
        Runtime::Other(name) => name.clone(),
    }
}

/// Describe the problems that keep the call from resolving (pure).
pub(crate) fn describe_problems(problems: &[Problem]) -> PipelineData {
    PipelineData::Problems(problems.iter().map(problem_view).collect())
}

fn size_text(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let gb = bytes as f64 / 1_000_000_000.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    }
}

/// One problem as a code with its values.
pub(crate) fn problem_view(p: &Problem) -> PipelineProblem {
    let (code, args): (&str, Vec<String>) = match &p.kind {
        ProblemKind::InvalidLanguage { key, value } => {
            ("invalid_language", vec![key.clone(), value.clone()])
        }
        ProblemKind::LanguageNotSet { key } => ("language_not_set", vec![key.clone()]),
        ProblemKind::InvalidSetting { key, value } => {
            ("invalid_setting", vec![key.clone(), value.clone()])
        }
        ProblemKind::NoProviderSelected => ("no_provider", vec![]),
        ProblemKind::UnknownProvider { id } => ("unknown_provider", vec![id.clone()]),
        ProblemKind::UnknownProtocol { entry, protocol } => {
            ("unknown_protocol", vec![entry.clone(), protocol.clone()])
        }
        ProblemKind::UnknownTemplate { entry, template } => {
            ("unknown_template", vec![entry.clone(), template.clone()])
        }
        ProblemKind::KindNotValidForRole { id, .. } => ("wrong_kind", vec![id.to_string()]),
        ProblemKind::ModelNotInstalled { model, size_bytes } => (
            "model_not_installed",
            vec![model.to_string(), size_text(*size_bytes)],
        ),
        ProblemKind::ModelInstalling { model } => ("model_installing", vec![model.to_string()]),
        ProblemKind::ModelFailedVerification { model, .. } => {
            ("model_failed_verification", vec![model.to_string()])
        }
        ProblemKind::ModelFailed { model, detail } => {
            ("model_failed", vec![model.to_string(), detail.clone()])
        }
        ProblemKind::ModelFilesIncomplete { model, purpose } => (
            "model_files_incomplete",
            vec![model.to_string(), purpose.clone()],
        ),
        ProblemKind::RuntimeNotBuiltIn { id, runtime } => (
            "runtime_not_built_in",
            vec![id.to_string(), runtime.clone()],
        ),
        ProblemKind::NotStreaming { id, reason } => {
            ("not_streaming", vec![id.to_string(), reason.clone()])
        }
        ProblemKind::LanguageUnsupported { id, lang } => (
            "language_unsupported",
            vec![id.to_string(), lang_name(*lang)],
        ),
        ProblemKind::PairUnsupported { id, from, to } => (
            "pair_unsupported",
            vec![id.to_string(), lang_name(*from), lang_name(*to)],
        ),
        ProblemKind::CredentialMissing { entry } => ("credential_missing", vec![entry.clone()]),
        ProblemKind::CredentialUnreadable { entry, detail } => {
            ("credential_unreadable", vec![entry.clone(), detail.clone()])
        }
        ProblemKind::EndpointBlocked { entry, host } => {
            ("endpoint_blocked", vec![entry.clone(), host.clone()])
        }
        ProblemKind::InvalidEndpoint { entry, detail } => {
            ("invalid_endpoint", vec![entry.clone(), detail.clone()])
        }
        ProblemKind::InvalidParams { id, param, reason } => (
            "invalid_params",
            vec![id.to_string(), param.clone(), reason.clone()],
        ),
        ProblemKind::VoiceProfileNotSet => ("voice_profile_not_set", vec![]),
        ProblemKind::VoiceProfileMissing { profile_id, .. } => {
            ("voice_profile_missing", vec![profile_id.clone()])
        }
        ProblemKind::VoiceProfileUnreadable { profile_id, detail } => (
            "voice_profile_unreadable",
            vec![profile_id.clone(), detail.clone()],
        ),
        ProblemKind::VoiceNotPrepared { profile_id } => {
            ("voice_not_prepared", vec![profile_id.clone()])
        }
        ProblemKind::VoicePreparedForOtherModel { profile_id, .. } => {
            ("voice_prepared_for_other_model", vec![profile_id.clone()])
        }
        ProblemKind::VoicePresetUnknown { name } => ("voice_preset_unknown", vec![name.clone()]),
        ProblemKind::VoicePresetFileMissing { preset, .. } => {
            ("voice_preset_file_missing", vec![preset.clone()])
        }
        ProblemKind::VoicePresetUnreadable { preset, detail } => (
            "voice_preset_unreadable",
            vec![preset.clone(), detail.clone()],
        ),
        ProblemKind::VoicePresetLanguage {
            preset,
            wanted,
            found,
        } => (
            "voice_preset_language",
            vec![preset.clone(), found.clone(), lang_name(*wanted)],
        ),
        ProblemKind::NotAvailableYet { id, name } => {
            ("not_available_yet", vec![id.to_string(), name.clone()])
        }
        ProblemKind::FallbackIsPrimary { id } => ("fallback_is_primary", vec![id.to_string()]),
        // `ProblemKind` is non-exhaustive: a kind this version does not know is still shown.
        other => ("other", vec![format!("{other}")]),
    };
    PipelineProblem {
        subject: p.role.map(|k| (role_of(k), k.config_key())),
        fallback: p.slot == Slot::Fallback,
        code: code.to_owned(),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prxvc_models::ModelId;
    use prxvc_ports::ProviderId;

    fn problem(role: Option<RoleKey>, slot: Slot, kind: ProblemKind) -> Problem {
        Problem { role, slot, kind }
    }

    #[test]
    fn a_missing_model_is_a_code_with_the_model_and_its_size() {
        let p = problem(
            Some(RoleKey::Mt {
                source: Lang::Zh,
                target: Lang::En,
            }),
            Slot::Primary,
            ProblemKind::ModelNotInstalled {
                model: ModelId::new("gemma"),
                size_bytes: 4_600_000_000,
            },
        );
        let v = problem_view(&p);
        assert_eq!(v.code, "model_not_installed");
        assert_eq!(v.args, ["gemma", "4.6 GB"]);
        assert_eq!(
            v.subject,
            Some((PipelineRole::Translation, "zh>en".to_owned()))
        );
        assert!(!v.fallback);
    }

    #[test]
    fn a_runtime_that_is_not_built_in_names_the_provider_and_the_runtime() {
        let p = problem(
            Some(RoleKey::Asr(Lang::En)),
            Slot::Primary,
            ProblemKind::RuntimeNotBuiltIn {
                id: ProviderId::new("qwen3-asr"),
                runtime: "transcribe".into(),
            },
        );
        let v = problem_view(&p);
        assert_eq!(
            (v.code.as_str(), v.args.as_slice()),
            (
                "runtime_not_built_in",
                ["qwen3-asr".to_owned(), "transcribe".to_owned()].as_slice()
            )
        );
        assert_eq!(
            v.subject,
            Some((PipelineRole::Recognition, "en".to_owned()))
        );
    }

    #[test]
    fn fallback_problems_and_whole_call_problems_are_marked() {
        let v = problem_view(&problem(
            None,
            Slot::Fallback,
            ProblemKind::NoProviderSelected,
        ));
        assert!(v.fallback);
        assert_eq!(v.subject, None);
        assert_eq!(v.code, "no_provider");
    }

    #[test]
    fn every_code_the_filler_produces_has_a_sentence_in_both_languages() {
        // The codes of the mapping above; the strings live in `prxvc-i18n`.
        let codes = [
            "invalid_language",
            "language_not_set",
            "invalid_setting",
            "no_provider",
            "unknown_provider",
            "unknown_protocol",
            "unknown_template",
            "wrong_kind",
            "model_not_installed",
            "model_installing",
            "model_failed_verification",
            "model_failed",
            "model_files_incomplete",
            "runtime_not_built_in",
            "not_streaming",
            "language_unsupported",
            "pair_unsupported",
            "credential_missing",
            "credential_unreadable",
            "endpoint_blocked",
            "invalid_endpoint",
            "invalid_params",
            "voice_profile_not_set",
            "voice_profile_missing",
            "voice_profile_unreadable",
            "voice_not_prepared",
            "voice_prepared_for_other_model",
            "voice_preset_unknown",
            "voice_preset_file_missing",
            "voice_preset_unreadable",
            "voice_preset_language",
            "fallback_is_primary",
            "not_available_yet",
            "server_not_answering",
            "store_unreadable",
            "other",
        ];
        for code in codes {
            let key = format!("settings.pipeline.problem.{code}");
            assert_ne!(prxvc_i18n::t(&key), key, "{code} has no sentence");
        }
    }

    #[test]
    fn sizes_are_shown_in_megabytes_below_a_gigabyte() {
        assert_eq!(size_text(885_098), "1 MB");
        assert_eq!(size_text(250_000_000), "250 MB");
        assert_eq!(size_text(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn an_unreadable_store_is_a_problem_of_the_call_as_a_whole() {
        match store_problem("denied".into()) {
            PipelineData::Problems(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].code, "store_unreadable");
                assert_eq!(list[0].args, ["denied"]);
            }
            other => panic!("{other:?}"),
        }
    }
}
