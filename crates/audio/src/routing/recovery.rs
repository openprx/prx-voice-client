//! What recovery at application start should do, decided from the journal, the default input and whether the journal's process is running.
//!
//! Pure: the controller executes the plan, and the `routing` example prints it
//! without executing it.

use prxvc_ports::routing::{DeviceUid, RoutingError};

use super::events::DeviceRef;
use super::journal::{Journal, JournalRead};
use crate::input_policy::OWN_MIC_UID;

/// The decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryPlan {
    /// No journal and our microphone is not the default: nothing to do.
    Nothing,
    /// No journal, yet our microphone is the default: change nothing, report it.
    OwnMicrophoneWithoutJournal,
    /// The journal belongs to another running process: leave everything alone.
    OwnerAlive {
        /// The running owner.
        pid: u32,
    },
    /// A stale journal and the default input is already another device: delete the journal only.
    ClearStale {
        /// The process that left it.
        crashed_pid: u32,
    },
    /// A stale journal and our microphone is the default: restore the default input.
    Restore {
        /// The remembered default input (`None` when the journal has none).
        saved: Option<DeviceRef>,
        /// The process that left the journal.
        crashed_pid: u32,
    },
    /// An unusable journal: move it aside; restore by policy when our microphone is the default.
    Corrupt {
        /// Why the file was unusable.
        reason: String,
        /// Our microphone is the default input, so a restore follows.
        restore: bool,
    },
    /// The default input could not be read, so nothing can be decided.
    Unreadable(RoutingError),
}

/// Decide what recovery does. `owner_alive` answers whether a journal's process
/// is a running process other than this one.
pub fn decide(
    journal: &JournalRead,
    default_input: &Result<DeviceUid, RoutingError>,
    owner_alive: impl Fn(u32) -> bool,
) -> RecoveryPlan {
    if let JournalRead::Valid(j) = journal
        && owner_alive(j.pid)
    {
        return RecoveryPlan::OwnerAlive { pid: j.pid };
    }
    let current = match default_input {
        Ok(c) => c,
        Err(e) => return RecoveryPlan::Unreadable(e.clone()),
    };
    let ours = current.as_str() == OWN_MIC_UID;
    match journal {
        JournalRead::Absent if ours => RecoveryPlan::OwnMicrophoneWithoutJournal,
        JournalRead::Absent => RecoveryPlan::Nothing,
        JournalRead::Valid(j) if ours => RecoveryPlan::Restore {
            saved: saved_of(j),
            crashed_pid: j.pid,
        },
        JournalRead::Valid(j) => RecoveryPlan::ClearStale { crashed_pid: j.pid },
        JournalRead::Corrupt { reason } => RecoveryPlan::Corrupt {
            reason: reason.clone(),
            restore: ours,
        },
    }
}

fn saved_of(journal: &Journal) -> Option<DeviceRef> {
    journal
        .saved_default_input_uid
        .as_ref()
        .map(|uid| DeviceRef {
            uid: DeviceUid::new(uid.clone()),
            name: journal
                .saved_default_input_name
                .clone()
                .unwrap_or_else(|| uid.clone()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::journal::SCHEMA_VERSION;

    fn journal(pid: u32) -> JournalRead {
        JournalRead::Valid(Journal {
            schema_version: SCHEMA_VERSION,
            saved_default_input_uid: Some("usb".to_owned()),
            saved_default_input_name: None,
            our_mic_uid: OWN_MIC_UID.to_owned(),
            pid,
            started_at: 0,
        })
    }

    fn default(uid: &str) -> Result<DeviceUid, RoutingError> {
        Ok(DeviceUid::new(uid))
    }

    #[test]
    fn the_decision_table() {
        let dead = |_| false;
        assert_eq!(
            decide(&JournalRead::Absent, &default("usb"), dead),
            RecoveryPlan::Nothing
        );
        assert_eq!(
            decide(&JournalRead::Absent, &default(OWN_MIC_UID), dead),
            RecoveryPlan::OwnMicrophoneWithoutJournal
        );
        assert_eq!(
            decide(&journal(7), &default("usb"), |_| true),
            RecoveryPlan::OwnerAlive { pid: 7 }
        );
        assert_eq!(
            decide(&journal(7), &default("usb"), dead),
            RecoveryPlan::ClearStale { crashed_pid: 7 }
        );
        let RecoveryPlan::Restore { saved, crashed_pid } =
            decide(&journal(7), &default(OWN_MIC_UID), dead)
        else {
            panic!("restore expected")
        };
        assert_eq!(
            (saved.unwrap().name, crashed_pid),
            ("usb".to_owned(), 7),
            "the uid stands in for a missing name"
        );
        let corrupt = JournalRead::Corrupt {
            reason: "x".to_owned(),
        };
        assert_eq!(
            decide(&corrupt, &default(OWN_MIC_UID), dead),
            RecoveryPlan::Corrupt {
                reason: "x".to_owned(),
                restore: true
            }
        );
        assert_eq!(
            decide(&corrupt, &default("usb"), dead),
            RecoveryPlan::Corrupt {
                reason: "x".to_owned(),
                restore: false
            }
        );
        let err = Err(RoutingError::Internal("hal".to_owned()));
        assert!(matches!(
            decide(&journal(7), &err, dead),
            RecoveryPlan::Unreadable(_)
        ));
        assert_eq!(
            decide(&journal(7), &err, |_| true),
            RecoveryPlan::OwnerAlive { pid: 7 }
        );
    }
}
