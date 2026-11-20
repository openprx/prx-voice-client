//! Public types of the process tap: target, mute mode, configuration, error, events and counters.

use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

/// What happens to the tapped application's own playback while it is tapped.
///
/// Values match `CATapMuteBehavior` in the Core Audio headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TapMuteMode {
    /// The application stays audible (the default everywhere for now).
    #[default]
    Unmuted,
    /// The application is silenced locally; only the tap hears it. Silence
    /// persists if our process dies while the tap exists, so callers opt in.
    Muted,
    /// Silenced locally only while another client (this one) reads the tap, so
    /// the application is audible again if we die.
    MutedWhenTapped,
}

impl TapMuteMode {
    /// The `CATapMuteBehavior` value passed to the native shim.
    pub fn to_native(self) -> i32 {
        match self {
            Self::Unmuted => 0,
            Self::Muted => 1,
            Self::MutedWhenTapped => 2,
        }
    }

    /// Parse the command-line spelling (`unmuted`, `muted`, `muted-when-tapped`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unmuted" => Some(Self::Unmuted),
            "muted" => Some(Self::Muted),
            "muted-when-tapped" => Some(Self::MutedWhenTapped),
            _ => None,
        }
    }
}

/// The one application whose audio is tapped (an include list of one process).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapTarget {
    /// A running process by pid. If it exits the tap cannot follow it.
    Pid(i32),
    /// An application by bundle ID. The tap survives restarts of the
    /// application: natively on macOS 26 and later, by re-creation elsewhere.
    BundleId(String),
}

impl TapTarget {
    /// Short text for logs and events.
    pub fn describe(&self) -> String {
        match self {
            Self::Pid(pid) => format!("pid {pid}"),
            Self::BundleId(id) => id.clone(),
        }
    }
}

/// Everything chosen by the caller when creating a tap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapConfig {
    /// The tapped application.
    pub target: TapTarget,
    /// Mute mode; [`TapMuteMode::Unmuted`] unless the caller opts in.
    pub mute: TapMuteMode,
}

impl TapConfig {
    /// An unmuted tap of one process.
    pub fn for_pid(pid: i32) -> Self {
        Self { target: TapTarget::Pid(pid), mute: TapMuteMode::Unmuted }
    }

    /// An unmuted tap of one application by bundle ID.
    pub fn for_bundle(bundle_id: impl Into<String>) -> Self {
        Self { target: TapTarget::BundleId(bundle_id.into()), mute: TapMuteMode::Unmuted }
    }
}

/// Why a tap could not be created or started.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TapError {
    /// The system does not provide process taps (before macOS 14.4) or this
    /// build is not for macOS.
    #[error("capturing another application's audio needs macOS 14.4 or later")]
    UnsupportedOs,
    /// The user has not allowed system audio recording for this application.
    #[error(
        "system audio recording is not permitted: open System Settings, Privacy & Security, \
         Screen & System Audio Recording, allow \"System Audio Recording Only\" for this \
         application, then start again"
    )]
    PermissionDenied,
    /// The process does not run from an application bundle that declares
    /// `NSAudioCaptureUsageDescription`, so macOS refuses to ask for consent
    /// and a tap would deliver silence.
    #[error(
        "this process does not run from an application bundle that declares \
         NSAudioCaptureUsageDescription, so macOS cannot ask for system audio recording \
         permission: start PRX Voice from its application bundle"
    )]
    NotInAppBundle,
    /// System audio recording has not been asked for yet; call
    /// `request_capture_consent` (from a worker thread) before starting the tap.
    #[error(
        "system audio recording permission has not been requested yet: allow the \
         system prompt for PRX Voice, then start again"
    )]
    ConsentUndetermined,
    /// The system gave no answer to the permission request.
    #[error("the system gave no answer to the system audio recording permission request: {reason}")]
    ConsentNoAnswer {
        /// Why there was no answer (timeout or missing system function).
        reason: String,
    },
    /// The target plays audio but the tap has delivered only exact digital
    /// zeros, even after one re-creation: permission is probably missing.
    #[error(
        "no signal from {target}: it is playing audio but the capture delivers only silence; \
         open System Settings, Privacy & Security, Screen & System Audio Recording, allow \
         \"System Audio Recording Only\" for PRX Voice (run from its application bundle), \
         then start again"
    )]
    NoSignal {
        /// The tapped application.
        target: String,
    },
    /// The target application has no audio process (it is not running).
    #[error("{target} is not running or has no audio process")]
    ProcessNotFound {
        /// The requested target.
        target: String,
    },
    /// The tap delivers a sample format the client does not handle.
    #[error("unsupported tap audio format: {detail}")]
    UnsupportedFormat {
        /// What was wrong.
        detail: String,
    },
    /// A Core Audio call failed for a reason other than permission.
    #[error("Core Audio {operation} failed (status {status})")]
    CoreAudio {
        /// The failing step.
        operation: String,
        /// The `OSStatus`.
        status: i32,
    },
    /// An internal failure (allocation, thread start).
    #[error("internal tap error: {0}")]
    Internal(String),
}

