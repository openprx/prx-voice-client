//! The assistant's problems and faults as the issues the orb stage shows: a message key, the English detail and the settings page that fixes it.
//!
//! Nothing is guessed: every typed problem of the provider registry, every build error, every
//! start error and every fault of the loop maps to one issue, and a kind this version does not know
//! becomes the generic `start_failed` with its detail, never a silence.

use prxvc_desktop_ui::AssistantIssue;
use prxvc_desktop_ui::call_view::IssueAction;
use prxvc_ports::{DialogueError, EndpointError};
use prxvc_providers::keys::CallRole;
use prxvc_providers::{BuildError, Problem, ProblemKind};
use prxvc_session::assistant::{AssistantFault, StartError};

fn make(key: &str, detail: impl Into<String>, action: IssueAction) -> AssistantIssue {
    AssistantIssue {
        key: format!("assistant.issue.{key}"),
        detail: detail.into(),
        action,
    }
}

/// Call translation holds the microphone.
pub(crate) fn call_running(detail: &str) -> AssistantIssue {
    make("call_running", detail, IssueAction::None)
}

/// The dialogue server is not accepting connections.
pub(crate) fn dialogue_down(base_url: &str, reason: &str) -> AssistantIssue {
    make(
        "dialogue_down",
        format!("the dialogue server at {base_url} does not answer: {reason}"),
        IssueAction::OpenAssistantSettings,
    )
}

/// The managed local model server could not be provided for the dialogue.
pub(crate) fn server_failed(detail: &str) -> AssistantIssue {
    make(
        "server_failed",
        detail.to_owned(),
        IssueAction::OpenAssistantSettings,
    )
}

/// The page a problem of `role` is fixed on when it is a matter of settings.
fn settings_action(role: Option<CallRole>) -> IssueAction {
    if role == Some(CallRole::Dialogue) {
        IssueAction::OpenAssistantSettings
    } else {
        IssueAction::OpenSpeechSettings
    }
}

/// A resolution problem as an issue.
pub(crate) fn from_problem(problem: &Problem) -> AssistantIssue {
    use ProblemKind as K;
    let role = problem.role.map(prxvc_providers::RoleKey::role);
    let detail = problem.to_string();
    let (key, action) = match &problem.kind {
        K::ModelNotInstalled { .. } | K::ModelFilesIncomplete { .. } => {
            ("model_missing", IssueAction::OpenSpeechSettings)
        }
        K::ModelInstalling { .. } => ("model_installing", IssueAction::OpenSpeechSettings),
        K::ModelFailedVerification { .. } | K::ModelFailed { .. } => {
            ("model_damaged", IssueAction::OpenSpeechSettings)
        }
        K::RuntimeNotBuiltIn { .. } => ("runtime_missing", IssueAction::OpenSpeechSettings),
        K::NotStreaming { .. } | K::LanguageUnsupported { .. } | K::PairUnsupported { .. } => {
            ("unsupported", settings_action(role))
        }
        K::EndpointBlocked { .. } => ("offline", IssueAction::OpenPermissions),
        K::NotAvailableYet { .. } => (
            "dialogue_not_available_yet",
            IssueAction::OpenAssistantSettings,
        ),
        K::VoiceProfileNotSet
        | K::VoiceProfileMissing { .. }
        | K::VoiceProfileUnreadable { .. }
        | K::VoicePresetUnknown { .. }
        | K::VoicePresetFileMissing { .. }
        | K::VoicePresetUnreadable { .. }
        | K::VoicePresetLanguage { .. } => ("voice_missing", IssueAction::OpenAssistantSettings),
        K::VoiceNotPrepared { .. } | K::VoicePreparedForOtherModel { .. } => {
            ("voice_not_prepared", IssueAction::OpenAssistantSettings)
        }
        K::InvalidLanguage { .. }
        | K::LanguageNotSet { .. }
        | K::InvalidSetting { .. }
        | K::NoProviderSelected
        | K::UnknownProvider { .. }
        | K::UnknownProtocol { .. }
        | K::UnknownTemplate { .. }
        | K::KindNotValidForRole { .. }
        | K::CredentialMissing { .. }
        | K::CredentialUnreadable { .. }
        | K::InvalidEndpoint { .. }
        | K::InvalidParams { .. }
        | K::FallbackIsPrimary { .. } => ("setting", settings_action(role)),
        _ => ("start_failed", IssueAction::OpenSpeechSettings),
    };
    make(key, detail, action)
}

