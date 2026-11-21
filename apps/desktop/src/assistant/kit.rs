//! Preparing the assistant's providers from the registry: resolve what the session needs, report every problem as an issue, check that the dialogue server answers, then build.
//!
//! Recognition, voice activity and synthesis resolve exactly like a call's (the same registry, the
//! same models); the dialogue is the registry's own role. Nothing is built before every problem of
//! the resolution is known, and nothing falls back: a role that cannot be served is an issue.

use std::sync::Arc;

use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;
use prxvc_media::Lang;
use prxvc_ports::{DialogueBackend, SpeechSynth, StreamingAsr, VadProvider, VoiceSelection};
use prxvc_providers::{Problem, ProviderRegistry};

use super::issues;
use super::params::{language_of, voice_choice};
use crate::call_kit::providers::{ProviderHost, probe_address};
use prxvc_desktop_ui::AssistantIssue;

/// Everything a session needs from the registry.
pub(crate) struct Prepared {
    /// Owns the shared engines; the providers below hold leases on them.
    pub registry: Arc<ProviderRegistry>,
    /// Voice activity detection.
    pub vad: Arc<dyn VadProvider>,
    /// Streaming recognition of `lang`.
    pub asr: Arc<dyn StreamingAsr>,
    /// Synthesis of `lang` with `voice`.
    pub tts: Arc<dyn SpeechSynth>,
    /// The dialogue backend.
    pub dialogue: Arc<dyn DialogueBackend>,
    /// The language of the session.
    pub lang: Lang,
    /// The voice the assistant speaks with.
    pub voice: VoiceSelection,
    /// The hold on the local model server while the session runs; dropping it lets the server
    /// idle out.
    pub server: Option<crate::call_kit::model_server::Hold>,
}

/// Resolve, check and build. `system` is the operating system's language tag.
///
/// # Errors
/// Every problem found, each as an issue; nothing stays loaded.
pub(crate) fn prepare(
    config: &AppConfig,
    paths: Option<&AppPaths>,
    system: Option<&str>,
) -> Result<Prepared, Vec<AssistantIssue>> {
    let registry = ProviderHost::registry_for(config, paths)
        .map_err(|fault| vec![issues_of_fault(&fault.detail)])?;
    let lang = language_of(config, system);
    let choice = voice_choice(config);

    let mut problems: Vec<Problem> = Vec::new();
    let vad = take(registry.resolve_vad(), &mut problems);
    let asr = take(registry.resolve_asr(lang), &mut problems);
    let tts = take(registry.resolve_tts(lang, &choice), &mut problems);
    let dialogue = take(registry.resolve_dialogue(), &mut problems);
    let (Some(vad), Some(asr), Some(tts), Some(dialogue)) = (vad, asr, tts, dialogue) else {
        return Err(dedupe(problems.iter().map(issues::from_problem).collect()));
    };
    if problems
        .iter()
        .any(|p| p.slot == prxvc_providers::Slot::Fallback)
    {
        for warning in &problems {
            tracing::warn!(%warning, "a fallback provider cannot be used");
        }
    }

    // The local model server is started on demand for the dialogue (and stopped again when the
    // session ends and nothing else needs it).
    let server = crate::call_kit::model_server::hold(
        dialogue.primary.id.as_str(),
        &dialogue.primary.base_url,
    )
    .map_err(|detail| vec![issues::server_failed(&detail)])?;
    // A server that does not answer is reported now, before any model is loaded.
    if let Err(reason) = probe_address(&dialogue.primary.base_url, dialogue.primary.locality) {
        return Err(vec![issues::dialogue_down(
            &dialogue.primary.base_url,
            &reason,
        )]);
    }

    let build = |e: prxvc_providers::BuildError| vec![issues::from_build_error(&e)];
    let vad_p: Arc<dyn VadProvider> = Arc::from(registry.build_vad(&vad.primary).map_err(build)?);
    let asr_p: Arc<dyn StreamingAsr> = Arc::from(registry.build_asr(&asr.primary).map_err(build)?);
    let tts_p: Arc<dyn SpeechSynth> = Arc::from(registry.build_tts(&tts.primary).map_err(build)?);
    let dialogue_p: Arc<dyn DialogueBackend> =
        Arc::from(registry.build_dialogue(&dialogue.primary).map_err(build)?);
    Ok(Prepared {
        registry,
        vad: vad_p,
        asr: asr_p,
        tts: tts_p,
        dialogue: dialogue_p,
        lang,
        voice: tts.primary.voice.selection(),
        server,
    })
}

fn issues_of_fault(detail: &str) -> AssistantIssue {
    AssistantIssue {
        key: "assistant.issue.setting".to_owned(),
        detail: detail.to_owned(),
        action: prxvc_desktop_ui::call_view::IssueAction::OpenSpeechSettings,
    }
}

/// The resolved role, or its problems collected into `problems`.
fn take<S>(
    result: Result<prxvc_providers::Resolved<S>, Vec<Problem>>,
    problems: &mut Vec<Problem>,
) -> Option<prxvc_providers::Resolved<S>> {
    match result {
        Ok(mut resolved) => {
            problems.append(&mut resolved.warnings);
            Some(resolved)
        }
        Err(mut more) => {
            problems.append(&mut more);
            None
        }
    }
}

fn dedupe(issues: Vec<AssistantIssue>) -> Vec<AssistantIssue> {
    let mut out: Vec<AssistantIssue> = Vec::with_capacity(issues.len());
    for i in issues {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_in(dir: &std::path::Path, config: &AppConfig) -> AppPaths {
        AppPaths::from_roots(
            dir.join("config"),
            dir.join("data"),
            dir.join("data/models"),
            config,
        )
    }

    #[test]
    fn an_empty_installation_reports_what_is_missing_and_never_builds_anything() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let mut config = AppConfig::default();
        config.ui.language = "zh-CN".into();
        let paths = paths_in(dir.path(), &config);
        let issues = match prepare(&config, Some(&paths), None) {
            Ok(_) => panic!("nothing is installed, so nothing can be prepared"),
            Err(i) => i,
        };
        let keys: Vec<&str> = issues.iter().map(|i| i.key.as_str()).collect();
        assert!(keys.contains(&"assistant.issue.model_missing"), "{keys:?}");
        assert!(issues.iter().all(|i| !i.detail.is_empty()));
    }

    #[test]
    fn the_openprx_placeholder_is_reported_as_not_available_yet_next_to_the_other_problems() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let mut config = AppConfig::default();
        config
            .providers
            .active
            .dialogue
            .insert(String::new(), prxvc_providers::OPENPRX_PROVIDER.into());
        let paths = paths_in(dir.path(), &config);
        let Err(issues) = prepare(&config, Some(&paths), None) else {
            panic!("the placeholder cannot serve");
        };
        assert!(
            issues
                .iter()
                .any(|i| i.key == "assistant.issue.dialogue_not_available_yet"),
            "{issues:?}"
        );
    }
}
