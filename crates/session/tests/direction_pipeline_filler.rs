//! Deterministic tests of the filler rule and the speakable-text rule: a unit that is only filler is neither translated nor spoken, and a translation with nothing to say never reaches the synthesiser (call findings of 2026-11-21).

mod pipeline_rig;

use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::TraceStage;
use prxvc_ports::fakes::{FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::{ClauseState, Stage};

use pipeline_rig::{Rig, asr_script, utterances};

const OUT: Direction = Direction::Outgoing;

fn clause(seg: u64, index: u16) -> ClauseId {
    ClauseId::new(SegmentId::new(OUT, seg).expect("seq >= 1"), index)
}

fn providers(clock: &ManualClock) -> (FakeTranslator, FakeSpeechSynth) {
    (
        FakeTranslator::new(clock.clone())
            .with_script("Hello.", MtScript::tokens(["Hello."]))
            .with_default_script(MtScript::tokens(["Mm."])),
        FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400)),
    )
}

#[test]
fn an_utterance_recognised_as_a_hum_is_counted_and_never_translated_or_spoken() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let rig = Rig::start(&clock, utterances(1, 0), asr_script(&["嗯。"]), mt, tts);
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("filler dropped", |s| s.counters.filler_dropped >= 1);
    assert!(snap.clauses.is_empty(), "no clause exists for a filler");
    assert_eq!(snap.counters.clauses_committed, 0);
    assert_eq!(snap.counters.filler_dropped, 1);
    assert!(
        rig.fakes.mt.requests().iter().all(|r| r.text == "Hello."),
        "nothing but the health probe may reach the translator"
    );
    assert!(rig.fakes.tts.requests().is_empty());
    let stages: Vec<TraceStage> = rig
        .trace_stages(clause(1, 0))
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    assert!(stages.contains(&TraceStage::FillerDropped), "{stages:?}");
    assert!(!stages.contains(&TraceStage::ClauseCommitted), "{stages:?}");
    assert!(rig.stop().clean);
}

#[test]
fn with_the_filter_off_the_same_utterance_is_translated_and_spoken() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let rig = Rig::start_with_asr(
        &clock,
        utterances(1, 0),
        asr_script(&["嗯。"]),
        mt,
        tts,
        |p| p.drop_filler = false,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("clause translated", |s| {
        s.clauses.first().is_some_and(|c| c.translation == "Mm.")
    });
    assert_eq!(snap.counters.filler_dropped, 0);
    assert_eq!(snap.clauses[0].source_text, "嗯。");
    assert!(rig.stop().clean);
}

#[test]
fn a_short_real_answer_is_kept() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let mt = mt.with_script("好", MtScript::tokens(["Okay."]));
    let rig = Rig::start(&clock, utterances(1, 0), asr_script(&["好"]), mt, tts);
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("answer translated", |s| {
        s.clauses.first().is_some_and(|c| c.translation == "Okay.")
    });
    assert_eq!(snap.counters.filler_dropped, 0);
    assert!(rig.stop().clean);
}

#[test]
fn a_translation_with_nothing_to_say_is_a_visible_failure_and_never_reaches_the_synthesiser() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let mt = mt.with_script("今天天气很好", MtScript::tokens(["...", " "]));
    let rig = Rig::start(
        &clock,
        utterances(1, 0),
        asr_script(&["今天天气很好"]),
        mt,
        tts,
    );
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("clause failed", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.state == ClauseState::Failed(Stage::Mt))
    });
    assert_eq!(snap.counters.mt_suspect, 1);
    assert!(
        rig.fakes.tts.requests().is_empty(),
        "punctuation is never sent to the synthesiser"
    );
    assert!(rig.stop().clean);
}
