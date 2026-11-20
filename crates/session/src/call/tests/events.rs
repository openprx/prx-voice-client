//! Events while a call runs: lost virtual microphone, displaced default input, capture without signal, lost paths, sleep and wake.

use prxvc_media::Direction;

use super::kit::Rig;
use crate::call::{
    ActivityJudgement, CallCommand, CallErrorCode, PathEvent, PathStats, RoutingNotice,
};
use crate::call_state::{CallReason, CallState};
use crate::fault::Endpoint;

fn running() -> Rig {
    let mut rig = Rig::new();
    rig.start();
    rig.world.clear_log();
    rig
}

#[test]
fn a_lost_virtual_microphone_restores_the_default_input_at_once_and_recovery_brings_it_back() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::VirtualMicrophoneLost);
    rig.world
        .fail_always("start_outgoing", CallErrorCode::DriverNotLoaded);
    rig.tick_after(1);
    let s = rig.last();
    assert!(
        matches!(s.state, CallState::Recovering { .. }),
        "{:?}",
        s.state
    );
    assert!(rig.has_issue(CallErrorCode::VirtualMicrophoneLost));
    assert!(!s.routing.in_call_mode, "the user's microphone is back");
    let log = rig.log();
    assert!(log.contains(&"leave".to_owned()), "{log:?}");
    let leave = log.iter().position(|e| e == "leave").unwrap();
    assert!(
        !log[..leave].contains(&"start_outgoing".to_owned()),
        "restore comes before any attempt to reopen"
    );

    // The driver comes back: the path reopens and call mode is entered again.
    rig.world.heal("start_outgoing");
    rig.world.clear_log();
    rig.tick_after(1);
    let s = rig.last();
    assert_eq!(s.state, CallState::AudioReady { translate: false });
    assert!(s.routing.in_call_mode);
    assert!(!rig.has_issue(CallErrorCode::VirtualMicrophoneLost));
    let log = rig.log();
    let reopen = log
        .iter()
        .position(|e| e == "start_outgoing")
        .expect("reopened");
    let enter = log
        .iter()
        .position(|e| e == "enter")
        .expect("entered again");
    assert!(
        reopen < enter,
        "the feeder runs before the default input switches: {log:?}"
    );
}

#[test]
fn a_virtual_microphone_that_never_returns_degrades_the_call_after_the_recovery_timeout() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::VirtualMicrophoneLost);
    rig.world
        .fail_always("start_outgoing", CallErrorCode::DriverNotLoaded);
    rig.tick_after(1);
    assert!(matches!(rig.last().state, CallState::Recovering { .. }));
    for _ in 0..9 {
        rig.tick_after(1);
    }
    rig.tick_after(1);
    let s = rig.last();
    match &s.state {
        CallState::Degraded { reasons } => {
            assert!(reasons.contains(&CallReason::EndpointMissing(Endpoint::RemoteOutput)));
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
    assert!(rig.has_issue(CallErrorCode::RecoveryTimeout));
}

#[test]
fn a_displaced_default_input_is_shown_and_reasserted_only_on_command() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::DefaultInputDisplaced {
        to: "AirPods".into(),
    });
    rig.tick_after(1);
    let s = rig.last();
    assert!(rig.has_issue(CallErrorCode::DefaultInputDisplaced));
    assert!(!s.routing.default_input_is_ours);
    assert_eq!(s.routing.displaced_to.as_deref(), Some("AirPods"));
    assert_eq!(
        s.state,
        CallState::AudioReady { translate: false },
        "the call itself is untouched"
    );

    // Time passes: nothing is re-applied behind the user's back.
    rig.tick_after(5);
    rig.tick_after(5);
    assert!(
        !rig.log().contains(&"reassert".to_owned()),
        "{:?}",
        rig.log()
    );

    rig.coord.handle(CallCommand::ReapplyVirtualMicrophone);
    assert!(rig.log().contains(&"reassert".to_owned()));
    let s = rig.last();
    assert!(s.routing.default_input_is_ours);
    assert!(!rig.has_issue(CallErrorCode::DefaultInputDisplaced));
}

#[test]
fn reselecting_our_microphone_by_hand_clears_the_displacement() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::DefaultInputDisplaced {
        to: "AirPods".into(),
    });
    rig.tick_after(1);
    rig.world
        .push_notice(RoutingNotice::OwnMicrophoneReselected);
    rig.tick_after(1);
    assert!(!rig.has_issue(CallErrorCode::DefaultInputDisplaced));
    assert!(rig.last().routing.default_input_is_ours);
}

