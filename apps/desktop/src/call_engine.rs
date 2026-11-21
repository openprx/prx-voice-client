//! The translation engine in the Call translation window: which engine the call uses, the actions
//! the window offers next to an engine problem, and what those actions do.
//!
//! The window's own files are not changed by this work: it receives `CallPanel::translation` (the
//! engine name and the actions) and pushes the `command::SWITCH_TO_*` and
//! `command::OPEN_TRANSLATION_LANGUAGES` commands, which the app intercepts here, on the UI
//! thread, before the commands go to the engine loop. A switch is an edit of the settings value
//! `call_translation_engine`, so it is saved, shown in the settings window and applied (the model
//! server started or stopped) exactly like a change made in the dropdown.

use std::sync::atomic::{AtomicU8, Ordering};

use prxvc_config::TranslationEngine;
use prxvc_desktop_ui::DesktopUiState;
use prxvc_desktop_ui::call_view::{CallPanel, EngineAction, IssueView, TranslationView, command};
use prxvc_desktop_ui::settings_command::SystemPane;

const UNKNOWN: u8 = 0;
const SYSTEM: u8 = 1;
const LLM: u8 = 2;

static ENGINE: AtomicU8 = AtomicU8::new(UNKNOWN);

/// Remember the configured engine (a value of `call.translation_engine`).
pub(crate) fn set(engine: &str) {
    let value = match TranslationEngine::parse(engine) {
        Some(TranslationEngine::System) => SYSTEM,
        Some(TranslationEngine::Llm) => LLM,
        None => UNKNOWN,
    };
    ENGINE.store(value, Ordering::Relaxed);
}

fn current() -> Option<TranslationEngine> {
    match ENGINE.load(Ordering::Relaxed) {
        SYSTEM => Some(TranslationEngine::System),
        LLM => Some(TranslationEngine::Llm),
        _ => None,
    }
}

/// The issue keys that concern the translation engine.
const ENGINE_ISSUES: [&str; 7] = [
    "call.issue.provider.language_pack_missing",
    "call.issue.provider.os_too_old",
    "call.issue.provider.server_failed",
    "call.issue.provider.server_unreachable",
    "call.issue.provider.runtime_not_built_in",
    "call.issue.provider.start_failed",
    "call.issue.provider.unsupported",
];

/// What the window shows about the engine for `issue_keys` (pure).
pub(crate) fn translation_view(
    engine: Option<TranslationEngine>,
    issue_keys: &[&str],
) -> TranslationView {
    let Some(engine) = engine else {
        return TranslationView::default();
    };
    let has = |key: &str| issue_keys.contains(&key);
    let engine_problem = issue_keys.iter().any(|k| ENGINE_ISSUES.contains(k));
    let mut actions = Vec::new();
    if engine_problem {
        match engine {
            TranslationEngine::System => {
                if has("call.issue.provider.language_pack_missing") {
                    actions.push(EngineAction::OpenTranslationLanguages);
                }
                actions.push(EngineAction::SwitchToLlm);
            }
            TranslationEngine::Llm => actions.push(EngineAction::SwitchToSystem),
        }
    }
    TranslationView {
        engine: engine.as_str().to_owned(),
        engine_key: match engine {
            TranslationEngine::System => "callwin.engine.system",
            TranslationEngine::Llm => "callwin.engine.llm",
        },
        no_context: engine == TranslationEngine::System,
        actions,
    }
}

/// Fill `panel.translation` from the configured engine and the panel's issues.
pub(crate) fn annotate(panel: &mut CallPanel) {
    let keys: Vec<&str> = panel
        .issues
        .iter()
        .chain(panel.last_error.iter())
        .map(|i: &IssueView| i.key.as_str())
        .collect();
    panel.translation = translation_view(current(), &keys);
}

/// Handle the engine commands the window pushed and keep the remembered engine in step with the
/// settings. Returns after removing the commands it handled from `state.pending_commands`.
pub(crate) fn intercept(state: &mut DesktopUiState) {
    let mut open_pane = false;
    state.pending_commands.retain(|cmd| match cmd.as_str() {
        command::SWITCH_TO_LLM => {
            state.settings.call_translation_engine = TranslationEngine::Llm.as_str().to_owned();
            false
        }
        command::SWITCH_TO_SYSTEM => {
            state.settings.call_translation_engine = TranslationEngine::System.as_str().to_owned();
            false
        }
        command::OPEN_TRANSLATION_LANGUAGES => {
            open_pane = true;
            false
        }
        _ => true,
    });
    if open_pane {
        crate::settings_host::open_pane(SystemPane::TranslationLanguages);
    }
    set(&state.settings.call_translation_engine);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACK: &str = "call.issue.provider.language_pack_missing";
    const SERVER: &str = "call.issue.provider.server_failed";

    #[test]
    fn a_missing_pack_offers_the_pane_and_the_language_model() {
        let v = translation_view(Some(TranslationEngine::System), &[PACK]);
        assert_eq!(
            v.actions,
            [
                EngineAction::OpenTranslationLanguages,
                EngineAction::SwitchToLlm
            ]
        );
        assert_eq!(v.engine_key, "callwin.engine.system");
        assert!(v.no_context);
    }

    #[test]
    fn an_old_system_offers_only_the_language_model() {
        let v = translation_view(
            Some(TranslationEngine::System),
            &["call.issue.provider.os_too_old"],
        );
        assert_eq!(v.actions, [EngineAction::SwitchToLlm]);
    }

    #[test]
    fn a_server_problem_offers_system_translation() {
        let v = translation_view(Some(TranslationEngine::Llm), &[SERVER]);
        assert_eq!(v.actions, [EngineAction::SwitchToSystem]);
        assert!(!v.no_context);
        assert_eq!(v.engine_key, "callwin.engine.llm");
    }

    #[test]
    fn no_engine_problem_no_action_and_unrelated_issues_are_not_engine_problems() {
        assert!(
            translation_view(Some(TranslationEngine::System), &[])
                .actions
                .is_empty()
        );
        assert!(
            translation_view(
                Some(TranslationEngine::Llm),
                &[
                    "call.issue.provider.voice_profile_missing",
                    "call.issue.speaker_output"
                ]
            )
            .actions
            .is_empty()
        );
        assert_eq!(translation_view(None, &[PACK]), TranslationView::default());
    }

    #[test]
    fn the_buttons_have_labels_and_commands() {
        for a in [
            EngineAction::SwitchToLlm,
            EngineAction::SwitchToSystem,
            EngineAction::OpenTranslationLanguages,
        ] {
            assert_ne!(
                prxvc_i18n::t(a.label_key()),
                a.label_key(),
                "{a:?} has a string"
            );
            assert!(a.command().starts_with("Call"));
        }
    }

    #[test]
    fn switching_edits_the_setting_and_passes_other_commands_on() {
        let mut state = DesktopUiState::new();
        state.settings.call_translation_engine = "system".into();
        state.pending_commands = vec![
            "CallStart".into(),
            command::SWITCH_TO_LLM.into(),
            "StopSession".into(),
        ];
        intercept(&mut state);
        assert_eq!(state.settings.call_translation_engine, "llm");
        assert_eq!(state.pending_commands, ["CallStart", "StopSession"]);
        assert_eq!(current(), Some(TranslationEngine::Llm));
        state.pending_commands = vec![command::SWITCH_TO_SYSTEM.into()];
        intercept(&mut state);
        assert_eq!(state.settings.call_translation_engine, "system");
        assert!(state.pending_commands.is_empty());
    }
}