/// A failure to build a provider as an issue.
pub(crate) fn from_build_error(error: &BuildError) -> AssistantIssue {
    let detail = error.to_string();
    match error {
        BuildError::NotBuiltIn { .. } => {
            make("runtime_missing", detail, IssueAction::OpenSpeechSettings)
        }
        BuildError::Blocked { .. } => make("offline", detail, IssueAction::OpenPermissions),
        BuildError::InvalidEndpoint { .. } => {
            make("setting", detail, IssueAction::OpenAssistantSettings)
        }
        BuildError::Voice { .. } => make(
            "voice_not_prepared",
            detail,
            IssueAction::OpenAssistantSettings,
        ),
        BuildError::Capability(_) => make("unsupported", detail, IssueAction::OpenSpeechSettings),
        _ => make("start_failed", detail, IssueAction::OpenSpeechSettings),
    }
}

/// A session that could not start as an issue.
pub(crate) fn from_start_error(error: &StartError) -> AssistantIssue {
    let detail = error.to_string();
    match error {
        StartError::Vad(_) | StartError::Recognition(_) => {
            make("listening_failed", detail, IssueAction::OpenSpeechSettings)
        }
        StartError::Microphone(_) => make(
            "microphone_unavailable",
            detail,
            IssueAction::OpenPermissions,
        ),
        StartError::Rate { .. } => make("microphone_rate", detail, IssueAction::OpenAudioSettings),
        _ => make("start_failed", detail, IssueAction::None),
    }
}

