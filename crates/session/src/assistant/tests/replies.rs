//! The reply: it streams (first audio before the text is complete), it is cut by the user and by the orb.

use std::sync::atomic::Ordering;

use prxvc_ports::fakes::{DialogueScript, EventKind, FakeDialogue};

use super::super::{AssistantEvent, Phase};
use super::rig::{ONE_UTTERANCE, Rig, RigConfig, ms, vad_runs};

fn slow_dialogue(cfg: &mut RigConfig, clock: &prxvc_media::ManualClock, tokens: &[&'static str]) {
    cfg.dialogue = Some(FakeDialogue::new(clock.clone()).with_default_script(
        DialogueScript::tokens(tokens.iter().copied()).with_token_delay(ms(100)),
    ));
}

#[test]
fn the_first_audio_plays_before_the_reply_text_is_complete() {
    let (mut cfg, clock) = RigConfig::new("what time is it", ["x"]);
    slow_dialogue(&mut cfg, &clock, &["It is noon!", " Anything else?"]);
    let mut rig = Rig::build(cfg, clock.clone());
    let log = rig.dialogue.log();
    rig.start().expect("starts");
    rig.speak();
    rig.wait_for("the turn", |e| matches!(e, AssistantEvent::UserTurn { .. }));
    log.wait_for(EventKind::Opened, 1);

    // One token arrives: its clause is synthesised and played while the rest is still to come.
    clock.advance(ms(100));
    rig.wait_phase(Phase::Speaking);
    assert!(
        rig.out.chunks.load(Ordering::SeqCst) >= 1,
        "audio is playing"
    );
    assert_eq!(rig.reply_text(), "It is noon!");
    assert!(
        !log.kinds_for("turn1").contains(&EventKind::Completed),
        "the dialogue has not finished: the first audio did not wait for the whole reply"
    );
    assert_eq!(rig.tts.requests().len(), 1);

    // The rest arrives and is spoken after it.
    clock.advance(ms(100));
    clock.advance(ms(100));
    rig.wait_until("the second clause", |r| r.tts.requests().len() == 2);
    rig.wait_until("the end of the audio", |r| {
        r.out.ends.load(Ordering::SeqCst) == 1
    });
    rig.out.drained.store(true, Ordering::SeqCst);
    rig.wait_for("heard", |e| *e == AssistantEvent::ReplyDone);
    rig.stop();
}

#[test]
fn trace_marks_follow_the_stages_of_one_turn() {
    use prxvc_ports::TraceStage as T;
    let mut rig = Rig::started("what time is it", ["It is noon!"]);
    rig.speak();
    rig.wait_for("the turn", |e| matches!(e, AssistantEvent::UserTurn { .. }));
    rig.wait_phase(Phase::Speaking);
    let stages = rig.trace.stages();
    let at = |s: T| stages.iter().position(|x| *x == s);
    let order = [
        T::SpeechStart,
        T::AsrPartial,
        T::MtRequestSent,
        T::MtFirstToken,
        T::TtsRequestSent,
        T::TtsFirstChunk,
    ];
    let mut last = 0;
    for stage in order {
        let i = at(stage).unwrap_or_else(|| panic!("{stage:?} was not recorded: {stages:?}"));
        assert!(i >= last, "{stage:?} out of order in {stages:?}");
        last = i;
    }
    rig.stop();
}

#[test]
fn the_user_speaking_over_the_reply_cuts_it_and_a_new_turn_follows() {
    let (mut cfg, clock) = RigConfig::new("first thing", ["Long answer!"]);
    cfg.vad = vad_runs(&[
        ONE_UTTERANCE[0],
        ONE_UTTERANCE[1],
        ONE_UTTERANCE[2],
        ONE_UTTERANCE[1],
        ONE_UTTERANCE[2],
    ]);
    cfg.asr = prxvc_ports::fakes::AsrScript::new(["first"], "first thing")
        .then_round([("stop", 0)], "stop please")
        .then_round([], "");
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig.speak();
    rig.wait_phase(Phase::Speaking);
    // The output still holds the reply: it is not heard to the end.
    assert!(!rig.out.drained.load(Ordering::SeqCst));

    rig.feed.push(ONE_UTTERANCE[1].1 + ONE_UTTERANCE[2].1);
    rig.wait_for("the reply is cut", |e| *e == AssistantEvent::ReplyCut);
    assert!(
        rig.out.flushes.load(Ordering::SeqCst) >= 1,
        "the output was silenced at once"
    );
    rig.wait_until("the second turn", |r| r.user_turns().len() == 2);
    assert_eq!(rig.user_turns(), ["first thing", "stop please"]);
    let phases = rig.phases();
    let speaking = phases
        .iter()
        .position(|p| *p == Phase::Speaking)
        .expect("spoke");
    assert_eq!(
        phases[speaking + 1],
        Phase::UserSpeaking,
        "the user's speech cut the reply directly"
    );
    assert!(
        !rig.seen.contains(&AssistantEvent::ReplyDone),
        "the first reply never finished"
    );
    rig.stop();
}

#[test]
fn an_interrupt_command_cuts_the_reply_and_the_session_goes_on_listening() {
    let mut rig = Rig::started("tell me more", ["A long story!"]);
    rig.speak();
    rig.wait_phase(Phase::Speaking);
    rig.handle().interrupt();
    rig.wait_for("the reply is cut", |e| *e == AssistantEvent::ReplyCut);
    rig.wait_phase(Phase::Listening);
    assert_eq!(rig.out.flushes.load(Ordering::SeqCst), 1);
    assert!(
        !rig.handle().is_finished(),
        "an interrupt does not end the session"
    );
    rig.stop();
}

#[test]
fn stopping_while_speaking_silences_the_output_and_ends_the_session() {
    let mut rig = Rig::started("tell me more", ["A long story!"]);
    rig.speak();
    rig.wait_phase(Phase::Speaking);
    rig.stop();
    assert!(rig.out.flushes.load(Ordering::SeqCst) >= 1);
    assert!(rig.seen.contains(&AssistantEvent::ReplyCut));
    assert_eq!(rig.phases().last(), Some(&Phase::Idle));
}
