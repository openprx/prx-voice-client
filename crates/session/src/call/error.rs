//! Typed error codes of call mode and the issues the snapshot carries.
//!
//! The coordinator and its host speak in [`CallErrorCode`]s, never in text:
//! the machines branch on the mapping to their own vocabulary
//! ([`CallErrorCode::blocker`], [`CallErrorCode::fault_kind`]) and the UI maps a
//! code to a message through the i18n catalogue. The `detail` of a fault is
//! English text for the debug view and the log only.

use prxvc_media::MonoTime;

use crate::fault::{Blocker, Endpoint, FaultKind};

/// What went wrong, as a closed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CallErrorCode {
    // Start-up checks (PD-4.1 steps 1 to 4).
    /// The process does not run from the application bundle.
    NotInBundle,
    /// No call application is selected.
    NoCallApp,
    /// Microphone access is denied.
    MicrophoneConsentDenied,
    /// Microphone access cannot be asked for or was not answered.
    MicrophoneConsentUnavailable,
    /// System audio recording is denied.
    CaptureConsentDenied,
    /// System audio recording cannot be asked for or was not answered.
    CaptureConsentUnavailable,
    /// The virtual microphone driver is not installed.
    DriverMissing,
    /// The driver is installed but the audio service has not loaded it, or it is not alive.
    DriverNotLoaded,
    /// The installed driver is not the version this app expects.
    DriverStale,
    /// The installed driver is not the variant the call application needs.
    DriverWrongVariant,
    /// The driver state cannot be read.
    DriverUnusable,
    /// No physical microphone can be used.
    NoMicrophone,
    // Start-up steps.
    /// An endpoint could not be opened or started.
    EndpointOpenFailed(Endpoint),
    /// The call application is not running (the tap has no process).
    TargetNotRunning,
    /// A routing journal of another run stands in the way.
    RoutingJournalPresent,
    /// The system refused or did not apply the switch of the default input.
    RoutingSwitchFailed,
    /// The system audio settings cannot be read or written.
    RoutingUnavailable,
    // While running.
    /// The virtual microphone disappeared.
    VirtualMicrophoneLost,
    /// The default input is no longer our virtual microphone.
    DefaultInputDisplaced,
    /// The tap delivers only silence while the call application plays.
    CaptureNoSignal,
    /// The tap stopped delivering audio.
    CaptureStalled,
    /// The call application quit.
    TargetGone,
    /// An endpoint was lost while running.
    EndpointLost(Endpoint),
    /// No microphone is available now; capture resumes when one appears.
    MicrophoneUnavailable,
    /// The microphone changed while running (information).
    MicrophoneChanged,
    /// Endpoints did not come back in time.
    RecoveryTimeout,
    /// The call application records from another microphone than ours.
    CallOnOtherMicrophone,
    /// Call detection is not possible.
    ActivityUnavailable,
    /// A direction runs in a reduced way.
    DirectionDegraded,
    // Stop.
    /// The default input could not be restored; the journal stays for the next start.
    RestoreFailed,
    /// Something was still running after the stop timeout.
    StopLeak,
    // Commands.
    /// The call machine does not accept the command in its current state.
    CommandRejected,
    /// An assistant session owns the microphone.
    AssistantActive,
    /// Translation is not available in this build.
    TranslationUnavailable,
    /// A translation provider cannot be used; the call runs without translation.
    Provider(ProviderIssue),
    /// The local output is the built-in loudspeaker: the incoming translation would be heard by
    /// the microphone and translated back. Headphones are required (design section 6).
    SpeakerOutput,
    /// A failure with no better code.
    Internal,
}

/// Why a translation provider cannot be used, as the user must act on it. The English `detail` of
/// the issue names the model, file or address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ProviderIssue {
    /// A model is not in the model store: install it.
    ModelNotInstalled,
    /// A model is still being downloaded or verified: wait.
    ModelInstalling,
    /// A model is installed but damaged or unusable: reinstall it.
    ModelDamaged,
    /// The runtime of a provider is not built into this application: use a build that has it.
    RuntimeNotBuiltIn,
    /// The owner's voice profile does not exist or cannot be read: enrol the voice.
    VoiceProfileMissing,
    /// The owner's voice has not been prepared for the synthesis model: prepare it.
    VoiceNotPrepared,
    /// The preset voice of the incoming direction is missing: install a preset.
    VoicePresetMissing,
    /// The translation server does not answer: start it.
    ServerUnreachable,
    /// A provider does not support a language, a pair or live use: choose another.
    Unsupported,
    /// Offline mode blocks a provider: allow it or choose a local one.
    Offline,
    /// A setting is wrong: correct it.
    Setting,
    /// A provider failed to start for another reason.
    StartFailed,
}

/// How serious an issue is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Call mode cannot run or stopped.
    Blocking,
    /// Call mode runs in a reduced way or something needs the user's action.
    Degraded,
    /// Worth showing, nothing to do.
    Info,
}

impl CallErrorCode {
    /// The class of failure for the machines.
    pub const fn fault_kind(self) -> FaultKind {
        match self {
            Self::MicrophoneConsentDenied
            | Self::MicrophoneConsentUnavailable
            | Self::CaptureConsentDenied
            | Self::CaptureConsentUnavailable => FaultKind::PermissionDenied,
            Self::RecoveryTimeout | Self::CaptureStalled => FaultKind::Timeout,
            Self::Internal => FaultKind::Crashed,
            Self::CommandRejected => FaultKind::Rejected,
            _ => FaultKind::Unavailable,
        }
    }

