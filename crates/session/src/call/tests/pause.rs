//! Pause, resume and the failure policy: what each listener hears is decided by the direction machines and announced.

use prxvc_media::Direction;

use super::kit::Rig;
use crate::call::{CallCommand, CallErrorCode, CallSettings};
use crate::call_state::CallState;
use crate::dir_set::DirSet;
use crate::direction_state::DirectionState;
use crate::fault::ProviderRole;
use crate::output::{OutputCause, OutputPolicy, OutputSource, Passthrough};

fn silent_policy() -> OutputPolicy {
    OutputPolicy {
        when_paused: Passthrough::Silence,
        on_failure: Passthrough::Silence,
    }
}

#[test]
fn translating_listeners_hear_the_translation() {
    let mut rig = Rig::new();
    rig.start_translating();
    let s = rig.last();
    assert_eq!(s.state, CallState::Translating);
    assert_eq!(
        s.direction(Direction::Outgoing).state,
        DirectionState::Running
    );
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Translation);
    assert_eq!(rig.hears(Direction::Incoming), OutputSource::Translation);
    assert!(rig.log().contains(&"outgoing.hears:Translation".to_owned()));
}

#[test]
fn pause_plays_the_original_by_default_and_resume_brings_the_translation_back() {
    let mut rig = Rig::new();
    rig.start_translating();
    rig.coord
        .handle(CallCommand::Pause(DirSet::one(Direction::Outgoing)));
    let s = rig.last();
    assert!(matches!(s.state, CallState::Paused { .. }));
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Original);
    assert_eq!(
        rig.hears(Direction::Incoming),
        OutputSource::Translation,
        "the other direction is untouched"
    );
    let change = s.last_output_change.expect("announced");
    assert_eq!(
        (change.direction, change.from, change.to, change.cause),
        (
            Direction::Outgoing,
            OutputSource::Translation,
            OutputSource::Original,
            OutputCause::Paused
        )
    );
    assert!(
        rig.log().contains(&"outgoing.hears:Original".to_owned()),
        "the path crossfades"
    );

    rig.coord.handle(CallCommand::Resume);
    assert_eq!(rig.last().state, CallState::Translating);
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Translation);
    assert_eq!(
        rig.last().last_output_change.map(|c| c.cause),
        Some(OutputCause::TranslationResumed)
    );
}

#[test]
fn the_configured_silence_policy_makes_a_paused_listener_hear_nothing() {
    let mut rig = Rig::with(CallSettings {
        outgoing: silent_policy(),
        incoming: silent_policy(),
        ..CallSettings::default()
    });
    rig.start_translating();
    rig.coord.handle(CallCommand::Pause(DirSet::BOTH));
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Nothing);
    assert_eq!(rig.hears(Direction::Incoming), OutputSource::Nothing);
    assert!(rig.log().contains(&"outgoing.hears:Nothing".to_owned()));
    rig.coord.handle(CallCommand::Resume);
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Translation);
}

#[test]
fn pause_is_rejected_in_audio_ready_because_nothing_translates() {
    let mut rig = Rig::new();
    rig.start();
    rig.coord.handle(CallCommand::Pause(DirSet::BOTH));
    let s = rig.last();
    assert_eq!(s.state, CallState::AudioReady { translate: false });
    assert!(rig.has_issue(CallErrorCode::CommandRejected));
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Original);
    // The next accepted command withdraws the note.
    rig.coord.handle(CallCommand::Stop);
    assert!(!rig.has_issue(CallErrorCode::CommandRejected));
}

#[test]
fn a_provider_failure_applies_the_failure_policy_through_the_direction_machines() {
    let mut rig = Rig::new();
    rig.world.fail_providers(ProviderRole::Mt);
    rig.start_translating();
    let s = rig.last();
    assert!(matches!(s.state, CallState::Degraded { .. }));
    assert!(matches!(
        s.direction(Direction::Outgoing).state,
        DirectionState::Failed { .. }
    ));
    assert_eq!(
        rig.hears(Direction::Outgoing),
        OutputSource::Original,
        "the default policy is the original"
    );
    assert!(rig.has_issue(CallErrorCode::TranslationUnavailable));
}

#[test]
fn the_silence_failure_policy_is_honoured_and_announced() {
    let mut rig = Rig::with(CallSettings {
        outgoing: silent_policy(),
        incoming: silent_policy(),
        ..CallSettings::default()
    });
    rig.world.fail_providers(ProviderRole::Tts);
    rig.start_translating();
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Nothing);
    assert_eq!(rig.hears(Direction::Incoming), OutputSource::Nothing);
    let change = rig.last().last_output_change.expect("announced");
    assert_eq!(change.cause, OutputCause::Failed);
    assert_eq!(change.to, OutputSource::Nothing);
}

#[test]
fn a_paused_degraded_call_keeps_its_reasons_across_resume() {
    let mut rig = Rig::new();
    rig.world.fail_providers(ProviderRole::Asr);
    rig.start_translating();
    rig.coord.handle(CallCommand::Pause(DirSet::BOTH));
    assert!(matches!(rig.last().state, CallState::Paused { .. }));
    rig.coord.handle(CallCommand::Resume);
    assert!(matches!(rig.last().state, CallState::Degraded { .. }));
}
