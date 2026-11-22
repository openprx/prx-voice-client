//! Fake-routing tests of the controller (pipeline-design 9.1): enter and leave, preconditions, journal order, crash recovery, mid-call changes, sleep and wake.

use prxvc_ports::routing::{RoutingChange, RoutingError};

use super::*;
use crate::driver_status::{BundleState, DriverIssue, DriverVariant};
use crate::input_policy::{InputPolicyError, OWN_MIC_NAME, OWN_MIC_SINK_UID};
use crate::routing::journal::{JournalRead, SCHEMA_VERSION};
use crate::routing::restore::RestoreReason;
use crate::routing::testkit::{
    OWN_PID, Rig, TestController, controller, controller_with_default, journal, uid,
};

fn enter(c: &mut TestController) -> Result<EnterOutcome, CallModeError> {
    c.enter_call_mode(&EnterRequest::default())
}

fn assert_untouched(c: &TestController, rig: &Rig, default: &str) {
    assert_eq!(*c.phase(), Phase::Idle);
    assert_eq!(
        rig.default_input(),
        Some(uid(default)),
        "the default input is as it was"
    );
    assert!(
        rig.switches().is_empty(),
        "no switch was attempted: {:?}",
        rig.switches()
    );
    assert_eq!(rig.journal.content(), JournalRead::Absent);
}

// ---- enter and leave -------------------------------------------------------------------------

#[test]
fn enter_then_leave_restores_the_default_input_exactly() {
    let (mut c, rig) = controller_with_default("usb");
    let entered = enter(&mut c).unwrap();
    assert_eq!(entered.saved.as_ref().map(|d| d.uid.as_str()), Some("usb"));
    assert_eq!(
        entered.saved.as_ref().map(|d| d.name.as_str()),
        Some("USB Mic")
    );
    assert_eq!(
        entered.physical_input.uid.as_str(),
        "usb",
        "the policy takes the physical default"
    );
    assert_eq!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    assert_eq!(*c.phase(), Phase::Active);

    let left = c.leave_call_mode().unwrap();
    assert!(matches!(
        left,
        LeaveOutcome::Restored { restored: Restored { reason: RestoreReason::Saved, ref to }, journal_cleared: true }
            if to.uid.as_str() == "usb"
    ));
    assert_eq!(rig.default_input(), Some(uid("usb")));
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID), uid("usb")]);
    assert_eq!(rig.journal.content(), JournalRead::Absent);
    assert_eq!(*c.phase(), Phase::Idle);
}

#[test]
fn the_journal_is_written_before_the_switch_and_removed_after_the_restore() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    let JournalRead::Valid(j) = rig.journal.content() else {
        panic!("journal expected during call mode")
    };
    assert_eq!(j.saved_default_input_uid.as_deref(), Some("builtin"));
    assert_eq!(j.saved_default_input_name.as_deref(), Some("Built-in Mic"));
    assert_eq!(j.our_mic_uid, OWN_MIC_UID);
    assert_eq!(
        (j.pid, j.started_at, j.schema_version),
        (OWN_PID, 1_790_000_000, SCHEMA_VERSION)
    );
    c.leave_call_mode().unwrap();

    let log = rig.journal.log_entries();
    assert_eq!(log.len(), 2);
    assert_eq!(
        (
            log[0].op,
            log[0].switches_so_far,
            log[0].default_input.clone()
        ),
        ("write", 0, Some(uid("builtin")))
    );
    assert_eq!(
        (
            log[1].op,
            log[1].switches_so_far,
            log[1].default_input.clone()
        ),
        ("remove", 2, Some(uid("builtin"))),
        "removed only after both switches, with the old default back"
    );
}

#[test]
fn leave_is_idempotent() {
    let (mut c, rig) = controller();
    assert_eq!(c.leave_call_mode().unwrap(), LeaveOutcome::NotInCallMode);
    enter(&mut c).unwrap();
    c.leave_call_mode().unwrap();
    let switches = rig.switches();
    assert_eq!(c.leave_call_mode().unwrap(), LeaveOutcome::NotInCallMode);
    assert_eq!(rig.switches(), switches);
    assert_eq!(rig.default_input(), Some(uid("builtin")));
}

