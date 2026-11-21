//! Half duplex: the gate keeps the assistant's own voice out of the recogniser.

use std::sync::atomic::Ordering;

use super::super::{AssistantEvent, Phase};
use super::rig::{ONE_UTTERANCE, Rig, RigConfig};

fn gated(asr: Option<prxvc_ports::fakes::AsrScript>) -> Rig {
    let (mut cfg, clock) = RigConfig::new("what time is it", ["It is noon!"]);
    cfg.use_test_gate = true;
    if let Some(asr) = asr {
        cfg.asr = asr;
    }
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig
}

#[test]
fn frames_are_dropped_while_the_gate_is_closed_and_listening_resumes_when_it_reopens() {
    // Reopening the gate starts the recogniser afresh (its next round), so the utterance heard after it is round one.
    let asr = prxvc_ports::fakes::AsrScript::new([], "")
        .then_round([("what time", 0)], "what time is it")
        .then_round([], "");
    let mut rig = gated(Some(asr));
    rig.gate.closed.store(true, Ordering::SeqCst);
    rig.feed.push(10);
    rig.wait_for("the microphone is paused", |e| {
        *e == AssistantEvent::MicPaused(true)
    });
    assert!(
        !rig.phases().contains(&Phase::UserSpeaking),
        "nothing of it reached the recogniser"
    );

    rig.gate.reopened.store(true, Ordering::SeqCst);
    rig.gate.closed.store(false, Ordering::SeqCst);
    rig.speak();
    rig.wait_for("the microphone is back", |e| {
        *e == AssistantEvent::MicPaused(false)
    });
    rig.wait_for("a turn from the audio after the reopening", |e| {
        matches!(e, AssistantEvent::UserTurn { .. })
    });
    assert_eq!(rig.user_turns(), ["what time is it"]);
    let paused_back = rig
        .seen
        .iter()
        .position(|e| *e == AssistantEvent::MicPaused(false))
        .expect("reopened");
    let first_speech = rig
        .seen
        .iter()
        .position(|e| *e == AssistantEvent::Phase(Phase::UserSpeaking))
        .expect("heard");
    assert!(paused_back < first_speech);
    let _ = ONE_UTTERANCE;
    rig.stop();
}

#[test]
fn the_gate_follows_the_reply_audio_and_a_cut() {
    let mut rig = gated(None);
    rig.speak();
    rig.wait_phase(Phase::Speaking);
    rig.wait_until("the gate saw the reply", |r| {
        r.gate.activity.load(Ordering::SeqCst) >= 1
    });
    rig.wait_until("the end of the audio", |r| {
        r.gate.ended.load(Ordering::SeqCst) == 1
    });
    rig.handle().interrupt();
    rig.wait_for("cut", |e| *e == AssistantEvent::ReplyCut);
    assert!(
        rig.gate.flushed.load(Ordering::SeqCst) >= 1,
        "the gate is told the output was flushed"
    );
    rig.stop();
}
