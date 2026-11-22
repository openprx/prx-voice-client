//! The graceful stop: the last sentences are played before call mode ends, bounded by the limit, and a second Stop ends it at once.

use std::time::Duration;

use prxvc_media::Direction;

use super::kit::{Played, Rig};
use super::translation::{clause, script_one_utterance, speak, translating_both};
use crate::call::{CallCommand, CallSettings, PlaybackReport};
use crate::call_state::CallState;

fn rig(grace_ms: u64) -> Rig {
    let rig = Rig::with(CallSettings {
        stop_grace: Duration::from_millis(grace_ms),
        ..CallSettings::default()
    });
    script_one_utterance(&rig);
    rig
}

/// Speech with no pause after it: the stop arrives while the segment is open.
fn say_without_pause(rig: &Rig) {
    speak(rig, "outgoing", 24);
}

#[test]
fn stop_with_a_sentence_in_flight_waits_for_it_and_plays_it_to_the_end() {
    let mut rig = rig(6_000);
    rig.start_translating();
    translating_both(&mut rig);
    say_without_pause(&rig);
    rig.stop();
    let s = rig.last();
    assert!(s.finishing_s.is_some(), "the snapshot says it is finishing");
    assert_eq!(s.state, CallState::Translating, "the call does not end yet");
    let first = clause(Direction::Outgoing, 1, 0);
    rig.until_world(
        "the open sentence was committed, translated and handed to the lane",
        |w| w.played("outgoing").contains(&Played::End(first)),
    );
    assert!(
        !rig.world.played("outgoing").contains(&Played::Flush),
        "nothing was cut"
    );
    // The mixer plays it; then the call ends by itself.
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Started {
            clause: first,
            at: prxvc_media::Clock::now(&rig.clock),
        },
    );
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Finished {
            clause: first,
            at: prxvc_media::Clock::now(&rig.clock),
        },
    );
    for _ in 0..40 {
        rig.tick_after(1);
        if rig.last().state == CallState::Idle {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(rig.last().state, CallState::Idle);
    assert!(rig.last().finishing_s.is_none());
}

#[test]
fn the_limit_ends_a_stop_that_would_wait_for_ever() {
    let mut rig = rig(3_000);
    rig.start_translating();
    translating_both(&mut rig);
    say_without_pause(&rig);
    rig.stop();
    let first = clause(Direction::Outgoing, 1, 0);
    rig.until_world("handed to the lane", |w| {
        w.played("outgoing").contains(&Played::End(first))
    });
    // Playback is never reported as finished: the limit ends the stop.
    rig.tick_after(1);
    assert!(rig.last().finishing_s.is_some());
    rig.tick_after(3);
    assert_eq!(rig.last().state, CallState::Idle);
}

#[test]
fn a_second_stop_ends_at_once() {
    let mut rig = rig(60_000);
    rig.start_translating();
    translating_both(&mut rig);
    say_without_pause(&rig);
    rig.stop();
    assert!(rig.last().finishing_s.is_some());
    rig.coord.handle(CallCommand::Stop);
    assert_eq!(rig.last().state, CallState::Idle);
}

#[test]
fn without_a_grace_period_or_a_translation_stop_acts_at_once() {
    let mut rig = rig(0);
    rig.start_translating();
    translating_both(&mut rig);
    rig.stop();
    assert_eq!(rig.last().state, CallState::Idle);

    let mut rig = Rig::with(CallSettings {
        stop_grace: Duration::from_secs(6),
        ..CallSettings::default()
    });
    rig.start();
    rig.stop();
    assert_eq!(
        rig.last().state,
        CallState::Idle,
        "audio only has nothing to finish"
    );
}

#[test]
fn shutdown_never_waits() {
    let mut rig = rig(60_000);
    rig.start_translating();
    translating_both(&mut rig);
    say_without_pause(&rig);
    rig.coord.shutdown();
    assert_eq!(rig.last().state, CallState::Idle);
}
