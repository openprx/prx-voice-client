//! Deterministic tests of the language rules: speech already in the listener's language is not translated and its original voice passes through; a translation that starts in the wrong language is asked for once more with the strict instruction and then fails visibly; the quality flags of a finished translation are counted and shown.

mod pipeline_rig;

use prxvc_media::{Direction, ManualClock};
use prxvc_ports::MtFlag;
use prxvc_ports::fakes::{FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::pipeline::{PipelineEvent, SkipReason};
use prxvc_session::{ClauseState, Stage};

use pipeline_rig::{Rig, asr_script, ms, utterances};

const IN: Direction = Direction::Incoming;
const OUT: Direction = Direction::Outgoing;

fn tts(clock: &ManualClock) -> FakeSpeechSynth {
    FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400))
}

fn mt(clock: &ManualClock) -> FakeTranslator {
    FakeTranslator::new(clock.clone()).with_script("Hello.", MtScript::tokens(["Hello."]))
}

fn passthroughs(rig: &Rig) -> Vec<bool> {
    rig.recorder
        .events()
        .into_iter()
        .filter_map(|e| match e {
            PipelineEvent::Passthrough { on } => Some(on),
            _ => None,
        })
        .collect()
}

#[test]
fn chinese_heard_by_a_chinese_listener_is_not_translated_and_passes_through() {
    let clock = ManualClock::new();
    let rig = Rig::start_with(
        &clock,
        IN,
        utterances(1, 0),
        asr_script(&["今天天气很好我们出去走走"]),
        mt(&clock),
        tts(&clock),
        |_| {},
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("passed through", |s| s.counters.in_target_passed >= 1);
    assert_eq!(snap.clauses[0].skipped, Some(SkipReason::SourceInTarget));
    assert_eq!(snap.counters.skipped, 0, "not a loss");
    assert!(rig.fakes.mt.requests().iter().all(|r| r.text == "Hello."));
    assert!(rig.fakes.tts.requests().is_empty());
    rig.recorder.wait("passthrough ended", |e| {
        e.iter()
            .filter(|e| matches!(e, PipelineEvent::Passthrough { on: false }))
            .count()
            == 1
    });
    assert_eq!(passthroughs(&rig), [true, false]);
    assert!(rig.stop().clean);
}

#[test]
fn english_heard_by_an_english_listener_is_the_same_in_the_outgoing_direction() {
    let clock = ManualClock::new();
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&["See you at the airport at three"]),
        mt(&clock),
        tts(&clock),
        |_| {},
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("passed through", |s| s.counters.in_target_passed >= 1);
    assert_eq!(snap.counters.clauses_failed, 0);
    assert!(rig.fakes.tts.requests().is_empty());
    assert!(rig.stop().clean);
}

#[test]
fn with_the_rule_off_the_clause_is_translated_as_before() {
    let clock = ManualClock::new();
    let t = mt(&clock).with_script(
        "今天天气很好我们出去走走",
        MtScript::tokens(["今天天气很好。"]),
    );
    let rig = Rig::start_with(
        &clock,
        IN,
        utterances(1, 0),
        asr_script(&["今天天气很好我们出去走走"]),
        t,
        tts(&clock),
        |p| p.skip_source_in_target = false,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("translated", |s| {
        s.clauses.first().is_some_and(|c| !c.translation.is_empty())
    });
    assert_eq!(snap.counters.in_target_passed, 0);
    assert!(passthroughs(&rig).is_empty());
    assert!(rig.stop().clean);
}

const SAID: &str = "我们三点在机场见面";

#[test]
fn a_reply_in_the_wrong_language_is_asked_for_again_before_anything_is_spoken() {
    let clock = ManualClock::new();
    let t = mt(&clock)
        .with_script(SAID, MtScript::tokens(["我们三点", "在机场见面。"]))
        .with_strict_script(SAID, MtScript::tokens(["We meet ", "at three."]));
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[SAID]),
        t,
        tts(&clock),
        |p| p.retry_wrong_language = true,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("retried and translated", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.translation == "We meet at three.")
    });
    assert_eq!(snap.counters.mt_retries, 1);
    assert!(snap.clauses[0].retried);
    let strictness: Vec<bool> = rig
        .fakes
        .mt
        .requests()
        .iter()
        .filter(|r| r.text == SAID)
        .map(|r| r.strict_language)
        .collect();
    assert_eq!(strictness, [false, true]);
    assert!(
        rig.fakes
            .tts
            .requests()
            .iter()
            .all(|r| !r.text.contains('我')),
        "nothing of the wrong-language reply was spoken"
    );
    assert!(rig.stop().clean);
}