#[test]
fn entering_twice_is_refused_without_touching_anything() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    assert_eq!(enter(&mut c), Err(CallModeError::AlreadyInCallMode));
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID)]);
}

#[test]
fn our_microphone_already_the_default_is_not_remembered_and_leave_uses_the_policy() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    let entered = enter(&mut c).unwrap();
    assert_eq!(entered.saved, None);
    let JournalRead::Valid(j) = rig.journal.content() else {
        panic!("journal expected")
    };
    assert_eq!(
        (j.saved_default_input_uid, j.saved_default_input_name),
        (None, None)
    );
    let LeaveOutcome::Restored { restored, .. } = c.leave_call_mode().unwrap() else {
        panic!("restore expected")
    };
    assert_eq!(
        (restored.reason, restored.to.uid.as_str()),
        (RestoreReason::Policy, "builtin")
    );
    assert_ne!(rig.default_input(), Some(uid(OWN_MIC_UID)));
}

// ---- preconditions ---------------------------------------------------------------------------

#[test]
fn each_failed_precondition_leaves_everything_as_it_was() {
    // Driver not installed.
    let (mut c, rig) = controller();
    rig.system.set_bundle(BundleState::Missing);
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::DriverNotReady(DriverIssue::DriverMissing))
    );
    assert_untouched(&c, &rig, "builtin");

    // Installed but the audio daemon has not loaded the microphone.
    let (mut c, rig) = controller();
    rig.system.remove_device(OWN_MIC_UID);
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::DriverNotReady(DriverIssue::NotLoaded {
            uid: OWN_MIC_UID.to_owned()
        }))
    );
    assert_untouched(&c, &rig, "builtin");

    // Loaded but not alive (the sink here).
    let (mut c, rig) = controller();
    rig.system.set_dead(OWN_MIC_SINK_UID);
    assert!(matches!(
        enter(&mut c),
        Err(CallModeError::DriverNotReady(DriverIssue::NotAlive { .. }))
    ));
    assert_untouched(&c, &rig, "builtin");

    // Wrong variant.
    let (mut c, rig) = controller();
    let request = EnterRequest {
        required_variant: Some(DriverVariant::Virtual),
        ..EnterRequest::default()
    };
    assert!(matches!(
        c.enter_call_mode(&request),
        Err(CallModeError::DriverNotReady(
            DriverIssue::WrongVariant { .. }
        ))
    ));
    assert_untouched(&c, &rig, "builtin");

    // No physical microphone: only ours is left.
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.system.keep_only(&[OWN_MIC_UID, OWN_MIC_SINK_UID]);
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::NoPhysicalMicrophone(
            InputPolicyError::NoUsableDevice
        ))
    );
    assert_eq!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    assert!(rig.switches().is_empty());
    assert_eq!(rig.journal.content(), JournalRead::Absent);

    // The configured microphone is ours.
    let (mut c, rig) = controller();
    let request = EnterRequest {
        configured_input: OWN_MIC_NAME.to_owned(),
        ..EnterRequest::default()
    };
    assert!(matches!(
        c.enter_call_mode(&request),
        Err(CallModeError::NoPhysicalMicrophone(
            InputPolicyError::OwnVirtualDevice { .. }
        ))
    ));
    assert_untouched(&c, &rig, "builtin");

    // The default input cannot be read.
    let (mut c, rig) = controller();
    rig.probe
        .fail_queries(Some(RoutingError::Internal("hal".to_owned())));
    assert!(matches!(enter(&mut c), Err(CallModeError::Routing(_))));
    rig.probe.fail_queries(None);
    assert_untouched(&c, &rig, "builtin");
}

