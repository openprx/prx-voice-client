//! Start-up routing recovery: puts the system default input back after a crash in call mode and publishes what happened as an event in the shared state.

use prxvc_audio::routing::RecoveryReport;
use prxvc_desktop_ui::{EventSnapshot, SharedAppState};

/// The event the UI can show for a recovery result, or `None` when there is nothing to tell.
///
/// The event types are stable names; the detail is plain English for the debug
/// view. The user-facing texts (a tip such as "PRX Voice restored your
/// microphone after an unexpected exit") are produced from the event type by
/// the UI package, through i18n.
pub(crate) fn event_for(report: &RecoveryReport) -> Option<(&'static str, String)> {
    match report {
        RecoveryReport::Nothing | RecoveryReport::InCallMode => None,
        RecoveryReport::Restored {
            restored,
            crashed_pid,
        } => Some((
            "RoutingRestored",
            format!(
                "restored the default input to {} [{}] after process {crashed_pid} ended in call mode",
                restored.to.name, restored.to.uid
            ),
        )),
        RecoveryReport::StaleJournalCleared { crashed_pid } => Some((
            "RoutingJournalCleared",
            format!(
                "removed the routing journal of process {crashed_pid}; the default input was not ours"
            ),
        )),
        RecoveryReport::OwnerAlive { pid } => Some((
            "RoutingOwnerAlive",
            format!(
                "another PRX Voice process ({pid}) is in call mode; the default input was left alone"
            ),
        )),
        RecoveryReport::OwnMicrophoneIsDefaultWithoutJournal => Some((
            "RoutingOwnMicrophoneDefault",
            "the PRX Voice Microphone is the system default input and no call mode is recorded; \
             it was left unchanged"
                .to_owned(),
        )),
        RecoveryReport::CorruptJournal {
            reason,
            preserved_at,
            restored,
        } => Some((
            "RoutingJournalCorrupt",
            format!(
                "the routing journal was unusable ({reason}), kept at {}; default input {}",
                preserved_at
                    .as_ref()
                    .map_or("(could not be moved)".to_owned(), |p| p
                        .display()
                        .to_string()),
                restored
                    .as_ref()
                    .map_or("left unchanged".to_owned(), |r| format!(
                        "restored to {}",
                        r.to.name
                    )),
            ),
        )),
        RecoveryReport::Failed { error } => Some((
            "RoutingRecoveryFailed",
            format!("could not restore the default input: {error}"),
        )),
    }
}

/// Run the recovery once, before anything else uses the microphone, and publish the result.
#[cfg(target_os = "macos")]
pub(crate) fn recover_at_start(shared: &SharedAppState) {
    use prxvc_audio::routing::{CoreAudioRouting, FileJournalStore, OsSystem, RoutingController};

    let Some(paths) = crate::config_load::app_paths() else {
        tracing::warn!("routing recovery skipped: the data directory is unknown");
        return;
    };
    let routing = match CoreAudioRouting::new() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("routing recovery skipped: {e}");
            return;
        }
    };
    let mut controller =
        RoutingController::new(routing, OsSystem, FileJournalStore::in_dir(&paths.data_dir));
    publish(shared, &controller.recover_if_needed());
}

/// Routing recovery exists on macOS only.
#[cfg(not(target_os = "macos"))]
pub(crate) fn recover_at_start(_shared: &SharedAppState) {}

#[cfg(target_os = "macos")]
fn publish(shared: &SharedAppState, report: &RecoveryReport) {
    if let Some((event_type, detail)) = event_for(report) {
        tracing::info!(event_type, %detail, "routing recovery");
        shared.push_event(EventSnapshot {
            event_type: event_type.into(),
            detail,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }
}

#[cfg(test)]
mod tests {
    use prxvc_audio::routing::{CallModeError, DeviceRef, RestoreReason, Restored};
    use prxvc_ports::routing::DeviceUid;

    use super::*;

    fn restored() -> Restored {
        Restored {
            to: DeviceRef {
                uid: DeviceUid::new("builtin"),
                name: "Built-in Mic".to_owned(),
            },
            reason: RestoreReason::Saved,
        }
    }

    #[test]
    fn nothing_to_tell_for_a_quiet_start() {
        assert_eq!(event_for(&RecoveryReport::Nothing), None);
        assert_eq!(event_for(&RecoveryReport::InCallMode), None);
    }

    #[test]
    fn every_other_result_is_an_event_with_a_stable_type() {
        let reports = [
            (
                RecoveryReport::Restored {
                    restored: restored(),
                    crashed_pid: 7,
                },
                "RoutingRestored",
            ),
            (
                RecoveryReport::StaleJournalCleared { crashed_pid: 7 },
                "RoutingJournalCleared",
            ),
            (RecoveryReport::OwnerAlive { pid: 7 }, "RoutingOwnerAlive"),
            (
                RecoveryReport::OwnMicrophoneIsDefaultWithoutJournal,
                "RoutingOwnMicrophoneDefault",
            ),
            (
                RecoveryReport::CorruptJournal {
                    reason: "bad".to_owned(),
                    preserved_at: None,
                    restored: Some(restored()),
                },
                "RoutingJournalCorrupt",
            ),
            (
                RecoveryReport::Failed {
                    error: CallModeError::NoRestoreTarget,
                },
                "RoutingRecoveryFailed",
            ),
        ];
        for (report, expected) in reports {
            let (event_type, detail) = event_for(&report).expect("an event");
            assert_eq!(event_type, expected);
            assert!(!detail.is_empty());
        }
    }

    #[test]
    fn the_restore_event_names_the_device_and_the_crashed_process() {
        let (_, detail) = event_for(&RecoveryReport::Restored {
            restored: restored(),
            crashed_pid: 4242,
        })
        .unwrap();
        assert!(
            detail.contains("Built-in Mic") && detail.contains("4242"),
            "{detail}"
        );
    }
}
