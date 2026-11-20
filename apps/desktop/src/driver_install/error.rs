//! Typed failures of the driver operations, with a stable code and what to tell the user.

use std::fmt;

/// Why install, update or uninstall did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DriverInstallError {
    /// This build has no driver payload (not run from the app bundle, or payload absent).
    BundleNotFound,
    /// The bundled driver failed the checks before anything privileged ran.
    SourceRejected(String),
    /// The user dismissed the administrator prompt.
    Cancelled,
    /// The administrator prompt could not be shown or the credentials were refused.
    AuthorizationFailed(String),
    /// The privileged copy failed at a step (`stage`, `verify`, `swap` or `remove`).
    CopyFailed { step: String, detail: String },
    /// The files are in place but the audio daemon could not be restarted.
    RestartFailed(String),
    /// The device did not show up after the restart.
    DeviceDidNotAppear,
    /// After removal the device is still listed.
    DeviceStillPresent,
    /// Driver installation exists on macOS only.
    Unsupported,
}

impl DriverInstallError {
    /// Stable machine-readable code; the UI maps it to a translated message.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::BundleNotFound => "bundle_not_found",
            Self::SourceRejected(_) => "source_rejected",
            Self::Cancelled => "cancelled",
            Self::AuthorizationFailed(_) => "authorization_failed",
            Self::CopyFailed { .. } => "copy_failed",
            Self::RestartFailed(_) => "restart_failed",
            Self::DeviceDidNotAppear => "device_did_not_appear",
            Self::DeviceStillPresent => "device_still_present",
            Self::Unsupported => "unsupported",
        }
    }

    /// What to tell the user to do next (English, for logs and the debug view; the UI
    /// shows a translated text chosen by [`code`](Self::code)).
    pub(crate) fn advice(&self) -> &'static str {
        match self {
            Self::BundleNotFound => {
                "This copy of PRX Voice has no driver inside. Start the app from PRX Voice.app."
            }
            Self::SourceRejected(_) => {
                "The driver inside the app failed its checks. Reinstall PRX Voice from a fresh download."
            }
            Self::Cancelled => {
                "Nothing was changed. Try again and approve the administrator prompt."
            }
            Self::AuthorizationFailed(_) => {
                "The administrator authorization failed. Check the account password and that an administrator is signed in."
            }
            Self::CopyFailed { .. } => {
                "The driver could not be copied into the system. Nothing was started; try again, and report this if it repeats."
            }
            Self::RestartFailed(_) => {
                "The driver was copied but the audio service did not restart. Restart the Mac, then check again."
            }
            Self::DeviceDidNotAppear => {
                "The driver was copied but macOS did not load it. Restart the Mac; if the device is still missing, report it."
            }
            Self::DeviceStillPresent => {
                "The driver was removed but macOS still lists the device. Wait a few seconds or restart the Mac."
            }
            Self::Unsupported => "The virtual microphone driver is available on macOS only.",
        }
    }
}

impl fmt::Display for DriverInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BundleNotFound => write!(f, "no driver payload in this app"),
            Self::SourceRejected(why) => write!(f, "bundled driver rejected: {why}"),
            Self::Cancelled => write!(f, "cancelled by the user"),
            Self::AuthorizationFailed(why) => write!(f, "authorization failed: {why}"),
            Self::CopyFailed { step, detail } => write!(f, "copy failed at step {step}: {detail}"),
            Self::RestartFailed(why) => write!(f, "audio daemon restart failed: {why}"),
            Self::DeviceDidNotAppear => write!(f, "the device did not appear after the restart"),
            Self::DeviceStillPresent => write!(f, "the device is still listed after removal"),
            Self::Unsupported => write!(f, "not supported on this platform"),
        }
    }
}

impl std::error::Error for DriverInstallError {}