#[test]
fn a_journal_that_cannot_be_written_stops_before_any_switch() {
    let (mut c, rig) = controller();
    rig.journal.fail_writes(true);
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::JournalWrite("disk full".to_owned()))
    );
    assert_eq!(*c.phase(), Phase::Idle);
    assert!(rig.switches().is_empty());
    assert_eq!(rig.default_input(), Some(uid("builtin")));
}

#[test]
fn an_existing_journal_blocks_entry_until_recovery_has_dealt_with_it() {
    let (mut c, rig) = controller();
    rig.journal
        .set_content(JournalRead::Valid(journal(999, Some("usb"))));
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::JournalPresent {
            owner_pid: 999,
            owner_alive: false
        })
    );
    rig.system.set_alive(999, true);
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::JournalPresent {
            owner_pid: 999,
            owner_alive: true
        })
    );
    rig.journal.set_content(JournalRead::Corrupt {
        reason: "x".to_owned(),
    });
    assert_eq!(enter(&mut c), Err(CallModeError::JournalCorrupt));
    assert!(rig.switches().is_empty());
}

// ---- the switch fails or does not take effect --------------------------------------------------

#[test]
fn a_switch_that_does_not_take_effect_is_detected_and_undone_cleanly() {
    let (mut c, rig) = controller();
    rig.ignore_switches(true);
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::SwitchNotApplied {
            expected: uid(OWN_MIC_UID),
            actual: Some(uid("builtin"))
        })
    );
    assert_eq!(*c.phase(), Phase::Idle);
    assert_eq!(rig.default_input(), Some(uid("builtin")));
    assert_eq!(
        rig.journal.content(),
        JournalRead::Absent,
        "the journal is removed again"
    );
    let ops: Vec<_> = rig.journal.log_entries().iter().map(|e| e.op).collect();
    assert_eq!(ops, vec!["write", "remove"]);
}

#[test]
fn a_refused_switch_is_a_typed_error_and_leaves_nothing_behind() {
    let (mut c, rig) = controller();
    rig.probe.fail_next_set(RoutingError::Unavailable {
        reason: "denied".to_owned(),
    });
    assert_eq!(
        enter(&mut c),
        Err(CallModeError::SwitchFailed(RoutingError::Unavailable {
            reason: "denied".to_owned()
        }))
    );
    assert_untouched(&c, &rig, "builtin");
    // A retry works.
    enter(&mut c).unwrap();
}

// ---- crash recovery --------------------------------------------------------------------------

#[test]
fn a_stale_journal_with_our_microphone_as_default_restores_the_saved_input() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.journal
        .set_content(JournalRead::Valid(journal(4242, Some("usb"))));
    let report = c.recover_if_needed();
    let RecoveryReport::Restored {
        restored,
        crashed_pid,
    } = report
    else {
        panic!("restore expected: {report:?}")
    };
    assert_eq!(crashed_pid, 4242);
    assert_eq!(
        (restored.reason, restored.to.uid.as_str()),
        (RestoreReason::Saved, "usb")
    );
    assert_eq!(rig.default_input(), Some(uid("usb")));
    assert_eq!(rig.journal.content(), JournalRead::Absent);
}

#[test]
fn a_journal_with_this_process_id_after_a_restart_is_stale() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.journal
        .set_content(JournalRead::Valid(journal(OWN_PID, Some("usb"))));
    assert!(matches!(
        c.recover_if_needed(),
        RecoveryReport::Restored { .. }
    ));
    assert_eq!(rig.default_input(), Some(uid("usb")));
}

#[test]
fn a_stale_journal_whose_saved_device_is_gone_restores_by_policy_and_never_to_ours() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.journal
        .set_content(JournalRead::Valid(journal(4242, Some("gone"))));
    let RecoveryReport::Restored { restored, .. } = c.recover_if_needed() else {
        panic!("restore expected")
    };
    assert_eq!(
        (restored.reason, restored.to.uid.as_str()),
        (RestoreReason::Policy, "builtin")
    );

    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.journal
        .set_content(JournalRead::Valid(journal(4242, Some(OWN_MIC_UID))));
    let RecoveryReport::Restored { restored, .. } = c.recover_if_needed() else {
        panic!("restore expected")
    };
    assert_ne!(restored.to.uid.as_str(), OWN_MIC_UID);
    assert_ne!(rig.default_input(), Some(uid(OWN_MIC_UID)));
}