#[test]
fn a_failed_reassert_is_shown_and_keeps_the_displacement() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::DefaultInputDisplaced {
        to: "AirPods".into(),
    });
    rig.tick_after(1);
    rig.world
        .fail_once("reassert", CallErrorCode::VirtualMicrophoneLost);
    rig.coord.handle(CallCommand::ReapplyVirtualMicrophone);
    assert!(rig.has_issue(CallErrorCode::VirtualMicrophoneLost));
    assert!(rig.has_issue(CallErrorCode::DefaultInputDisplaced));
}

#[test]
fn reapplying_outside_call_mode_is_rejected() {
    let mut rig = Rig::new();
    rig.coord.handle(CallCommand::ReapplyVirtualMicrophone);
    assert!(rig.has_issue(CallErrorCode::CommandRejected));
    assert!(rig.log().is_empty());
}

#[test]
fn the_zero_watchdog_surfaces_as_a_typed_error_and_clears_when_signal_returns() {
    let mut rig = running();
    rig.world.push_path_event(
        "incoming",
        PathEvent::NoSignal("no signal from net.whatsapp.WhatsApp".into()),
    );
    rig.tick_after(1);
    assert!(rig.has_issue(CallErrorCode::CaptureNoSignal));
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
    rig.world
        .push_path_event("incoming", PathEvent::SignalRestored);
    rig.tick_after(1);
    assert!(!rig.has_issue(CallErrorCode::CaptureNoSignal));
}

#[test]
fn revoked_capture_consent_and_a_quitting_application_are_shown() {
    let mut rig = running();
    rig.world.push_path_event(
        "incoming",
        PathEvent::Fault(crate::call::HostFault::new(
            CallErrorCode::CaptureConsentDenied,
            "revoked",
        )),
    );
    rig.world.push_path_event("incoming", PathEvent::TargetGone);
    rig.tick_after(1);
    assert!(rig.has_issue(CallErrorCode::CaptureConsentDenied));
    assert!(rig.has_issue(CallErrorCode::TargetGone));
    rig.world.push_path_event("incoming", PathEvent::TargetBack);
    rig.tick_after(1);
    assert!(!rig.has_issue(CallErrorCode::TargetGone));
}

#[test]
fn microphone_changes_update_the_device_name_and_unavailability_is_shown() {
    let mut rig = running();
    rig.world
        .push_path_event("outgoing", PathEvent::DeviceUnavailable("none".into()));
    rig.tick_after(1);
    assert!(rig.has_issue(CallErrorCode::MicrophoneUnavailable));
    assert_eq!(rig.last().devices.microphone, None);
    rig.world
        .push_path_event("outgoing", PathEvent::DeviceChanged("USB Mic".into()));
    rig.tick_after(1);
    assert!(!rig.has_issue(CallErrorCode::MicrophoneUnavailable));
    assert_eq!(rig.last().devices.microphone.as_deref(), Some("USB Mic"));
}

#[test]
fn a_stalled_tap_is_shown_until_audio_flows_again() {
    let mut rig = running();
    rig.world
        .push_path_event("incoming", PathEvent::Stalled { idle_ms: 2_000 });
    rig.tick_after(1);
    assert!(rig.has_issue(CallErrorCode::CaptureStalled));
    rig.world.set_path_stats(
        "incoming",
        PathStats {
            frames: 10,
            ..PathStats::default()
        },
    );
    rig.tick_after(1);
    assert!(!rig.has_issue(CallErrorCode::CaptureStalled));
}

#[test]
fn a_lost_path_goes_through_recovery_and_reopens() {
    let mut rig = running();
    rig.world.push_path_event(
        "outgoing",
        PathEvent::Lost(crate::call::HostFault::new(
            CallErrorCode::EndpointLost(Endpoint::LocalInput),
            "usb mic unplugged",
        )),
    );
    rig.tick_after(1);
    // The loss is published, the first reopening attempt follows at once.
    let states: Vec<CallState> = rig
        .snapshots
        .lock()
        .iter()
        .map(|s| s.state.clone())
        .collect();
    assert!(
        states
            .iter()
            .any(|s| matches!(s, CallState::Recovering { .. })),
        "{states:?}"
    );
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
    let log = rig.log();
    let stop = log
        .iter()
        .position(|e| e == "outgoing.stop")
        .expect("the dead path is released");
    let start = log
        .iter()
        .position(|e| e == "start_outgoing")
        .expect("reopened");
    assert!(stop < start, "{log:?}");
    assert!(rig.last().direction(Direction::Outgoing).path.is_some());
}

