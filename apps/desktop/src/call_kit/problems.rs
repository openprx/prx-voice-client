//! The registry's problems and build errors as the call's typed provider issues, each with the detail that names what the user must act on.

use prxvc_providers::keys::CallRole;
use prxvc_providers::{BuildError, Problem, ProblemKind};
use prxvc_session::ProviderRole;
use prxvc_session::call::{CallErrorCode, HostFault, ProviderIssue};

fn issue_of(kind: &ProblemKind) -> ProviderIssue {
    use ProblemKind as K;
    match kind {
        K::ModelNotInstalled { .. } | K::ModelFilesIncomplete { .. } => {
            ProviderIssue::ModelNotInstalled
        }
        K::ModelInstalling { .. } => ProviderIssue::ModelInstalling,
        K::ModelFailedVerification { .. } | K::ModelFailed { .. } => ProviderIssue::ModelDamaged,
        K::RuntimeNotBuiltIn { .. } => ProviderIssue::RuntimeNotBuiltIn,
        K::NotStreaming { .. } | K::LanguageUnsupported { .. } | K::PairUnsupported { .. } => {
            ProviderIssue::Unsupported
        }
        K::EndpointBlocked { .. } => ProviderIssue::Offline,
        K::VoiceProfileNotSet
        | K::VoiceProfileMissing { .. }
        | K::VoiceProfileUnreadable { .. } => ProviderIssue::VoiceProfileMissing,
        K::VoiceNotPrepared { .. } | K::VoicePreparedForOtherModel { .. } => {
            ProviderIssue::VoiceNotPrepared
        }
        K::VoicePresetUnknown { .. }
        | K::VoicePresetFileMissing { .. }
        | K::VoicePresetUnreadable { .. }
        | K::VoicePresetLanguage { .. } => ProviderIssue::VoicePresetMissing,
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
        | K::FallbackIsPrimary { .. } => ProviderIssue::Setting,
        _ => ProviderIssue::StartFailed,
    }
}

/// The role a problem concerns; a problem of the call as a whole counts as recognition.
pub(crate) fn role_of(problem: &Problem) -> ProviderRole {
    match problem.role.map(prxvc_providers::RoleKey::role) {
        Some(CallRole::Vad) => ProviderRole::Vad,
        Some(CallRole::AsrRefine) => ProviderRole::AsrRefine,
        Some(CallRole::Mt) => ProviderRole::Mt,
        Some(CallRole::Tts) => ProviderRole::Tts,
        // The assistant's dialogue is resolved on its own and never in a call's resolution.
        Some(CallRole::Asr | CallRole::Dialogue) | None => ProviderRole::Asr,
    }
}

/// A resolution problem as a fault the snapshot carries.
pub(crate) fn problem_fault(problem: &Problem) -> HostFault {
    HostFault::new(
        CallErrorCode::Provider(issue_of(&problem.kind)),
        problem.to_string(),
    )
}

/// A build error as a fault.
pub(crate) fn build_fault(error: &BuildError) -> HostFault {
    use prxvc_adapters::mt::apple::SystemFaultKind as Fault;
    let issue = match error {
        BuildError::System { fault, .. } => match fault.kind {
            Fault::OsTooOld => ProviderIssue::OsTooOld,
            Fault::PackNotInstalled => ProviderIssue::LanguagePackMissing,
            Fault::PairUnsupported => ProviderIssue::Unsupported,
            _ => ProviderIssue::StartFailed,
        },
        BuildError::NotBuiltIn { .. } => ProviderIssue::RuntimeNotBuiltIn,
        BuildError::Blocked { .. } => ProviderIssue::Offline,
        BuildError::InvalidEndpoint { .. } => ProviderIssue::Setting,
        BuildError::Voice { .. } => ProviderIssue::VoiceNotPrepared,
        BuildError::Capability(_) => ProviderIssue::Unsupported,
        _ => ProviderIssue::StartFailed,
    };
    HostFault::new(CallErrorCode::Provider(issue), error.to_string())
}