#[test]
fn a_stale_journal_with_another_default_is_only_deleted() {
    let (mut c, rig) = controller_with_default("usb");
    rig.journal
        .set_content(JournalRead::Valid(journal(4242, Some("builtin"))));
    assert_eq!(
        c.recover_if_needed(),
        RecoveryReport::StaleJournalCleared { crashed_pid: 4242 }
    );
    assert_eq!(
        rig.default_input(),
        Some(uid("usb")),
        "the user's choice is not touched"
    );
    assert!(rig.switches().is_empty());
    assert_eq!(rig.journal.content(), JournalRead::Absent);
}

#[test]
fn the_journal_of_a_live_other_process_is_left_alone() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.system.set_alive(4242, true);
    let saved = JournalRead::Valid(journal(4242, Some("usb")));
    rig.journal.set_content(saved.clone());
    assert_eq!(
        c.recover_if_needed(),
        RecoveryReport::OwnerAlive { pid: 4242 }
    );
    assert!(rig.switches().is_empty());
    assert_eq!(rig.journal.content(), saved);
}

#[test]
fn no_journal_and_our_microphone_as_default_is_put_back_by_policy_at_start() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    let report = c.recover_if_needed();
    assert!(
        matches!(report, RecoveryReport::OwnMicrophoneRestoredWithoutJournal { ref restored }
            if restored.reason == RestoreReason::Policy && restored.to.uid.as_str() != OWN_MIC_UID),
        "{report:?}"
    );
    assert_ne!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    assert_eq!(rig.switches().len(), 1);
    assert!(
        rig.journal.log_entries().is_empty(),
        "no journal was involved"
    );
}

#[test]
fn outside_call_mode_a_default_that_becomes_ours_is_corrected_and_never_in_a_loop() {
    let (mut c, rig) = controller();
    assert!(
        c.correct_idle_default().is_none(),
        "a physical default is left alone"
    );
    assert!(rig.switches().is_empty());
    // macOS picks our microphone when the device that was the default goes away: corrected.
    for round in 0..IDLE_FIXES_PER_WINDOW {
        rig.probe.user_selects_input(OWN_MIC_UID);
        let fixed = c.correct_idle_default();
        assert!(matches!(fixed, Some(Ok(_))), "round {round}: {fixed:?}");
        assert_ne!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    }
    // It keeps coming back: the correction stops for the window and says so once.
    rig.probe.user_selects_input(OWN_MIC_UID);
    assert!(c.correct_idle_default().is_none());
    assert_eq!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    assert_eq!(rig.switches().len(), IDLE_FIXES_PER_WINDOW as usize);
}

#[test]
fn in_call_mode_the_idle_correction_does_nothing() {
    let (mut c, rig) = controller_with_default("usb");
    enter(&mut c).unwrap();
    let switches = rig.switches().len();
    assert!(c.correct_idle_default().is_none());
    assert_eq!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    assert_eq!(rig.switches().len(), switches);
}

#[test]
fn no_journal_and_a_physical_default_is_nothing_to_do() {
    let (mut c, rig) = controller();
    assert_eq!(c.recover_if_needed(), RecoveryReport::Nothing);
    assert!(rig.switches().is_empty());
}

