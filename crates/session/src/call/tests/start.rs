//! The start-up sequence: order, each preflight failure, each later failure unwinding what was done.

use prxvc_media::Direction;

use super::kit::{Rig, steps};
use crate::call::{CallCommand, CallErrorCode, Severity};
use crate::call_state::CallState;
use crate::fault::{Blocker, Endpoint};
use crate::output::OutputSource;

#[test]
fn the_full_start_sequence_runs_in_the_designed_order() {
    let mut rig = Rig::new();
    rig.start();
    assert_eq!(
        steps(&rig.log()),
        [
            "bundle",
            "mic_consent",
            "capture_consent",
            "driver",
            "mic_check",
            "start_outgoing",
            "enter",
            "start_incoming",
            "probe",
        ]
    );
    let s = rig.last();
    assert_eq!(s.state, CallState::AudioReady { translate: false });
    assert!(s.issues.is_empty(), "{:?}", s.issues);
    assert!(s.routing.in_call_mode && s.routing.default_input_is_ours);
    assert_eq!(s.routing.saved_input.as_deref(), Some("Studio Mic"));
    assert_eq!(s.devices.microphone.as_deref(), Some("Built-in Mic"));
    assert_eq!(s.devices.virtual_sink.as_deref(), Some("virtual-sink"));
    assert_eq!(s.devices.call_app.as_deref(), Some(super::kit::APP));
    // M1: both listeners hear the original, both paths run.
    assert_eq!(
        s.direction(Direction::Outgoing).hears,
        OutputSource::Original
    );
    assert_eq!(
        s.direction(Direction::Incoming).hears,
        OutputSource::Original
    );
    assert!(s.direction(Direction::Outgoing).path.is_some());
    assert!(s.direction(Direction::Incoming).path.is_some());
}

#[test]
fn the_driver_is_checked_for_the_selected_application() {
    let mut rig = Rig::new();
    rig.start();
    assert!(rig.log().contains(&format!("driver:{}", super::kit::APP)));
}

#[test]
fn a_snapshot_is_published_for_every_transition_including_initializing() {
    let mut rig = Rig::new();
    rig.start();
    let states: Vec<CallState> = rig
        .snapshots
        .lock()
        .iter()
        .map(|s| s.state.clone())
        .collect();
    assert_eq!(
        states,
        [
            CallState::Initializing {
                providers: crate::call_state::ProviderProgress::NotNeeded
            },
            CallState::AudioReady { translate: false },
        ]
    );
    let revisions: Vec<u64> = rig.snapshots.lock().iter().map(|s| s.revision).collect();
    assert_eq!(revisions, [1, 2], "revisions count the publications");
    rig.stop();
    let states: Vec<CallState> = rig
        .snapshots
        .lock()
        .iter()
        .map(|s| s.state.clone())
        .collect();
    assert!(
        states.ends_with(&[CallState::Stopping, CallState::Idle]),
        "{states:?}"
    );
}

/// A failure before anything changed: the machine goes straight to Idle, nothing is started.
fn preflight_failure(
    setup: impl FnOnce(&mut Rig),
    code: CallErrorCode,
    blocker: Blocker,
    last_step: &str,
) {
    let mut rig = Rig::new();
    setup(&mut rig);
    rig.start();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle, "{code:?}");
    let issue = s.last_error.as_ref().expect("a typed error");
    assert_eq!(issue.code, code);
    assert_eq!(issue.severity, Severity::Blocking);
    assert_eq!(code.blocker(), blocker);
    let log = rig.log();
    assert_eq!(steps(&log).last().copied(), Some(last_step), "{log:?}");
    assert!(
        !log.iter().any(|e| e == "start_outgoing" || e == "enter"),
        "{log:?}"
    );
    assert!(!s.routing.in_call_mode);
    assert_eq!(rig.ownership.holder(), None, "the microphone is released");
}

#[test]
fn not_running_from_the_bundle_blocks_first() {
    preflight_failure(
        |r| r.world.set_outside_bundle(),
        CallErrorCode::NotInBundle,
        Blocker::NotInBundle,
        "bundle",
    );
}

#[test]
fn no_call_application_blocks_before_any_consent_is_asked() {
    let mut rig = Rig::new();
    rig.coord.handle(CallCommand::Start {
        target_app: None,
        translate: false,
    });
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert_eq!(
        s.last_error.as_ref().map(|i| i.code),
        Some(CallErrorCode::NoCallApp)
    );
    assert_eq!(steps(&rig.log()), ["bundle"]);
}

#[test]
fn microphone_consent_denied_blocks() {
    preflight_failure(
        |r| {
            r.world
                .fail_once("mic_consent", CallErrorCode::MicrophoneConsentDenied)
        },
        CallErrorCode::MicrophoneConsentDenied,
        Blocker::PermissionDenied,
        "mic_consent",
    );
}