#[test]
fn a_second_wrong_reply_fails_the_clause_visibly() {
    let clock = ManualClock::new();
    let t = mt(&clock).with_script(SAID, MtScript::tokens(["我们三点", "在机场见面。"]));
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[SAID]),
        t,
        tts(&clock),
        |p| p.retry_wrong_language = true,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("failed", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.state == ClauseState::Failed(Stage::Mt))
    });
    assert_eq!(snap.counters.mt_retries, 1);
    assert_eq!(snap.counters.mt_suspect, 1);
    assert!(rig.fakes.tts.requests().is_empty());
    assert!(rig.stop().clean);
}

#[test]
fn the_flags_of_a_finished_translation_are_counted_and_shown() {
    let clock = ManualClock::new();
    let t = mt(&clock).with_script(
        SAID,
        MtScript::tokens(["We meet at three."]).with_flags([MtFlag::Refusal, MtFlag::TooLong]),
    );
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[SAID]),
        t,
        tts(&clock),
        |_| {},
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("flagged", |s| {
        s.counters.flags_total() >= 2 && s.clauses.first().is_some_and(|c| c.flags.len() == 2)
    });
    assert_eq!(
        (snap.counters.flag_refusal, snap.counters.flag_too_long),
        (1, 1)
    );
    assert_eq!(snap.clauses[0].flags, [MtFlag::Refusal, MtFlag::TooLong]);
    assert_eq!(snap.counters.mt_suspect, 0, "reported, not a failure");
    assert!(rig.stop().clean);
}

#[test]
fn a_finished_utterance_enters_the_ledger() {
    let clock = ManualClock::new();
    let t = mt(&clock).with_script(SAID, MtScript::tokens(["We meet at three."]));
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[SAID]),
        t,
        tts(&clock),
        |_| {},
    );
    rig.wait_running();
    rig.feed(55);
    rig.recorder.wait("audio end", |e| {
        e.iter().any(|x| matches!(x, PipelineEvent::AudioEnd(_)))
    });
    rig.advance(ms(300));
    let snap = rig.wait_snapshot("utterance finished", |s| s.ledger.utterances >= 1);
    assert_eq!(snap.ledger.utterances, 1);
    assert!(rig.stop().clean);
}

#[test]
fn a_reply_that_answers_instead_of_translating_is_asked_for_again_before_it_is_spoken() {
    let clock = ManualClock::new();
    let t = mt(&clock)
        .with_script(SAID, MtScript::tokens(["Sure, ", "see you at three."]))
        .with_strict_script(SAID, MtScript::tokens(["We meet ", "at three."]));
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[SAID]),
        t,
        tts(&clock),
        |p| p.retry_wrong_language = true,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("retried and translated", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.translation == "We meet at three.")
    });
    assert_eq!(snap.counters.mt_retries, 1);
    assert!(snap.clauses[0].retried);
    let strictness: Vec<bool> = rig
        .fakes
        .mt
        .requests()
        .iter()
        .filter(|r| r.text == SAID)
        .map(|r| r.strict_language)
        .collect();
    assert_eq!(strictness, [false, true]);
    assert!(
        rig.fakes
            .tts
            .requests()
            .iter()
            .all(|r| !r.text.contains("Sure")),
        "nothing of the answer was spoken"
    );
    assert!(rig.stop().clean);
}