    /// The start-up blocker the call machine files this failure under.
    ///
    /// The machine has no blocker for "no call application selected" or "the
    /// call application is not running"; both are the remote input endpoint
    /// failing to start.
    pub const fn blocker(self) -> Blocker {
        let kind = self.fault_kind();
        match self {
            Self::NotInBundle => Blocker::NotInBundle,
            Self::MicrophoneConsentDenied
            | Self::MicrophoneConsentUnavailable
            | Self::CaptureConsentDenied
            | Self::CaptureConsentUnavailable => Blocker::PermissionDenied,
            Self::DriverMissing
            | Self::DriverNotLoaded
            | Self::DriverStale
            | Self::DriverWrongVariant
            | Self::DriverUnusable => Blocker::DriverUnusable,
            Self::NoMicrophone => Blocker::NoMicrophone,
            Self::RoutingJournalPresent => Blocker::RoutingUnsafe,
            Self::EndpointOpenFailed(ep) => Blocker::EndpointFailed(ep, kind),
            Self::NoCallApp | Self::TargetNotRunning => {
                Blocker::EndpointFailed(Endpoint::RemoteInput, kind)
            }
            _ => Blocker::RoutingFailed(kind),
        }
    }

    /// The default severity of an issue with this code.
    pub const fn severity(self) -> Severity {
        match self {
            Self::NotInBundle
            | Self::NoCallApp
            | Self::MicrophoneConsentDenied
            | Self::MicrophoneConsentUnavailable
            | Self::CaptureConsentDenied
            | Self::CaptureConsentUnavailable
            | Self::DriverMissing
            | Self::DriverNotLoaded
            | Self::DriverStale
            | Self::DriverWrongVariant
            | Self::DriverUnusable
            | Self::NoMicrophone
            | Self::EndpointOpenFailed(_)
            | Self::TargetNotRunning
            | Self::RoutingJournalPresent
            | Self::RoutingSwitchFailed
            | Self::RoutingUnavailable
            | Self::VirtualMicrophoneLost
            | Self::EndpointLost(_)
            | Self::RecoveryTimeout
            | Self::RestoreFailed
            | Self::StopLeak
            | Self::AssistantActive
            | Self::TranslationUnavailable
            | Self::Internal => Severity::Blocking,
            Self::MicrophoneChanged | Self::CommandRejected | Self::ActivityUnavailable => {
                Severity::Info
            }
            _ => Severity::Degraded,
        }
    }
}

/// A failure reported by the host or a path: a code and English detail text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostFault {
    /// What went wrong.
    pub code: CallErrorCode,
    /// Detail for the log and the debug view; never matched on.
    pub detail: String,
}

impl HostFault {
    /// A fault with a code and detail.
    pub fn new(code: CallErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for HostFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}

/// A condition shown to the user: the typed code with its detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallIssue {
    /// The code.
    pub code: CallErrorCode,
    /// How serious.
    pub severity: Severity,
    /// English detail for the debug view.
    pub detail: String,
    /// When it arose, on the coordinator's clock.
    pub since: MonoTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_codes_are_permission_blockers() {
        for code in [
            CallErrorCode::MicrophoneConsentDenied,
            CallErrorCode::CaptureConsentUnavailable,
        ] {
            assert_eq!(code.blocker(), Blocker::PermissionDenied);
            assert_eq!(code.fault_kind(), FaultKind::PermissionDenied);
            assert_eq!(code.severity(), Severity::Blocking);
        }
    }

    #[test]
    fn every_driver_code_is_the_driver_blocker() {
        for code in [
            CallErrorCode::DriverMissing,
            CallErrorCode::DriverNotLoaded,
            CallErrorCode::DriverStale,
            CallErrorCode::DriverWrongVariant,
            CallErrorCode::DriverUnusable,
        ] {
            assert_eq!(code.blocker(), Blocker::DriverUnusable);
        }
    }

    #[test]
    fn endpoint_codes_keep_their_endpoint() {
        let code = CallErrorCode::EndpointOpenFailed(Endpoint::RemoteOutput);
        assert_eq!(
            code.blocker(),
            Blocker::EndpointFailed(Endpoint::RemoteOutput, FaultKind::Unavailable)
        );
        assert_eq!(
            CallErrorCode::NoCallApp.blocker(),
            Blocker::EndpointFailed(Endpoint::RemoteInput, FaultKind::Unavailable)
        );
    }

    #[test]
    fn reduced_operation_is_degraded_and_notes_are_info() {
        assert_eq!(
            CallErrorCode::DefaultInputDisplaced.severity(),
            Severity::Degraded
        );
        assert_eq!(
            CallErrorCode::CaptureNoSignal.severity(),
            Severity::Degraded
        );
        assert_eq!(CallErrorCode::MicrophoneChanged.severity(), Severity::Info);
        assert_eq!(
            CallErrorCode::VirtualMicrophoneLost.severity(),
            Severity::Blocking
        );
    }
}