#[test]
fn a_corrupt_journal_is_preserved_aside_and_recovery_follows_the_policy() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.journal.set_content(JournalRead::Corrupt {
        reason: "not a journal".to_owned(),
    });
    let RecoveryReport::CorruptJournal {
        reason,
        preserved_at,
        restored,
    } = c.recover_if_needed()
    else {
        panic!("corrupt-journal report expected")
    };
    assert_eq!(reason, "not a journal");
    assert!(
        preserved_at
            .unwrap()
            .to_string_lossy()
            .contains("corrupt-1790000000")
    );
    let restored = restored.expect("our microphone was the default, so it was restored");
    assert_eq!(
        (restored.reason, restored.to.uid.as_str()),
        (RestoreReason::Policy, "builtin")
    );
    assert_eq!(rig.journal.content(), JournalRead::Absent);

    // A corrupt journal with a physical default: moved aside, nothing restored.
    let (mut c, rig) = controller_with_default("usb");
    rig.journal.set_content(JournalRead::Corrupt {
        reason: "x".to_owned(),
    });
    let RecoveryReport::CorruptJournal { restored, .. } = c.recover_if_needed() else {
        panic!("report expected")
    };
    assert_eq!(restored, None);
    assert!(rig.switches().is_empty());
}

#[test]
fn a_failed_recovery_keeps_the_journal() {
    let (mut c, rig) = controller_with_default(OWN_MIC_UID);
    rig.journal
        .set_content(JournalRead::Valid(journal(4242, Some("usb"))));
    rig.probe.fail_next_set(RoutingError::Unavailable {
        reason: "denied".to_owned(),
    });
    let RecoveryReport::Failed { error } = c.recover_if_needed() else {
        panic!("failure expected")
    };
    assert!(matches!(error, CallModeError::RestoreFailed(_)));
    assert!(matches!(rig.journal.content(), JournalRead::Valid(_)));
    // The next launch succeeds.
    assert!(matches!(
        c.recover_if_needed(),
        RecoveryReport::Restored { .. }
    ));
}

#[test]
fn recovery_in_call_mode_is_not_attempted() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    assert_eq!(c.recover_if_needed(), RecoveryReport::InCallMode);
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID)]);
}

// ---- leave --------------------------------------------------------------------------------------

#[test]
fn a_remembered_device_that_is_gone_falls_back_by_policy_and_the_change_is_reported() {
    let (mut c, rig) = controller_with_default("usb");
    enter(&mut c).unwrap();
    rig.unplug("usb");
    let events = c.poll();
    assert_eq!(events.len(), 1);
    let RoutingEvent::RestoreTargetChanged { target } = &events[0] else {
        panic!("target change expected")
    };
    assert_eq!(target.as_ref().map(|d| d.uid.as_str()), Some("builtin"));

    let LeaveOutcome::Restored { restored, .. } = c.leave_call_mode().unwrap() else {
        panic!("restore expected")
    };
    assert_eq!(
        (restored.reason, restored.to.uid.as_str()),
        (RestoreReason::Policy, "builtin")
    );
    assert_eq!(rig.default_input(), Some(uid("builtin")));
}

#[test]
fn a_remembered_device_that_comes_back_is_restored_and_reported() {
    let (mut c, rig) = controller_with_default("usb");
    enter(&mut c).unwrap();
    rig.unplug("usb");
    c.poll();
    rig.plug("usb");
    let events = c.poll();
    let [RoutingEvent::RestoreTargetChanged { target }] = events.as_slice() else {
        panic!("one event expected: {events:?}")
    };
    assert_eq!(target.as_ref().map(|d| d.uid.as_str()), Some("usb"));
    let LeaveOutcome::Restored { restored, .. } = c.leave_call_mode().unwrap() else {
        panic!("restore expected")
    };
    assert_eq!(
        (restored.reason, restored.to.uid.as_str()),
        (RestoreReason::Saved, "usb")
    );
}

#[test]
fn leave_never_restores_to_our_devices_even_when_only_they_remain() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.unplug("builtin");
    rig.unplug("usb");
    assert_eq!(c.leave_call_mode(), Err(CallModeError::NoRestoreTarget));
    assert_eq!(
        *c.phase(),
        Phase::Active,
        "call mode and journal stay so that leave can be retried"
    );
    assert!(matches!(rig.journal.content(), JournalRead::Valid(_)));
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID)]);
}