#[test]
fn an_acknowledgement_the_speaker_really_said_is_not_asked_for_again() {
    let clock = ManualClock::new();
    let said = "好的，我们三点在机场见面";
    let t = mt(&clock).with_script(said, MtScript::tokens(["Sure, ", "we meet at three."]));
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[said]),
        t,
        tts(&clock),
        |p| p.retry_wrong_language = true,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("translated", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.translation == "Sure, we meet at three.")
    });
    assert_eq!(snap.counters.mt_retries, 0);
    assert!(rig.stop().clean);
}

#[test]
fn a_second_reply_that_still_reads_like_an_answer_is_spoken_and_not_asked_for_a_third_time() {
    let clock = ManualClock::new();
    let t = mt(&clock).with_script(SAID, MtScript::tokens(["Sure, ", "see you at three."]));
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(1, 0),
        asr_script(&[SAID]),
        t,
        tts(&clock),
        |p| p.retry_wrong_language = true,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("spoken after one retry", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.translation == "Sure, see you at three.")
    });
    assert_eq!(snap.counters.mt_retries, 1);
    assert_eq!(
        rig.fakes
            .mt
            .requests()
            .iter()
            .filter(|r| r.text == SAID)
            .count(),
        2,
        "one retry, no more"
    );
    assert!(rig.stop().clean);
}

#[test]
fn our_own_translation_heard_by_the_microphone_is_dropped_and_never_passed_through() {
    // Outgoing: the speaker says something in Chinese, the other party hears the English; then the
    // microphone hears that English again (speakers, the other phone in the room).
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script("Hello.", MtScript::tokens(["Hello."]))
        .with_script(
            "今天天气很好",
            MtScript::tokens(["The weather ", "is nice ", "today."]),
        );
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(2, 30),
        asr_script(&["今天天气很好", "the weather is nice today"]),
        mt,
        tts(&clock),
        |_| {},
    );
    rig.wait_running();
    // The guard compares with the translations the core already holds. Feed the first utterance and
    // its gap, and let its translation arrive before the microphone hears it again: without this the
    // recogniser thread races the translation thread and the echo can be judged first.
    rig.feed(58);
    rig.wait_snapshot("the first translation is held", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.translation == "The weather is nice today.")
    });
    rig.feed(52);
    let snap = rig.wait_snapshot("the echo is dropped", |s| s.counters.self_echo_dropped >= 1);
    assert_eq!(snap.counters.self_echo_dropped, 1);
    assert_eq!(
        snap.counters.in_target_passed, 0,
        "never passed through as the original"
    );
    assert_eq!(snap.counters.skipped, 0, "not a loss");
    assert!(
        snap.clauses
            .iter()
            .any(|c| c.skipped == Some(SkipReason::SelfEcho)),
        "visible in the captions"
    );
    assert!(
        rig.fakes
            .mt
            .requests()
            .iter()
            .all(|r| r.text == "Hello." || r.text == "今天天气很好"),
        "the echo is never translated"
    );
    assert!(
        passthroughs(&rig).iter().all(|on| !on),
        "{:?}",
        passthroughs(&rig)
    );
    assert!(rig.stop().clean);
}

#[test]
fn english_the_speaker_really_said_is_still_passed_through() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script("Hello.", MtScript::tokens(["Hello."]))
        .with_script(
            "今天天气很好",
            MtScript::tokens(["The weather ", "is nice ", "today."]),
        );
    let rig = Rig::start_with(
        &clock,
        OUT,
        utterances(2, 30),
        asr_script(&["今天天气很好", "see you at the airport at three"]),
        mt,
        tts(&clock),
        |_| {},
    );
    rig.wait_running();
    rig.feed(110);
    let snap = rig.wait_snapshot("passed through", |s| s.counters.in_target_passed >= 1);
    assert_eq!(snap.counters.self_echo_dropped, 0);
    assert!(rig.stop().clean);
}
