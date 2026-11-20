//! Stopping from every state: routing always left, endpoints closed, the machine back in Idle.

use prxvc_media::Direction;

use super::kit::Rig;
use crate::call::{CallErrorCode, PathEvent, RoutingNotice};
use crate::call_state::CallState;
use crate::dir_set::DirSet;
use crate::fault::ProviderRole;

fn assert_stopped_cleanly(rig: &Rig) {
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert!(!s.routing.in_call_mode);
    assert!(s.direction(Direction::Outgoing).path.is_none());
    assert!(s.direction(Direction::Incoming).path.is_none());
    assert!(!rig.world.in_call_mode());
    assert_eq!(rig.ownership.holder(), None);
    let log = rig.log();
    for entry in ["leave", "incoming.stop", "outgoing.stop", "probe.stop"] {
        assert!(
            log.contains(&entry.to_owned()),
            "{entry} missing in {log:?}"
        );
    }
}

#[test]
fn stop_from_audio_ready_restores_first_and_closes_the_feeder_last() {
    let mut rig = Rig::new();
    rig.start();
    rig.world.clear_log();
    rig.stop();
    assert_stopped_cleanly(&rig);
    let log = rig.log();
    let at = |e: &str| {
        log.iter()
            .position(|x| x == e)
            .unwrap_or_else(|| panic!("{e} in {log:?}"))
    };
    assert!(at("leave") < at("incoming.stop"));
    assert!(at("incoming.stop") < at("outgoing.stop"));
    assert_eq!(
        rig.last().routing.restored_to.as_deref(),
        Some("Studio Mic")
    );
}

#[test]
fn stop_from_translating_degraded_and_paused() {
    // Translating.
    let mut rig = Rig::new();
    rig.start_translating();
    assert_eq!(rig.last().state, CallState::Translating);
    rig.stop();
    assert_stopped_cleanly(&rig);

    // Degraded: the providers failed.
    let mut rig = Rig::new();
    rig.world.fail_providers(ProviderRole::Mt);
    rig.start_translating();
    assert!(matches!(rig.last().state, CallState::Degraded { .. }));
    rig.stop();
    assert_stopped_cleanly(&rig);

    // Paused.
    let mut rig = Rig::new();
    rig.start_translating();
    rig.coord
        .handle(crate::call::CallCommand::Pause(DirSet::BOTH));
    assert!(matches!(rig.last().state, CallState::Paused { .. }));
    rig.stop();
    assert_stopped_cleanly(&rig);
}

#[test]
fn stop_from_recovering() {
    let mut rig = Rig::new();
    rig.start();
    rig.world.push_notice(RoutingNotice::SystemSleeping);
    rig.tick_after(1);
    assert!(matches!(rig.last().state, CallState::Recovering { .. }));
    rig.stop();
    assert_stopped_cleanly(&rig);
}

#[test]
fn stop_while_idle_is_a_quiet_no_op() {
    let mut rig = Rig::new();
    rig.stop();
    assert!(rig.log().is_empty());
    assert_eq!(rig.last().state, CallState::Idle);
    rig.start();
    rig.stop();
    rig.world.clear_log();
    let published = rig.snapshots.lock().len();
    rig.stop();
    assert!(rig.log().is_empty());
    assert_eq!(
        rig.snapshots.lock().len(),
        published,
        "nothing changed, nothing published"
    );
    assert!(!rig.has_issue(CallErrorCode::CommandRejected));
}

#[test]
fn a_path_that_does_not_stop_in_time_is_reported_but_the_routing_is_restored_first() {
    let mut rig = Rig::new();
    rig.start();
    rig.world.path_stop_leaks("outgoing");
    rig.world.clear_log();
    rig.stop();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert!(rig.has_issue(CallErrorCode::StopLeak));
    let log = rig.log();
    assert!(log.contains(&"leave".to_owned()));
    assert!(
        log.contains(&"incoming.stop".to_owned()),
        "the other path is still closed"
    );
    assert!(!s.routing.in_call_mode);
}

#[test]
fn a_failed_restore_is_shown_and_the_endpoints_still_close() {
    let mut rig = Rig::new();
    rig.start();
    rig.world.fail_once("leave", CallErrorCode::RestoreFailed);
    rig.stop();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert!(rig.has_issue(CallErrorCode::RestoreFailed));
    assert!(rig.log().contains(&"outgoing.stop".to_owned()));
    assert!(
        s.routing.in_call_mode,
        "the journal and call mode stay for the next start"
    );
    assert_eq!(
        s.last_error.as_ref().map(|i| i.code),
        Some(CallErrorCode::RestoreFailed)
    );
}

#[test]
fn a_stopped_call_ignores_stale_path_events() {
    let mut rig = Rig::new();
    rig.start();
    rig.stop();
    rig.world
        .push_path_event("outgoing", PathEvent::SignalRestored);
    rig.tick_after(1);
    assert_eq!(rig.last().state, CallState::Idle);
}
