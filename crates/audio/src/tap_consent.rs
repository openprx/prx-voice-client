//! System audio recording consent for the process tap (macOS): status, the request path and the gate that decides whether a tap may start.
//!
//! A tap of another application's audio needs the user's "System Audio
//! Recording" consent for the capturing application. Without it every Core
//! Audio call succeeds and the tap delivers exact digital zeros. macOS can only
//! show the consent prompt to an application bundle whose Info.plist declares
//! `NSAudioCaptureUsageDescription`; from a bare binary the request is refused
//! without a prompt (bench/callapp REPORT, addenda A and D).
//!
//! The status comes from the private `TCCAccessPreflight` and the request from
//! the private `TCCAccessRequest` (service `kTCCServiceAudioCapture`), the calls
//! the reference application uses; both are looked up at run time in the native
//! shim. The decision logic ([`plan_request`], [`start_gate`]) is pure.

use std::time::Duration;

use crate::tap_types::TapError;

unsafe extern "C" {
    fn prxvc_tap_permission_status() -> i32;
    fn prxvc_tap_bundle_declares_audio_capture() -> i32;
    fn prxvc_tap_request_consent(timeout_ms: i32) -> i32;
}

const PERMISSION_GRANTED: i32 = 0;
const PERMISSION_DENIED: i32 = 1;
const PERMISSION_UNDETERMINED: i32 = 2;
const CONSENT_GRANTED: i32 = 1;
const CONSENT_REFUSED: i32 = 0;
const CONSENT_UNAVAILABLE: i32 = -2;

/// The user's answer for system audio recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    /// Allowed.
    Granted,
    /// Refused; the tap would deliver silence.
    Denied,
    /// Not asked yet; [`request_capture_consent`] shows the system prompt.
    Undetermined,
    /// The system gives no answer.
    Unknown,
}

/// The user's answer to a consent request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentOutcome {
    /// Allowed (now or before).
    Granted,
    /// The user refused; the system does not ask again, the user must change it
    /// in System Settings.
    Refused,
}

/// Ask the system whether system audio recording is allowed. Never prompts.
pub fn permission_status() -> PermissionStatus {
    // SAFETY: no arguments; the shim only reads the permission database.
    match unsafe { prxvc_tap_permission_status() } {
        PERMISSION_GRANTED => PermissionStatus::Granted,
        PERMISSION_DENIED => PermissionStatus::Denied,
        PERMISSION_UNDETERMINED => PermissionStatus::Undetermined,
        _ => PermissionStatus::Unknown,
    }
}

/// Whether this process runs from an application bundle whose Info.plist
/// declares `NSAudioCaptureUsageDescription`, the precondition for the system
/// to show the consent prompt.
pub fn bundle_declares_capture_usage() -> bool {
    // SAFETY: no arguments; reads the main bundle's Info.plist.
    unsafe { prxvc_tap_bundle_declares_audio_capture() != 0 }
}

/// What a consent request should do for a given status and bundle fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestPlan {
    /// Nothing to ask: already allowed.
    AlreadyGranted,
    /// Ask the system; it shows the prompt.
    Ask,
    /// Fail with this error without calling the system.
    Fail(PlanFailure),
}

/// Why a request is not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanFailure {
    /// The bundle does not declare the usage key.
    NotInAppBundle,
    /// The user refused before; the system never asks twice.
    Denied,
}

impl From<PlanFailure> for TapError {
    fn from(f: PlanFailure) -> Self {
        match f {
            PlanFailure::NotInAppBundle => TapError::NotInAppBundle,
            PlanFailure::Denied => TapError::PermissionDenied,
        }
    }
}

/// Decide what [`request_capture_consent`] does. An unknown status is treated
/// like an undetermined one (the system's own answer decides).
pub(crate) fn plan_request(status: PermissionStatus, declares_usage: bool) -> RequestPlan {
    match status {
        PermissionStatus::Granted => RequestPlan::AlreadyGranted,
        _ if !declares_usage => RequestPlan::Fail(PlanFailure::NotInAppBundle),
        PermissionStatus::Denied => RequestPlan::Fail(PlanFailure::Denied),
        PermissionStatus::Undetermined | PermissionStatus::Unknown => RequestPlan::Ask,
    }
}