/// A fault of a running session as an issue.
pub(crate) fn from_fault(fault: &AssistantFault) -> AssistantIssue {
    let detail = fault.to_string();
    match fault {
        AssistantFault::Dialogue(e) => match e {
            DialogueError::Unavailable { .. } => {
                make("dialogue_down", detail, IssueAction::OpenAssistantSettings)
            }
            DialogueError::Timeout => make(
                "dialogue_timeout",
                detail,
                IssueAction::OpenAssistantSettings,
            ),
            DialogueError::Rejected { .. } => make(
                "dialogue_rejected",
                detail,
                IssueAction::OpenAssistantSettings,
            ),
            DialogueError::EmptyReply => make("dialogue_empty", detail, IssueAction::None),
            DialogueError::NotAvailableYet { .. } => make(
                "dialogue_not_available_yet",
                detail,
                IssueAction::OpenAssistantSettings,
            ),
            _ => make("start_failed", detail, IssueAction::None),
        },
        AssistantFault::Synthesis(_) => {
            make("synthesis_failed", detail, IssueAction::OpenSpeechSettings)
        }
        AssistantFault::NoAudio => make("no_audio", detail, IssueAction::None),
        AssistantFault::Listening { .. } => {
            make("listening_failed", detail, IssueAction::OpenSpeechSettings)
        }
        AssistantFault::Microphone(EndpointError::Unavailable { .. }) => make(
            "microphone_unavailable",
            detail,
            IssueAction::OpenPermissions,
        ),
        AssistantFault::Microphone(_) => {
            make("microphone_lost", detail, IssueAction::OpenAudioSettings)
        }
        AssistantFault::Output(_) => make("output_failed", detail, IssueAction::OpenAudioSettings),
        _ => make("start_failed", detail, IssueAction::None),
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::Lang;
    use prxvc_models::ModelId;
    use prxvc_ports::{AsrError, ProviderId, TtsError, VadError};
    use prxvc_providers::{RoleKey, Slot};

    use super::*;

    fn problem(role: RoleKey, kind: ProblemKind) -> Problem {
        Problem {
            role: Some(role),
            slot: Slot::Primary,
            kind,
        }
    }

    #[test]
    fn registry_problems_name_what_to_do_and_where() {
        let model = from_problem(&problem(
            RoleKey::Asr(Lang::Zh),
            ProblemKind::ModelNotInstalled {
                model: ModelId::new("qwen3-asr-0.6b-q8"),
                size_bytes: 1,
            },
        ));
        assert_eq!(model.key, "assistant.issue.model_missing");
        assert_eq!(model.action, IssueAction::OpenSpeechSettings);
        assert!(model.detail.contains("qwen3-asr-0.6b-q8"));

        let yet = from_problem(&problem(
            RoleKey::Dialogue,
            ProblemKind::NotAvailableYet {
                id: ProviderId::new("openprx"),
                name: "OpenPRX agent".into(),
            },
        ));
        assert_eq!(yet.key, "assistant.issue.dialogue_not_available_yet");
        assert_eq!(yet.action, IssueAction::OpenAssistantSettings);

        let setting = from_problem(&problem(RoleKey::Dialogue, ProblemKind::NoProviderSelected));
        assert_eq!(
            (setting.key.as_str(), setting.action),
            (
                "assistant.issue.setting",
                IssueAction::OpenAssistantSettings
            )
        );
        let setting = from_problem(&problem(RoleKey::Vad, ProblemKind::NoProviderSelected));
        assert_eq!(
            setting.action,
            IssueAction::OpenSpeechSettings,
            "a speech provider is chosen on the speech page"
        );

        let blocked = from_problem(&problem(
            RoleKey::Dialogue,
            ProblemKind::EndpointBlocked {
                entry: "cloud".into(),
                host: "api.example.com".into(),
            },
        ));
        assert_eq!(
            (blocked.key.as_str(), blocked.action),
            ("assistant.issue.offline", IssueAction::OpenPermissions)
        );
        let voice = from_problem(&problem(
            RoleKey::Tts(Lang::En),
            ProblemKind::VoiceProfileNotSet,
        ));
        assert_eq!(
            (voice.key.as_str(), voice.action),
            (
                "assistant.issue.voice_missing",
                IssueAction::OpenAssistantSettings
            )
        );
        let runtime = from_problem(&problem(
            RoleKey::Tts(Lang::En),
            ProblemKind::RuntimeNotBuiltIn {
                id: ProviderId::new("qwen3-tts"),
                runtime: "qwentts.cpp".into(),
            },
        ));
        assert_eq!(runtime.key, "assistant.issue.runtime_missing");
    }

    #[test]
    fn faults_of_the_dialogue_are_told_apart() {
        let key = |e: DialogueError| from_fault(&AssistantFault::Dialogue(e)).key;
        assert_eq!(
            key(DialogueError::Unavailable { reason: "x".into() }),
            "assistant.issue.dialogue_down"
        );
        assert_eq!(
            key(DialogueError::Timeout),
            "assistant.issue.dialogue_timeout"
        );
        assert_eq!(
            key(DialogueError::Rejected { status: 401 }),
            "assistant.issue.dialogue_rejected"
        );
        assert_eq!(
            key(DialogueError::EmptyReply),
            "assistant.issue.dialogue_empty"
        );
        assert_eq!(
            key(DialogueError::NotAvailableYet {
                backend: "openprx".into()
            }),
            "assistant.issue.dialogue_not_available_yet"
        );
    }

    #[test]
    fn every_other_fault_and_start_error_has_its_issue() {
        use prxvc_session::{FaultKind, Stage};
        let f = |f: AssistantFault| from_fault(&f);
        assert_eq!(
            f(AssistantFault::Synthesis(TtsError::Timeout)).key,
            "assistant.issue.synthesis_failed"
        );
        assert_eq!(f(AssistantFault::NoAudio).key, "assistant.issue.no_audio");
        assert_eq!(
            f(AssistantFault::Listening {
                stage: Stage::Vad,
                kind: FaultKind::Crashed,
                detail: "x".into()
            })
            .key,
            "assistant.issue.listening_failed"
        );
        let denied = f(AssistantFault::Microphone(EndpointError::Unavailable {
            reason: "denied".into(),
        }));
        assert_eq!(
            (denied.key.as_str(), denied.action),
            (
                "assistant.issue.microphone_unavailable",
                IssueAction::OpenPermissions
            )
        );
        let lost = f(AssistantFault::Microphone(EndpointError::DeviceLost {
            device: "usb".into(),
        }));
        assert_eq!(
            (lost.key.as_str(), lost.action),
            (
                "assistant.issue.microphone_lost",
                IssueAction::OpenAudioSettings
            )
        );
        assert_eq!(
            f(AssistantFault::Output("x".into())).key,
            "assistant.issue.output_failed"
        );

        let s = |e: StartError| from_start_error(&e);
        assert_eq!(
            s(StartError::Vad(VadError::Internal("x".into()))).key,
            "assistant.issue.listening_failed"
        );
        assert_eq!(
            s(StartError::Recognition(AsrError::Timeout)).key,
            "assistant.issue.listening_failed"
        );
        assert_eq!(
            s(StartError::Microphone(EndpointError::NotStarted)).action,
            IssueAction::OpenPermissions
        );
        assert_eq!(
            s(StartError::Thread("x".into())).key,
            "assistant.issue.start_failed"
        );
    }

    #[test]
    fn build_errors_and_the_down_server_say_what_is_wrong() {
        let id = ProviderId::new("p");
        assert_eq!(
            from_build_error(&BuildError::NotBuiltIn {
                provider: id.clone(),
                runtime: "transcribe.cpp".into()
            })
            .key,
            "assistant.issue.runtime_missing"
        );
        assert_eq!(
            from_build_error(&BuildError::Blocked {
                provider: id.clone(),
                host: "h".into()
            })
            .key,
            "assistant.issue.offline"
        );
        assert_eq!(
            from_build_error(&BuildError::Start {
                provider: id,
                detail: "boom".into()
            })
            .key,
            "assistant.issue.start_failed"
        );
        let down = dialogue_down("http://127.0.0.1:8080/v1", "connection refused");
        assert_eq!(down.key, "assistant.issue.dialogue_down");
        assert!(down.detail.contains("127.0.0.1:8080") && down.detail.contains("refused"));
        assert_eq!(down.action, IssueAction::OpenAssistantSettings);
        assert_eq!(call_running("x").action, IssueAction::None);
    }
}