/// A change in the tap's health, reported from the supervisor thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapEvent {
    /// The tap is running. `restored` is true when it was re-created after a loss.
    Started {
        /// Whether this follows a loss.
        restored: bool,
        /// Whether the bundle-ID form with process restore is in use.
        bundle_form: bool,
    },
    /// The target application is no longer running.
    TargetGone,
    /// The target runs but no audio buffers arrived for `idle_ms`.
    Stalled {
        /// Milliseconds without a buffer.
        idle_ms: u64,
    },
    /// A re-creation attempt failed; it is retried while the target runs.
    RestartFailed {
        /// The failure.
        error: TapError,
    },
    /// The target's output is running but the tap has delivered only exact
    /// digital zeros for `silent_ms`: audio-capture consent is probably missing
    /// or the tap is stale. The tap is rebuilt once after this event.
    SilentWhilePlaying {
        /// Milliseconds of exact zeros while the output ran.
        silent_ms: u64,
    },
    /// The zeros persisted after the rebuild: capture is in an error state the
    /// UI shows with the consent instructions (the error is
    /// [`TapError::NoSignal`]). Reported once; [`TapEvent::SignalRestored`]
    /// clears it.
    NoSignal {
        /// The typed error to show.
        error: TapError,
    },
    /// Real audio arrived again after [`TapEvent::SilentWhilePlaying`] or
    /// [`TapEvent::NoSignal`].
    SignalRestored,
    /// Buffers whose layout differs from the validated format were dropped.
    FormatMismatch {
        /// Total dropped buffers so far.
        dropped: u64,
    },
}

/// Counters shared between the real-time callback (writer) and other threads
/// (readers). Only relaxed atomics, so the callback never blocks.
#[derive(Debug, Default)]
pub struct TapCounters {
    pub(crate) callbacks: AtomicU64,
    pub(crate) frames: AtomicU64,
    pub(crate) overrun_slots: AtomicU64,
    pub(crate) overrun_frames: AtomicU64,
    pub(crate) bad_buffers: AtomicU64,
    pub(crate) nonzero_callbacks: AtomicU64,
}

impl TapCounters {
    /// A consistent-enough copy of the counters.
    pub fn snapshot(&self) -> TapStats {
        TapStats {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            frames: self.frames.load(Ordering::Relaxed),
            overrun_slots: self.overrun_slots.load(Ordering::Relaxed),
            overrun_frames: self.overrun_frames.load(Ordering::Relaxed),
            bad_buffers: self.bad_buffers.load(Ordering::Relaxed),
            nonzero_callbacks: self.nonzero_callbacks.load(Ordering::Relaxed),
        }
    }
}

/// Counters of one tap since it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapStats {
    /// IO callbacks received.
    pub callbacks: u64,
    /// Audio frames received (per channel).
    pub frames: u64,
    /// Ring slots dropped because the consumer fell behind (overruns).
    pub overrun_slots: u64,
    /// Audio frames in those dropped slots.
    pub overrun_frames: u64,
    /// Buffers dropped because their layout did not match the validated format.
    pub bad_buffers: u64,
    /// Callbacks with at least one sample that is not exactly zero.
    pub nonzero_callbacks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_mode_maps_to_core_audio_values() {
        assert_eq!(TapMuteMode::Unmuted.to_native(), 0);
        assert_eq!(TapMuteMode::Muted.to_native(), 1);
        assert_eq!(TapMuteMode::MutedWhenTapped.to_native(), 2);
    }

    #[test]
    fn mute_mode_defaults_to_unmuted_everywhere() {
        assert_eq!(TapMuteMode::default(), TapMuteMode::Unmuted);
        assert_eq!(TapConfig::for_pid(1).mute, TapMuteMode::Unmuted);
        assert_eq!(TapConfig::for_bundle("a.b").mute, TapMuteMode::Unmuted);
    }

    #[test]
    fn mute_mode_parses_cli_spellings() {
        assert_eq!(TapMuteMode::parse("unmuted"), Some(TapMuteMode::Unmuted));
        assert_eq!(TapMuteMode::parse("muted"), Some(TapMuteMode::Muted));
        assert_eq!(TapMuteMode::parse("muted-when-tapped"), Some(TapMuteMode::MutedWhenTapped));
        assert_eq!(TapMuteMode::parse("mute"), None);
    }

    #[test]
    fn consent_errors_tell_the_user_what_to_do() {
        let bundle = TapError::NotInAppBundle.to_string();
        assert!(bundle.contains("application bundle") && bundle.contains("NSAudioCaptureUsageDescription"));
        let silent = TapError::NoSignal { target: "net.whatsapp.WhatsApp".into() }.to_string();
        assert!(silent.contains("net.whatsapp.WhatsApp"));
        assert!(silent.contains("System Settings") && silent.contains("System Audio Recording"));
    }

    #[test]
    fn permission_error_tells_the_user_what_to_do() {
        let text = TapError::PermissionDenied.to_string();
        assert!(text.contains("System Settings"));
        assert!(text.contains("System Audio Recording"));
    }
}