/// Ask the user for system audio recording consent.
///
/// Shows the system prompt when the status is undetermined and the process runs
/// from a bundle that declares `NSAudioCaptureUsageDescription`. Blocks until
/// the user answers or `timeout` passes, so call it from a worker thread, never
/// the UI thread, and never from a test or probe that must not prompt.
///
/// # Errors
/// [`TapError::NotInAppBundle`] outside such a bundle (nothing is asked),
/// [`TapError::PermissionDenied`] when the user refused earlier (the system does
/// not ask again), [`TapError::ConsentNoAnswer`] on timeout or when the system
/// function is missing.
pub fn request_capture_consent(timeout: Duration) -> Result<ConsentOutcome, TapError> {
    match plan_request(permission_status(), bundle_declares_capture_usage()) {
        RequestPlan::AlreadyGranted => return Ok(ConsentOutcome::Granted),
        RequestPlan::Fail(f) => return Err(f.into()),
        RequestPlan::Ask => {}
    }
    let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: plain integer argument; the shim owns everything it blocks on.
    match unsafe { prxvc_tap_request_consent(timeout_ms) } {
        CONSENT_GRANTED => Ok(ConsentOutcome::Granted),
        CONSENT_REFUSED => Ok(ConsentOutcome::Refused),
        CONSENT_UNAVAILABLE => Err(TapError::ConsentNoAnswer {
            reason: "the system permission request function is not available".into(),
        }),
        _ => Err(TapError::ConsentNoAnswer {
            reason: format!("no answer within {} s", timeout.as_secs()),
        }),
    }
}

/// Decide whether a tap may start for the given status and bundle fact.
///
/// * Granted, or unknown (the system gives no answer; the zero watchdog then
///   catches a missing consent): start.
/// * Not granted outside a declaring bundle: [`TapError::NotInAppBundle`], since
///   neither a prompt nor the settings list can help there.
/// * Denied: [`TapError::PermissionDenied`] with the settings instruction.
/// * Undetermined: [`TapError::ConsentUndetermined`]; the caller runs
///   [`request_capture_consent`] first.
pub(crate) fn start_gate(status: PermissionStatus, declares_usage: bool) -> Result<(), TapError> {
    match status {
        PermissionStatus::Granted | PermissionStatus::Unknown => Ok(()),
        _ if !declares_usage => Err(TapError::NotInAppBundle),
        PermissionStatus::Denied => Err(TapError::PermissionDenied),
        PermissionStatus::Undetermined => Err(TapError::ConsentUndetermined),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PermissionStatus::{Denied, Granted, Undetermined, Unknown};

    #[test]
    fn request_plan_covers_every_status_and_bundle_fact() {
        let cases = [
            (Granted, true, RequestPlan::AlreadyGranted),
            (Granted, false, RequestPlan::AlreadyGranted),
            (Undetermined, true, RequestPlan::Ask),
            (Unknown, true, RequestPlan::Ask),
            (Denied, true, RequestPlan::Fail(PlanFailure::Denied)),
            (
                Undetermined,
                false,
                RequestPlan::Fail(PlanFailure::NotInAppBundle),
            ),
            (
                Unknown,
                false,
                RequestPlan::Fail(PlanFailure::NotInAppBundle),
            ),
            (
                Denied,
                false,
                RequestPlan::Fail(PlanFailure::NotInAppBundle),
            ),
        ];
        for (status, bundle, expected) in cases {
            assert_eq!(
                plan_request(status, bundle),
                expected,
                "{status:?} bundle={bundle}"
            );
        }
    }

    #[test]
    fn plan_failures_map_to_the_typed_errors() {
        assert_eq!(
            TapError::from(PlanFailure::NotInAppBundle),
            TapError::NotInAppBundle
        );
        assert_eq!(
            TapError::from(PlanFailure::Denied),
            TapError::PermissionDenied
        );
    }

    #[test]
    fn start_gate_returns_the_typed_error_for_each_state() {
        assert_eq!(start_gate(Granted, true), Ok(()));
        assert_eq!(
            start_gate(Granted, false),
            Ok(()),
            "a granted terminal may be used for development"
        );
        assert_eq!(start_gate(Unknown, false), Ok(()));
        assert_eq!(start_gate(Denied, true), Err(TapError::PermissionDenied));
        assert_eq!(
            start_gate(Undetermined, true),
            Err(TapError::ConsentUndetermined)
        );
        assert_eq!(
            start_gate(Undetermined, false),
            Err(TapError::NotInAppBundle)
        );
        assert_eq!(start_gate(Denied, false), Err(TapError::NotInAppBundle));
    }

    #[test]
    fn denied_error_carries_the_settings_instruction() {
        let text = start_gate(Denied, true).unwrap_err().to_string();
        assert!(text.contains("System Settings") && text.contains("System Audio Recording"));
    }
}
