//! Turn taking: what the user says becomes a request, the reply is spoken, partials stream, filler makes no turn.

use prxvc_media::Lang;
use prxvc_ports::ConversationId;
use prxvc_ports::fakes::{AsrScript, EventKind};

use super::super::{AssistantEvent, Phase};
use super::rig::{ONE_UTTERANCE, Rig, RigConfig, vad_runs};

/// Finish the reply the usual way: the output reports that it has played everything.
fn let_the_output_drain(rig: &mut Rig) {
    rig.wait_until("the end of the reply's audio", |r| {
        r.out.ends.load(std::sync::atomic::Ordering::SeqCst) >= 1
    });
    rig.out
        .drained
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

#[test]
fn a_turn_is_recognised_answered_and_spoken_in_order() {
    let mut rig = Rig::started("what time is it", ["It is noon!", " Anything else?"]);
    rig.speak();
    rig.wait_for("the turn", |e| matches!(e, AssistantEvent::UserTurn { .. }));
    rig.wait_phase(Phase::Speaking);
    let_the_output_drain(&mut rig);
    rig.wait_for("the reply is heard", |e| *e == AssistantEvent::ReplyDone);
    rig.wait_phase(Phase::Listening);

    assert_eq!(
        rig.phases(),
        [
            Phase::Listening,
            Phase::UserSpeaking,
            Phase::Thinking,
            Phase::Speaking,
            Phase::Listening
        ]
    );
    assert_eq!(rig.user_turns(), ["what time is it"]);
    assert_eq!(rig.reply_text(), "It is noon! Anything else?");

    let requests = rig.dialogue.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages[0].text, "what time is it");
    assert_eq!(requests[0].conversation, ConversationId::new("session-1"));
    assert_eq!(requests[0].lang, Lang::En);
    let spoken: Vec<_> = rig.tts.requests().into_iter().map(|r| r.text).collect();
    assert_eq!(
        spoken,
        ["It is noon!", "Anything else?"],
        "one request per clause, in order"
    );
    assert_eq!(rig.out.chunks.load(std::sync::atomic::Ordering::SeqCst), 4);
    assert_eq!(
        rig.out.flushes.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing was cut"
    );
    rig.stop();
    assert!(
        rig.feed.stopped.load(std::sync::atomic::Ordering::SeqCst),
        "the microphone is released"
    );
    assert_eq!(rig.dialogue.ended(), [ConversationId::new("session-1")]);
}