#[test]
fn a_failed_restore_keeps_the_journal_and_can_be_retried() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe.fail_next_set(RoutingError::Unavailable {
        reason: "denied".to_owned(),
    });
    assert!(matches!(
        c.leave_call_mode(),
        Err(CallModeError::RestoreFailed(_))
    ));
    assert!(matches!(rig.journal.content(), JournalRead::Valid(_)));
    assert_eq!(*c.phase(), Phase::Active);
    assert!(matches!(
        c.leave_call_mode(),
        Ok(LeaveOutcome::Restored { .. })
    ));
    assert_eq!(rig.default_input(), Some(uid("builtin")));
}

#[test]
fn a_restore_that_does_not_take_effect_is_detected() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.ignore_switches(true);
    assert_eq!(
        c.leave_call_mode(),
        Err(CallModeError::RestoreNotApplied {
            expected: uid("builtin"),
            actual: Some(uid(OWN_MIC_UID))
        })
    );
    assert!(matches!(rig.journal.content(), JournalRead::Valid(_)));
}

#[test]
fn a_journal_that_cannot_be_deleted_after_the_restore_is_reported_not_fatal() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.journal.fail_removes(true);
    assert!(matches!(
        c.leave_call_mode().unwrap(),
        LeaveOutcome::Restored {
            journal_cleared: false,
            ..
        }
    ));
    assert_eq!(*c.phase(), Phase::Idle);
    assert_eq!(rig.default_input(), Some(uid("builtin")));
}

// ---- changes while call mode is on --------------------------------------------------------------

#[test]
fn a_default_change_by_the_user_is_reported_and_never_fought() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    let _ = c.poll();
    rig.probe.user_selects_input("usb");
    assert_eq!(
        c.poll(),
        vec![RoutingEvent::DefaultInputDisplaced { to: uid("usb") }]
    );
    assert_eq!(*c.phase(), Phase::Displaced { to: uid("usb") });
    // More notices, even the same device again: no event and no switch of ours.
    rig.probe.user_selects_input("usb");
    rig.probe.push_change(RoutingChange::DeviceListChanged);
    assert!(c.poll().is_empty());
    assert_eq!(
        rig.switches(),
        vec![uid(OWN_MIC_UID)],
        "no re-assertion, so no loop"
    );
    assert_eq!(rig.default_input(), Some(uid("usb")));
}

#[test]
fn a_headset_taking_over_then_the_user_choosing_ours_again_is_followed() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe.user_selects_input("usb");
    c.poll();
    rig.probe.user_selects_input("builtin");
    assert_eq!(
        c.poll(),
        vec![RoutingEvent::DefaultInputDisplaced { to: uid("builtin") }]
    );
    rig.probe.user_selects_input(OWN_MIC_UID);
    assert_eq!(c.poll(), vec![RoutingEvent::OwnMicrophoneReselected]);
    assert_eq!(*c.phase(), Phase::Active);
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID)]);
}

#[test]
fn leaving_after_a_user_change_leaves_the_users_choice_and_clears_the_journal() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe.user_selects_input("usb");
    c.poll();
    let left = c.leave_call_mode().unwrap();
    assert_eq!(
        left,
        LeaveOutcome::LeftAlone {
            default: Some(uid("usb")),
            journal_cleared: true
        }
    );
    assert_eq!(rig.default_input(), Some(uid("usb")));
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID)]);
    assert_eq!(rig.journal.content(), JournalRead::Absent);
}

#[test]
fn leaving_before_the_change_was_polled_also_respects_the_users_choice() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe.user_selects_input("usb");
    assert!(matches!(
        c.leave_call_mode().unwrap(),
        LeaveOutcome::LeftAlone { .. }
    ));
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID)]);
}

