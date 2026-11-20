//! Call mode and an assistant session are mutually exclusive.

use super::kit::Rig;
use crate::call::{CallErrorCode, MicBusy, MicOwner};
use crate::call_state::CallState;

#[test]
fn call_mode_is_refused_while_an_assistant_session_owns_the_microphone() {
    let mut rig = Rig::new();
    let assistant = rig.ownership.claim(MicOwner::Assistant).expect("free");
    rig.start();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    assert!(rig.has_issue(CallErrorCode::AssistantActive));
    assert!(rig.log().is_empty(), "nothing was checked or opened: {:?}", rig.log());
    drop(assistant);
    rig.start();
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
}

#[test]
fn an_assistant_session_is_refused_while_call_mode_runs_and_after_it_may_start() {
    let mut rig = Rig::new();
    rig.start();
    assert_eq!(
        rig.ownership.claim(MicOwner::Assistant).unwrap_err(),
        MicBusy { held_by: MicOwner::Call }
    );
    rig.stop();
    assert!(rig.ownership.claim(MicOwner::Assistant).is_ok());
}

#[test]
fn a_failed_start_gives_the_microphone_back() {
    let mut rig = Rig::new();
    rig.world.fail_once("driver", CallErrorCode::DriverMissing);
    rig.start();
    assert!(rig.ownership.claim(MicOwner::Assistant).is_ok());
}