#[test]
fn sleep_suspends_recovery_and_wake_reopens_every_path() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::SystemSleeping);
    rig.tick_after(1);
    assert!(matches!(rig.last().state, CallState::Recovering { .. }));
    rig.world.clear_log();
    // Asleep: no attempts and no timeout, however long it takes.
    for _ in 0..30 {
        rig.tick_after(1);
    }
    let log = rig.log();
    assert!(!log.contains(&"start_outgoing".to_owned()), "{log:?}");
    assert!(matches!(rig.last().state, CallState::Recovering { .. }));

    rig.world.push_notice(RoutingNotice::SystemWoke {
        default_input_intact: true,
    });
    rig.tick_after(1);
    let log = rig.log();
    assert!(log.contains(&"start_outgoing".to_owned()), "{log:?}");
    assert!(log.contains(&"start_incoming".to_owned()), "{log:?}");
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
}

#[test]
fn a_wake_that_cannot_reopen_a_path_retries_and_then_degrades() {
    let mut rig = running();
    rig.world.push_notice(RoutingNotice::SystemSleeping);
    rig.tick_after(1);
    rig.world
        .fail_always("start_incoming", CallErrorCode::TargetNotRunning);
    rig.world.push_notice(RoutingNotice::SystemWoke {
        default_input_intact: true,
    });
    rig.tick_after(1);
    assert!(matches!(rig.last().state, CallState::Recovering { .. }));
    for _ in 0..11 {
        rig.tick_after(1);
    }
    assert!(
        matches!(rig.last().state, CallState::Degraded { .. }),
        "{:?}",
        rig.last().state
    );
}

#[test]
fn call_activity_on_another_microphone_is_shown_and_the_judgement_is_in_the_snapshot() {
    let mut rig = running();
    rig.world
        .set_activity(ActivityJudgement::ActiveOnOtherMicrophone {
            devices: vec!["AirPods".into()],
        });
    rig.tick_after(1);
    assert!(rig.has_issue(CallErrorCode::CallOnOtherMicrophone));
    assert!(matches!(
        rig.last().routing.call_activity,
        ActivityJudgement::ActiveOnOtherMicrophone { .. }
    ));
    rig.world
        .set_activity(ActivityJudgement::ActiveOnOurMicrophone {
            output_running: true,
        });
    rig.tick_after(1);
    assert!(!rig.has_issue(CallErrorCode::CallOnOtherMicrophone));
}

#[test]
fn the_routing_controller_is_polled_about_once_a_second_not_every_tick() {
    let mut rig = running();
    for _ in 0..4 {
        rig.clock.advance(std::time::Duration::from_millis(250));
        rig.coord.tick();
    }
    let polls = rig.log().iter().filter(|e| *e == "poll").count();
    assert_eq!(polls, 1, "{:?}", rig.log());
}

#[test]
fn health_counters_and_levels_reach_the_snapshot() {
    let mut rig = running();
    rig.world.set_path_stats(
        "outgoing",
        PathStats {
            frames: 100,
            peak: 0.4,
            rms: 0.1,
            overruns: 3,
            underruns: 2,
            ..PathStats::default()
        },
    );
    rig.tick_after(1);
    let stats = rig
        .last()
        .direction(Direction::Outgoing)
        .path
        .clone()
        .expect("running");
    assert_eq!((stats.frames, stats.overruns, stats.underruns), (100, 3, 2));
    assert!((stats.peak - 0.4).abs() < 1e-6);
}

#[test]
fn setting_the_local_output_mode_is_recorded_and_changes_nothing_audible_in_m1() {
    let mut rig = running();
    rig.coord.handle(CallCommand::SetLocalOutputMode(
        crate::call::LocalOutputMode::Original,
    ));
    let s = rig.last();
    assert_eq!(s.local_output_mode, crate::call::LocalOutputMode::Original);
    assert_eq!(
        rig.hears(Direction::Incoming),
        crate::output::OutputSource::Original
    );
    assert!(rig.log().iter().all(|e| !e.contains("hears")));
}