#[test]
fn reassert_is_an_explicit_action_that_makes_our_microphone_the_default_again() {
    let (mut c, rig) = controller();
    assert_eq!(c.reassert(), Err(CallModeError::NotInCallMode));
    enter(&mut c).unwrap();
    rig.probe.user_selects_input("usb");
    c.poll();
    c.reassert().unwrap();
    assert_eq!(rig.default_input(), Some(uid(OWN_MIC_UID)));
    assert_eq!(*c.phase(), Phase::Active);
    assert_eq!(rig.switches(), vec![uid(OWN_MIC_UID), uid(OWN_MIC_UID)]);
    // The notice of our own switch is not a displacement.
    assert!(c.poll().is_empty());
}

#[test]
fn reassert_fails_typed_when_the_switch_does_not_stick() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe.user_selects_input("usb");
    c.poll();
    rig.ignore_switches(true);
    assert_eq!(
        c.reassert(),
        Err(CallModeError::SwitchNotApplied {
            expected: uid(OWN_MIC_UID),
            actual: Some(uid("usb"))
        })
    );
    assert_eq!(*c.phase(), Phase::Displaced { to: uid("usb") });
}

#[test]
fn our_microphone_disappearing_reports_the_failure_once_and_leave_still_restores() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.unplug(OWN_MIC_UID);
    assert_eq!(c.poll(), vec![RoutingEvent::VirtualMicrophoneLost]);
    assert_eq!(*c.phase(), Phase::MicLost);
    rig.probe.push_change(RoutingChange::DeviceListChanged);
    assert!(c.poll().is_empty(), "reported once");
    assert_eq!(c.reassert(), Err(CallModeError::VirtualMicrophoneLost));

    // The audio daemon moved the default to the built-in microphone by itself.
    rig.probe.user_selects_input("builtin");
    assert!(c.poll().is_empty());
    assert!(matches!(
        c.leave_call_mode().unwrap(),
        LeaveOutcome::LeftAlone { .. }
    ));
    assert_eq!(rig.default_input(), Some(uid("builtin")));
    assert_eq!(rig.journal.content(), JournalRead::Absent);
}

#[test]
fn our_microphone_losing_its_life_counts_as_gone() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.system.set_dead(OWN_MIC_UID);
    rig.probe.push_change(RoutingChange::DeviceListChanged);
    assert_eq!(c.poll(), vec![RoutingEvent::VirtualMicrophoneLost]);
}

#[test]
fn changes_outside_call_mode_are_ignored() {
    let (mut c, rig) = controller();
    rig.probe.user_selects_input("usb");
    rig.probe.push_change(RoutingChange::Sleep);
    assert!(c.poll().is_empty());
}

#[test]
fn sleep_keeps_the_journal_and_wake_reports_whether_the_microphone_survived() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe.push_change(RoutingChange::Sleep);
    assert_eq!(c.poll(), vec![RoutingEvent::SystemSleeping]);
    assert!(matches!(rig.journal.content(), JournalRead::Valid(_)));
    rig.probe.push_change(RoutingChange::Wake);
    assert_eq!(
        c.poll(),
        vec![RoutingEvent::SystemWoke {
            default_input_intact: true
        }]
    );

    // The default changed while the machine slept.
    rig.probe.push_change(RoutingChange::Sleep);
    rig.probe.user_selects_input("usb");
    rig.probe.push_change(RoutingChange::Wake);
    assert_eq!(
        c.poll(),
        vec![
            RoutingEvent::SystemSleeping,
            RoutingEvent::DefaultInputDisplaced { to: uid("usb") },
            RoutingEvent::SystemWoke {
                default_input_intact: false
            },
        ]
    );
    assert_eq!(
        rig.switches(),
        vec![uid(OWN_MIC_UID)],
        "wake never switches by itself"
    );
}

#[test]
fn the_default_output_change_is_passed_on() {
    let (mut c, rig) = controller();
    enter(&mut c).unwrap();
    rig.probe
        .push_change(RoutingChange::DefaultOutputChanged(uid("speakers")));
    assert_eq!(
        c.poll(),
        vec![RoutingEvent::DefaultOutputChanged {
            to: uid("speakers")
        }]
    );
}
