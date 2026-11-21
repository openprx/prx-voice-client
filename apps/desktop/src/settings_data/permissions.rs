//! Permission status, the requests the user triggers, and the System Settings panes.
//!
//! Status is read without asking (no probe in this file can show a system prompt):
//!
//! * Microphone: `prxvc_audio::mic_consent::permission_status()`, the authorization status of
//!   the capture device (macOS).
//! * Call audio: `prxvc_audio::tap_consent::permission_status()`, a pre-flight of the system's
//!   audio-capture permission (macOS). It goes through the private `TCCAccessPreflight` that
//!   `prxvc-audio` uses, which the design (3.5) had not wanted to depend on; a status it cannot
//!   give is "Not checked yet".
//!
//! The requests (`request_microphone`, `request_call_audio`) do show the system prompt and run
//! only when the user clicks "Allow" or "Check now", on a thread of their own.

use prxvc_desktop_ui::SystemPane;
use prxvc_desktop_ui::settings_data::{PermissionView, RequestOutcome};

/// The microphone permission now. Never prompts.
pub(crate) fn microphone_status() -> PermissionView {
    #[cfg(target_os = "macos")]
    {
        use prxvc_audio::mic_consent::{MicPermission, permission_status};
        match permission_status() {
            MicPermission::Granted => PermissionView::Allowed,
            MicPermission::Denied => PermissionView::NotAllowed,
            MicPermission::Undetermined => PermissionView::NotAsked,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionView::NotNeeded
    }
}

/// The system audio capture permission now. Never prompts.
pub(crate) fn call_audio_status() -> PermissionView {
    #[cfg(target_os = "macos")]
    {
        use prxvc_audio::tap_consent::{PermissionStatus, permission_status};
        match permission_status() {
            PermissionStatus::Granted => PermissionView::Allowed,
            PermissionStatus::Denied => PermissionView::NotAllowed,
            PermissionStatus::Undetermined => PermissionView::NotAsked,
            PermissionStatus::Unknown => PermissionView::NotChecked,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionView::NotNeeded
    }
}

/// How long a permission prompt may stay unanswered.
#[cfg(target_os = "macos")]
const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Ask for the microphone. Shows the system prompt when the status is not determined and the
/// process runs from a bundle that declares the usage; blocks until answered.
pub(crate) fn request_microphone() -> Option<RequestOutcome> {
    #[cfg(target_os = "macos")]
    {
        Some(microphone_outcome(
            prxvc_audio::mic_consent::ensure_microphone_consent(PROMPT_TIMEOUT),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Ask for system audio capture; blocks until answered.
pub(crate) fn request_call_audio() -> Option<RequestOutcome> {
    #[cfg(target_os = "macos")]
    {
        Some(call_audio_outcome(
            prxvc_audio::tap_consent::request_capture_consent(PROMPT_TIMEOUT),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The outcome of a microphone request (pure).
#[cfg(target_os = "macos")]
pub(crate) fn microphone_outcome(
    result: Result<(), prxvc_audio::mic_consent::MicConsentError>,
) -> RequestOutcome {
    use prxvc_audio::mic_consent::MicConsentError;
    match result {
        Ok(()) => RequestOutcome::Granted,
        Err(MicConsentError::Denied) => RequestOutcome::Refused,
        Err(MicConsentError::NotInAppBundle) => RequestOutcome::NotInBundle,
        Err(MicConsentError::NoAnswer { .. }) => RequestOutcome::NoAnswer,
    }
}

/// The outcome of a call-audio request (pure).
#[cfg(target_os = "macos")]
pub(crate) fn call_audio_outcome(
    result: Result<prxvc_audio::tap_consent::ConsentOutcome, prxvc_audio::tap_types::TapError>,
) -> RequestOutcome {
    use prxvc_audio::tap_consent::ConsentOutcome;
    use prxvc_audio::tap_types::TapError;
    match result {
        Ok(ConsentOutcome::Granted) => RequestOutcome::Granted,
        Ok(ConsentOutcome::Refused) | Err(TapError::PermissionDenied) => RequestOutcome::Refused,
        Err(TapError::NotInAppBundle) => RequestOutcome::NotInBundle,
        Err(_) => RequestOutcome::NoAnswer,
    }
}

/// The URL that opens the System Settings pane for `pane` on `os` (`"macos"` or `"windows"`);
/// `None` where the platform has no such pane.
///
/// The macOS anchors are the documented `x-apple.systempreferences:` scheme with the Privacy
/// pane ids `Privacy_Microphone` and `Privacy_ScreenCapture` (the pane that holds "System Audio
/// Recording Only" on macOS 14.4 and later). They were not opened by the developer of this code.
pub(crate) fn system_pane_url(pane: SystemPane, os: &str) -> Option<&'static str> {
    match (os, pane) {
        ("macos", SystemPane::Microphone) => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        ("macos", SystemPane::AudioCapture) => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        }
        ("macos", SystemPane::TranslationLanguages) => {
            // Language & Region; the Translation Languages list is a button on that pane.
            Some("x-apple.systempreferences:com.apple.Localization-Settings.extension")
        }
        ("windows", SystemPane::Microphone) => Some("ms-settings:privacy-microphone"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panes_have_urls_on_macos_and_the_microphone_on_windows() {
        assert!(
            system_pane_url(SystemPane::Microphone, "macos")
                .is_some_and(|u| u.ends_with("Privacy_Microphone"))
        );
        assert!(
            system_pane_url(SystemPane::AudioCapture, "macos")
                .is_some_and(|u| u.starts_with("x-apple.systempreferences:"))
        );
        assert_eq!(
            system_pane_url(SystemPane::Microphone, "windows"),
            Some("ms-settings:privacy-microphone")
        );
        assert_eq!(system_pane_url(SystemPane::AudioCapture, "windows"), None);
        assert_eq!(system_pane_url(SystemPane::Microphone, "linux"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn request_results_map_to_outcomes() {
        use prxvc_audio::mic_consent::MicConsentError;
        use prxvc_audio::tap_consent::ConsentOutcome;
        use prxvc_audio::tap_types::TapError;
        assert_eq!(microphone_outcome(Ok(())), RequestOutcome::Granted);
        assert_eq!(
            microphone_outcome(Err(MicConsentError::Denied)),
            RequestOutcome::Refused
        );
        assert_eq!(
            microphone_outcome(Err(MicConsentError::NotInAppBundle)),
            RequestOutcome::NotInBundle
        );
        assert_eq!(
            microphone_outcome(Err(MicConsentError::NoAnswer { secs: 1 })),
            RequestOutcome::NoAnswer
        );
        assert_eq!(
            call_audio_outcome(Ok(ConsentOutcome::Granted)),
            RequestOutcome::Granted
        );
        assert_eq!(
            call_audio_outcome(Ok(ConsentOutcome::Refused)),
            RequestOutcome::Refused
        );
        assert_eq!(
            call_audio_outcome(Err(TapError::PermissionDenied)),
            RequestOutcome::Refused
        );
        assert_eq!(
            call_audio_outcome(Err(TapError::NotInAppBundle)),
            RequestOutcome::NotInBundle
        );
        assert_eq!(
            call_audio_outcome(Err(TapError::ConsentNoAnswer { reason: "t".into() })),
            RequestOutcome::NoAnswer
        );
    }
}