#[test]
fn capture_consent_denied_blocks() {
    preflight_failure(
        |r| {
            r.world
                .fail_once("capture_consent", CallErrorCode::CaptureConsentDenied)
        },
        CallErrorCode::CaptureConsentDenied,
        Blocker::PermissionDenied,
        "capture_consent",
    );
}

#[test]
fn a_missing_driver_blocks() {
    preflight_failure(
        |r| r.world.fail_once("driver", CallErrorCode::DriverMissing),
        CallErrorCode::DriverMissing,
        Blocker::DriverUnusable,
        "driver",
    );
}

#[test]
fn a_wrong_driver_variant_blocks() {
    preflight_failure(
        |r| {
            r.world
                .fail_once("driver", CallErrorCode::DriverWrongVariant)
        },
        CallErrorCode::DriverWrongVariant,
        Blocker::DriverUnusable,
        "driver",
    );
}

#[test]
fn no_microphone_blocks() {
    preflight_failure(
        |r| r.world.fail_once("mic_check", CallErrorCode::NoMicrophone),
        CallErrorCode::NoMicrophone,
        Blocker::NoMicrophone,
        "mic_check",
    );
}

#[test]
fn failing_to_open_the_outgoing_path_leaves_nothing_behind() {
    let mut rig = Rig::new();
    rig.world.fail_once(
        "start_outgoing",
        CallErrorCode::EndpointOpenFailed(Endpoint::RemoteOutput),
    );
    rig.start();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert_eq!(
        s.last_error.as_ref().map(|i| i.code),
        Some(CallErrorCode::EndpointOpenFailed(Endpoint::RemoteOutput))
    );
    let log = rig.log();
    assert!(
        !log.contains(&"enter".to_owned()),
        "the default input was never touched: {log:?}"
    );
    assert!(!log.contains(&"leave".to_owned()));
    assert_eq!(rig.ownership.holder(), None);
}

#[test]
fn failing_to_switch_the_default_input_closes_the_outgoing_path() {
    let mut rig = Rig::new();
    rig.world
        .fail_once("enter", CallErrorCode::RoutingSwitchFailed);
    rig.start();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert_eq!(
        s.last_error.as_ref().map(|i| i.code),
        Some(CallErrorCode::RoutingSwitchFailed)
    );
    let log = rig.log();
    assert!(log.contains(&"outgoing.stop".to_owned()), "{log:?}");
    assert!(!log.contains(&"start_incoming".to_owned()), "{log:?}");
    assert!(!s.routing.in_call_mode);
    assert!(s.direction(Direction::Outgoing).path.is_none());
}

#[test]
fn failing_to_tap_the_application_restores_the_microphone_then_closes_the_feeder() {
    let mut rig = Rig::new();
    rig.world
        .fail_once("start_incoming", CallErrorCode::TargetNotRunning);
    rig.start();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert_eq!(
        s.last_error.as_ref().map(|i| i.code),
        Some(CallErrorCode::TargetNotRunning)
    );
    let log = rig.log();
    let leave = log.iter().position(|e| e == "leave").expect("routing left");
    let stop = log
        .iter()
        .position(|e| e == "outgoing.stop")
        .expect("feeder closed");
    assert!(
        leave < stop,
        "the default input returns before the feeder stops: {log:?}"
    );
    assert!(!rig.world.in_call_mode());
    assert!(s.direction(Direction::Outgoing).path.is_none());
}

#[test]
fn a_restore_failure_while_unwinding_still_closes_the_endpoints() {
    let mut rig = Rig::new();
    rig.world
        .fail_once("start_incoming", CallErrorCode::TargetNotRunning);
    rig.world.fail_once("leave", CallErrorCode::RestoreFailed);
    rig.start();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert!(rig.has_issue(CallErrorCode::RestoreFailed));
    assert!(rig.log().contains(&"outgoing.stop".to_owned()));
    assert!(
        s.routing.in_call_mode,
        "the journal stays: the next start retries the restore"
    );
    // The next start first retries the restore.
    rig.world.clear_log();
    rig.start();
    let log = rig.log();
    let leave = log.iter().position(|e| e == "leave");
    let enter = log.iter().position(|e| e == "enter");
    assert!(leave.is_some() && leave < enter, "{log:?}");
}

#[test]
fn a_failing_activity_probe_is_shown_and_does_not_block() {
    let mut rig = Rig::new();
    rig.world
        .fail_once("probe", CallErrorCode::ActivityUnavailable);
    rig.start();
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
    assert!(rig.has_issue(CallErrorCode::ActivityUnavailable));
}

#[test]
fn starting_twice_is_rejected_and_changes_nothing() {
    let mut rig = Rig::new();
    rig.start();
    rig.world.clear_log();
    rig.start();
    assert!(rig.has_issue(CallErrorCode::CommandRejected));
    assert!(rig.log().is_empty());
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
}
