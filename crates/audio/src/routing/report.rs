//! What entering, leaving and recovering call mode report: the typed errors and the outcomes.

use std::path::PathBuf;

use prxvc_ports::routing::{DeviceUid, RoutingError};

use super::events::DeviceRef;
use super::restore::RestoreReason;
use crate::driver_status::DriverIssue;
use crate::input_policy::InputPolicyError;

/// Entering, leaving or re-asserting call mode failed. Nothing is left half-done:
/// a failed entry has put the default input back and removed the journal; a
/// failed leave keeps the journal so that the next launch can finish the job.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallModeError {
    /// Call mode is already on.
    #[error("call mode is already active")]
    AlreadyInCallMode,
    /// Call mode is not on.
    #[error("call mode is not active")]
    NotInCallMode,
    /// The virtual microphone driver is not usable (not installed, not loaded, stale, wrong variant).
    #[error("the virtual microphone is not ready: {0}")]
    DriverNotReady(DriverIssue),
    /// No physical microphone can be used as the call's local input.
    #[error("no usable microphone: {0}")]
    NoPhysicalMicrophone(InputPolicyError),
    /// A journal from an earlier run exists; `recover_if_needed` decides what to do with it.
    #[error("a routing journal of process {owner_pid} exists (that process is {})", if *.owner_alive { "running" } else { "gone" })]
    JournalPresent {
        /// The process that wrote it.
        owner_pid: u32,
        /// It is still running.
        owner_alive: bool,
    },
    /// An unusable journal file exists; `recover_if_needed` moves it aside.
    #[error("the routing journal is unreadable")]
    JournalCorrupt,
    /// The journal could not be written, so the switch was not attempted.
    #[error("the routing journal could not be written: {0}")]
    JournalWrite(String),
    /// The operating system could not be asked.
    #[error("{0}")]
    Routing(RoutingError),
    /// The system refused to make our microphone the default input.
    #[error("could not switch the microphone: {0}")]
    SwitchFailed(RoutingError),
    /// The switch was accepted but reading the default input back shows another device.
    #[error(
        "the microphone switch did not take effect (default is {actual:?}, expected {expected})"
    )]
    SwitchNotApplied {
        /// Our microphone.
        expected: DeviceUid,
        /// What the default input is.
        actual: Option<DeviceUid>,
    },
    /// Our virtual microphone is gone.
    #[error("the virtual microphone is missing")]
    VirtualMicrophoneLost,
    /// No physical input exists to restore to; call mode stays on and the journal stays.
    #[error("no microphone to restore the system default to")]
    NoRestoreTarget,
    /// The system refused to restore the default input; the journal stays.
    #[error("could not restore the microphone: {0}")]
    RestoreFailed(RoutingError),
    /// The restore was accepted but reading back shows another device; the journal stays.
    #[error(
        "the microphone restore did not take effect (default is {actual:?}, expected {expected})"
    )]
    RestoreNotApplied {
        /// The device that should be the default.
        expected: DeviceUid,
        /// What the default input is.
        actual: Option<DeviceUid>,
    },
}

/// Call mode is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterOutcome {
    /// The default input before call mode; `None` when it was already our microphone.
    pub saved: Option<DeviceRef>,
    /// The physical microphone the input policy chose for the call.
    pub physical_input: DeviceRef,
}

/// A default input restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// The device now the default input.
    pub to: DeviceRef,
    /// Whether it is the remembered device or the policy's choice (then the UI
    /// says "Your previous microphone is gone; using <name>").
    pub reason: RestoreReason,
}

/// What leaving call mode did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaveOutcome {
    /// Call mode was not on; nothing was touched (leave is idempotent).
    NotInCallMode,
    /// The default input was our microphone and is now `restored.to`.
    Restored {
        /// The restore.
        restored: Restored,
        /// The journal file was deleted (a failure to delete is logged and is not an error).
        journal_cleared: bool,
    },
    /// The default input was already another device (the user's choice), so it was left alone.
    LeftAlone {
        /// The default input.
        default: Option<DeviceUid>,
        /// The journal file was deleted.
        journal_cleared: bool,
    },
}

/// What `recover_if_needed` found and did at application start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryReport {
    /// No journal and our microphone is not the default: nothing to do.
    Nothing,
    /// This controller is in call mode; nothing to recover.
    InCallMode,
    /// A stale journal and our microphone was the default: the default input was restored.
    Restored {
        /// The restore.
        restored: Restored,
        /// Process that left the journal.
        crashed_pid: u32,
    },
    /// A stale journal, but the default input was already another device: the journal was deleted, nothing else changed.
    StaleJournalCleared {
        /// Process that left the journal.
        crashed_pid: u32,
    },
    /// The journal belongs to another running process (a second instance in call mode): left alone.
    OwnerAlive {
        /// The running owner.
        pid: u32,
    },
    /// No journal, yet our microphone was the default input outside call mode (macOS chose it when
    /// the previous default device went away, or the user selected it): it was put back by policy,
    /// because every other application would record silence from it.
    OwnMicrophoneRestoredWithoutJournal {
        /// The restore.
        restored: Restored,
    },
    /// The journal was unusable: moved aside, and if our microphone was the
    /// default input it was restored by policy.
    CorruptJournal {
        /// Why the file was unusable.
        reason: String,
        /// Where the file went; `None` when moving it failed.
        preserved_at: Option<PathBuf>,
        /// The restore, when our microphone was the default input.
        restored: Option<Restored>,
    },
    /// Recovery was needed and failed; the journal stays for the next attempt.
    Failed {
        /// Why.
        error: CallModeError,
    },
}
