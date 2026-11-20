//! Microphone consent (macOS): status, the request path and the gate that decides whether call mode may open the microphone.
//!
//! macOS attributes microphone access to the application bundle. Without
//! consent the capture delivers silence without any error. The request shows
//! the system prompt, which needs `NSMicrophoneUsageDescription` in the
//! bundle's Info.plist (the native shim refuses to ask without it, because the
//! system would terminate the process). The decision logic ([`plan_request`])
//! is pure.

use std::time::Duration;

unsafe extern "C" {
    fn prxvc_mic_permission_status() -> i32;
    fn prxvc_mic_bundle_declares_usage() -> i32;
    fn prxvc_mic_request(timeout_ms: i32) -> i32;
}

const STATUS_GRANTED: i32 = 0;
const STATUS_DENIED: i32 = 1;
const STATUS_UNDETERMINED: i32 = 2;

/// The user's answer for the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicPermission {
    /// Allowed.
    Granted,
    /// Refused or restricted; the user must change it in System Settings.
    Denied,
    /// Not asked yet.
    Undetermined,
}

/// Why the microphone may not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MicConsentError {
    /// Refused earlier or by the user just now.
    #[error(
        "microphone access is not permitted: open System Settings, Privacy & Security, \
         Microphone, allow PRX Voice, then start again"
    )]
    Denied,
    /// The process runs without a bundle that declares the microphone usage key.
    #[error(
        "this process does not run from an application bundle that declares \
         NSMicrophoneUsageDescription, so macOS cannot ask for microphone permission: \
         start PRX Voice from its application bundle"
    )]
    NotInAppBundle,
    /// The prompt was not answered in time.
    #[error("the system microphone permission prompt was not answered within {secs} s")]
    NoAnswer {
        /// The time waited.
        secs: u64,
    },
}

/// The microphone permission now. Never prompts.
pub fn permission_status() -> MicPermission {
    // SAFETY: no arguments; the shim only reads the authorization status.
    match unsafe { prxvc_mic_permission_status() } {
        STATUS_GRANTED => MicPermission::Granted,
        STATUS_DENIED => MicPermission::Denied,
        STATUS_UNDETERMINED => MicPermission::Undetermined,
        _ => MicPermission::Denied,
    }
}

/// Whether the main bundle declares `NSMicrophoneUsageDescription`.
pub fn bundle_declares_usage() -> bool {
    // SAFETY: no arguments; reads the main bundle's Info.plist.
    unsafe { prxvc_mic_bundle_declares_usage() != 0 }
}

/// What a consent request does for a status and a bundle fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestPlan {
    /// Allowed already.
    AlreadyGranted,
    /// Ask the system.
    Ask,
    /// Fail without asking.
    Fail(MicConsentError),
}

/// Decide what [`ensure_microphone_consent`] does.
pub fn plan_request(status: MicPermission, declares_usage: bool) -> RequestPlan {
    match status {
        MicPermission::Granted => RequestPlan::AlreadyGranted,
        MicPermission::Denied => RequestPlan::Fail(MicConsentError::Denied),
        MicPermission::Undetermined if !declares_usage => {
            RequestPlan::Fail(MicConsentError::NotInAppBundle)
        }
        MicPermission::Undetermined => RequestPlan::Ask,
    }
}

/// Make sure the microphone may be used, asking the user when the status is
/// undetermined. Blocks until the user answers or `timeout` passes.
///
/// # Errors
/// [`MicConsentError`] when access is denied, cannot be asked for, or the
/// prompt is not answered in time.
pub fn ensure_microphone_consent(timeout: Duration) -> Result<(), MicConsentError> {
    match plan_request(permission_status(), bundle_declares_usage()) {
        RequestPlan::AlreadyGranted => return Ok(()),
        RequestPlan::Fail(e) => return Err(e),
        RequestPlan::Ask => {}
    }
    let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: plain integer argument; the shim owns everything it blocks on.
    match unsafe { prxvc_mic_request(timeout_ms) } {
        1 => Ok(()),
        0 => Err(MicConsentError::Denied),
        -2 => Err(MicConsentError::NotInAppBundle),
        _ => Err(MicConsentError::NoAnswer { secs: timeout.as_secs() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MicPermission::{Denied, Granted, Undetermined};

    #[test]
    fn the_plan_covers_every_status_and_bundle_fact() {
        assert_eq!(plan_request(Granted, false), RequestPlan::AlreadyGranted);
        assert_eq!(plan_request(Granted, true), RequestPlan::AlreadyGranted);
        assert_eq!(plan_request(Denied, true), RequestPlan::Fail(MicConsentError::Denied));
        assert_eq!(plan_request(Denied, false), RequestPlan::Fail(MicConsentError::Denied));
        assert_eq!(plan_request(Undetermined, true), RequestPlan::Ask);
        assert_eq!(
            plan_request(Undetermined, false),
            RequestPlan::Fail(MicConsentError::NotInAppBundle)
        );
    }

    #[test]
    fn errors_tell_the_user_what_to_do() {
        assert!(MicConsentError::Denied.to_string().contains("System Settings"));
        assert!(MicConsentError::NotInAppBundle.to_string().contains("application bundle"));
    }
}
