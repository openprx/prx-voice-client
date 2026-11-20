//! The coordinator thread: commands in, snapshots out, a bounded shutdown and no thread left behind.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_media::SystemClock;
use prxvc_ports::BundleId;

use super::kit::{APP, FakeHost, World};
use crate::call::{
    CallCommand, CallHandle, CallSettings, CallSnapshot, CoordinatorDeps, MicOwnership, SendError,
};
use crate::call_state::CallState;

fn spawn(world: &World, snapshots: &Arc<Mutex<Vec<CallSnapshot>>>) -> CallHandle {
    let (world, snapshots) = (world.clone(), Arc::clone(snapshots));
    CallHandle::spawn(move || CoordinatorDeps {
        host: Box::new(FakeHost::new(world)),
        clock: Arc::new(SystemClock::new()),
        ownership: MicOwnership::new(),
        sink: Box::new(move |s| snapshots.lock().push(s.clone())),
        settings: CallSettings::default(),
    })
    .expect("thread")
}

fn wait_for(
    snapshots: &Arc<Mutex<Vec<CallSnapshot>>>,
    what: &str,
    f: impl Fn(&CallSnapshot) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !snapshots.lock().last().is_some_and(&f) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn start_and_stop_through_the_thread_publish_snapshots_and_end_cleanly() {
    let world = World::new();
    let snapshots: Arc<Mutex<Vec<CallSnapshot>>> = Arc::default();
    let mut handle = spawn(&world, &snapshots);
    handle
        .send(CallCommand::Start {
            target_app: Some(BundleId::new(APP)),
            translate: false,
        })
        .expect("queued");
    wait_for(&snapshots, "audio ready", |s| {
        matches!(s.state, CallState::AudioReady { .. })
    });
    handle.send(CallCommand::Stop).expect("queued");
    wait_for(&snapshots, "idle again", |s| {
        s.state == CallState::Idle && s.revision > 2
    });
    handle
        .shutdown(Duration::from_secs(2))
        .expect("the thread ends");
    assert_eq!(
        handle.send(CallCommand::Stop),
        Err(SendError::Ended),
        "no coordinator thread is left to take commands"
    );
}

#[test]
fn shutdown_while_a_call_runs_stops_it_and_restores_the_microphone() {
    let world = World::new();
    let snapshots: Arc<Mutex<Vec<CallSnapshot>>> = Arc::default();
    let mut handle = spawn(&world, &snapshots);
    handle
        .send(CallCommand::Start {
            target_app: Some(BundleId::new(APP)),
            translate: false,
        })
        .expect("queued");
    wait_for(&snapshots, "audio ready", |s| {
        matches!(s.state, CallState::AudioReady { .. })
    });
    handle
        .shutdown(Duration::from_secs(2))
        .expect("the thread ends");
    assert!(world.log().contains(&"leave".to_owned()));
    assert!(!world.in_call_mode());
    assert_eq!(
        snapshots.lock().last().map(|s| s.state.clone()),
        Some(CallState::Idle)
    );
}

#[test]
fn dropping_the_handle_ends_the_thread_too() {
    let world = World::new();
    let snapshots: Arc<Mutex<Vec<CallSnapshot>>> = Arc::default();
    let handle = spawn(&world, &snapshots);
    handle
        .send(CallCommand::Start {
            target_app: Some(BundleId::new(APP)),
            translate: false,
        })
        .expect("queued");
    wait_for(&snapshots, "audio ready", |s| {
        matches!(s.state, CallState::AudioReady { .. })
    });
    drop(handle);
    assert!(world.log().contains(&"leave".to_owned()));
}

#[test]
fn the_tick_publishes_path_counters_while_the_call_runs() {
    let world = World::new();
    let snapshots: Arc<Mutex<Vec<CallSnapshot>>> = Arc::default();
    let mut handle = spawn(&world, &snapshots);
    handle
        .send(CallCommand::Start {
            target_app: Some(BundleId::new(APP)),
            translate: false,
        })
        .expect("queued");
    wait_for(&snapshots, "audio ready", |s| {
        matches!(s.state, CallState::AudioReady { .. })
    });
    world.set_path_stats(
        "incoming",
        crate::call::PathStats {
            frames: 7,
            peak: 0.5,
            ..crate::call::PathStats::default()
        },
    );
    wait_for(&snapshots, "the incoming counters", |s| {
        s.direction(prxvc_media::Direction::Incoming)
            .path
            .as_ref()
            .is_some_and(|p| p.frames == 7)
    });
    handle.shutdown(Duration::from_secs(2)).expect("ends");
}