/// The managed local model server could not be provided; `detail` is the manager's own sentence.
pub(crate) fn server_fault(detail: &str) -> HostFault {
    HostFault::new(
        CallErrorCode::Provider(ProviderIssue::ServerFailed),
        detail.to_owned(),
    )
}

/// The translation server does not answer at `base_url`.
pub(crate) fn unreachable_fault(base_url: &str, reason: &str) -> HostFault {
    HostFault::new(
        CallErrorCode::Provider(ProviderIssue::ServerUnreachable),
        format!("the translation server at {base_url} does not answer: {reason}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use prxvc_media::Lang;
    use prxvc_models::ModelId;
    use prxvc_providers::{RoleKey, Slot};

    fn problem(role: Option<RoleKey>, kind: ProblemKind) -> Problem {
        Problem {
            role,
            slot: Slot::Primary,
            kind,
        }
    }

    #[test]
    fn what_the_user_must_do_follows_from_the_kind() {
        let cases = [
            (
                ProblemKind::ModelNotInstalled {
                    model: ModelId::new("m"),
                    size_bytes: 1,
                },
                ProviderIssue::ModelNotInstalled,
            ),
            (
                ProblemKind::RuntimeNotBuiltIn {
                    id: prxvc_ports::ProviderId::new("p"),
                    runtime: "qwentts.cpp".into(),
                },
                ProviderIssue::RuntimeNotBuiltIn,
            ),
            (
                ProblemKind::VoiceNotPrepared {
                    profile_id: "owner".into(),
                },
                ProviderIssue::VoiceNotPrepared,
            ),
            (
                ProblemKind::VoiceProfileNotSet,
                ProviderIssue::VoiceProfileMissing,
            ),
            (
                ProblemKind::VoicePresetUnknown {
                    name: String::new(),
                },
                ProviderIssue::VoicePresetMissing,
            ),
            (
                ProblemKind::InvalidLanguage {
                    key: "k".into(),
                    value: "x".into(),
                },
                ProviderIssue::Setting,
            ),
        ];
        for (kind, issue) in cases {
            let fault = problem_fault(&problem(None, kind));
            assert_eq!(fault.code, CallErrorCode::Provider(issue));
            assert!(!fault.detail.is_empty());
        }
    }

    #[test]
    fn the_role_comes_from_the_problem_and_a_call_level_problem_counts_as_recognition() {
        let p = problem(
            Some(RoleKey::Tts(Lang::En)),
            ProblemKind::VoiceProfileNotSet,
        );
        assert_eq!(role_of(&p), ProviderRole::Tts);
        let p = problem(
            Some(RoleKey::Mt {
                source: Lang::Zh,
                target: Lang::En,
            }),
            ProblemKind::NoProviderSelected,
        );
        assert_eq!(role_of(&p), ProviderRole::Mt);
        assert_eq!(
            role_of(&problem(None, ProblemKind::NoProviderSelected)),
            ProviderRole::Asr
        );
    }

    #[test]
    fn system_translation_problems_say_what_the_user_must_do() {
        use prxvc_adapters::mt::apple::{SystemFault, SystemFaultKind as K};
        let cases = [
            (K::PackNotInstalled, ProviderIssue::LanguagePackMissing),
            (K::OsTooOld, ProviderIssue::OsTooOld),
            (K::PairUnsupported, ProviderIssue::Unsupported),
            (K::Framework, ProviderIssue::StartFailed),
            (K::Timeout, ProviderIssue::StartFailed),
        ];
        for (kind, issue) in cases {
            let fault = build_fault(&BuildError::System {
                provider: prxvc_ports::ProviderId::new("apple-system-translation"),
                fault: SystemFault::new(kind, "detail"),
            });
            assert_eq!(fault.code, CallErrorCode::Provider(issue), "{kind:?}");
            assert!(fault.detail.contains("detail"));
        }
    }

    #[test]
    fn an_unreachable_server_names_its_address() {
        let fault = unreachable_fault("http://127.0.0.1:8080/v1", "connection refused");
        assert_eq!(
            fault.code,
            CallErrorCode::Provider(ProviderIssue::ServerUnreachable)
        );
        assert!(fault.detail.contains("127.0.0.1:8080"));
    }
}