#[test]
fn recognition_streams_partials_into_the_transcript_before_the_final_turn() {
    let mut rig = Rig::started("what time is it", ["Noon."]);
    rig.speak();
    rig.wait_for("the turn", |e| matches!(e, AssistantEvent::UserTurn { .. }));
    let partials: Vec<_> = rig
        .seen
        .iter()
        .filter_map(|e| match e {
            AssistantEvent::Partial { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        partials.first(),
        Some(&"wha"),
        "the first partial shows while the user still speaks"
    );
    assert!(partials.contains(&"what time"));
    assert_eq!(
        partials.last(),
        Some(&"what time is it"),
        "the final text closes the line"
    );
    let first_partial = rig
        .seen
        .iter()
        .position(|e| matches!(e, AssistantEvent::Partial { .. }))
        .expect("a partial");
    let turn = rig
        .seen
        .iter()
        .position(|e| matches!(e, AssistantEvent::UserTurn { .. }))
        .expect("a turn");
    assert!(first_partial < turn);
    rig.stop();
}

#[test]
fn filler_only_speech_creates_no_turn_and_no_request() {
    let (mut cfg, clock) = RigConfig::new("嗯。", ["unused"]);
    cfg.lang = Lang::Zh;
    cfg.asr = AsrScript::new(["嗯"], "嗯。").then_round([], "");
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig.speak();
    rig.wait_phase(Phase::UserSpeaking);
    rig.wait_until("back to listening", |r| {
        r.phases()
            .ends_with(&[Phase::UserSpeaking, Phase::Listening])
    });
    assert!(rig.user_turns().is_empty());
    assert!(rig.dialogue.requests().is_empty(), "no request for a hum");
    assert!(rig.tts.requests().is_empty());
    rig.stop();
}

#[test]
fn the_second_turn_goes_to_the_same_conversation() {
    let (mut cfg, clock) = RigConfig::new("first question", ["Answer one."]);
    cfg.vad = vad_runs(&[
        ONE_UTTERANCE[0],
        ONE_UTTERANCE[1],
        ONE_UTTERANCE[2],
        ONE_UTTERANCE[1],
        ONE_UTTERANCE[2],
    ]);
    cfg.asr = AsrScript::new(["first"], "first question")
        .then_round([("second", 0)], "second question")
        .then_round([], "");
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig.speak();
    rig.wait_phase(Phase::Speaking);
    let_the_output_drain(&mut rig);
    rig.wait_for("first reply heard", |e| *e == AssistantEvent::ReplyDone);
    rig.wait_phase(Phase::Listening);
    rig.feed.push(ONE_UTTERANCE[1].1 + ONE_UTTERANCE[2].1);
    rig.wait_until("the second request", |r| r.dialogue.requests().len() == 2);
    let requests = rig.dialogue.requests();
    assert_eq!(requests[1].messages[0].text, "second question");
    assert_eq!(requests[0].conversation, requests[1].conversation);
    rig.stop();
    assert_eq!(
        rig.dialogue.ended().len(),
        1,
        "the conversation ends once, with the session"
    );
}

#[test]
fn speech_while_thinking_cancels_the_reply_and_merges_the_words_into_one_turn() {
    let (mut cfg, clock) = RigConfig::new("tell me", ["A story!"]);
    // The reply never starts: the dialogue waits for a clock nobody advances.
    cfg.dialogue = Some(
        prxvc_ports::fakes::FakeDialogue::new(clock.clone()).with_default_script(
            prxvc_ports::fakes::DialogueScript::tokens(["A story!"])
                .with_token_delay(super::rig::ms(100)),
        ),
    );
    cfg.vad = vad_runs(&[
        ONE_UTTERANCE[0],
        ONE_UTTERANCE[1],
        ONE_UTTERANCE[2],
        ONE_UTTERANCE[1],
        ONE_UTTERANCE[2],
    ]);
    cfg.asr = AsrScript::new(["tell"], "tell me")
        .then_round([("a long", 0)], "a long one")
        .then_round([], "");
    let mut rig = Rig::build(cfg, clock);
    let log = rig.dialogue.log();
    rig.start().expect("starts");
    rig.speak();
    rig.wait_phase(Phase::Thinking);
    log.wait_for(EventKind::Opened, 1);
    // The user goes on talking: the pending request is dropped and nothing is said.
    rig.feed.push(ONE_UTTERANCE[1].1 + ONE_UTTERANCE[2].1);
    rig.wait_for("the reply is cut", |e| *e == AssistantEvent::ReplyCut);
    rig.wait_until("the merged turn", |r| r.user_turns().len() == 2);
    assert_eq!(rig.user_turns(), ["tell me", "tell me a long one"]);
    assert_eq!(
        log.kinds_for("turn1").last(),
        Some(&EventKind::Cancelled),
        "the first request was cancelled"
    );
    rig.wait_until("the merged request", |r| r.dialogue.requests().len() == 2);
    let requests = rig.dialogue.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].messages[0].text, "tell me a long one");
    assert!(rig.tts.requests().is_empty(), "nothing was ever spoken");
    rig.stop();
}

#[test]
fn stopping_while_thinking_cancels_the_dialogue_request_and_releases_everything() {
    let (mut cfg, clock) = RigConfig::new("hello there", ["Hi!"]);
    cfg.dialogue = Some(
        prxvc_ports::fakes::FakeDialogue::new(clock.clone()).with_default_script(
            prxvc_ports::fakes::DialogueScript::tokens(["Hi!"])
                .with_token_delay(super::rig::ms(100)),
        ),
    );
    let mut rig = Rig::build(cfg, clock);
    let log = rig.dialogue.log();
    rig.start().expect("starts");
    rig.speak();
    rig.wait_phase(Phase::Thinking);
    log.wait_for(EventKind::Opened, 1);
    rig.stop();
    assert_eq!(
        log.kinds_for("turn1").last(),
        Some(&EventKind::Cancelled),
        "the request was abandoned"
    );
    assert!(rig.feed.stopped.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(rig.dialogue.ended().len(), 1);
    assert_eq!(rig.phases().last(), Some(&Phase::Idle));
    assert_eq!(rig.seen.last(), Some(&AssistantEvent::Ended));
}
