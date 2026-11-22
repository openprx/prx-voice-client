//! Deterministic tests of the filler rule and the speakable-text rule: a unit that is only filler is neither translated nor spoken, and a translation with nothing to say never reaches the synthesiser (call findings of 2026-11-21).

mod pipeline_rig;

use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::TraceStage;
use prxvc_ports::fakes::{AsrScript, FakeSpeechSynth, FakeStreamingAsr, FakeTranslator, MtScript, TtsScript};
use prxvc_session::{ClauseState, Stage};

use pipeline_rig::{Rig, asr_script, utterances, vad_runs};

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

/// One segment of three speech runs, a clause pause (352 ms) between them, then the segment end.
fn three_runs() -> prxvc_ports::fakes::VadScript {
    vad_runs(&[(0.0, 10), (0.9, 20), (0.0, 11), (0.9, 20), (0.0, 11), (0.9, 20), (0.0, 25)])
}

fn rounds(clock: &ManualClock, texts: &[&'static str]) -> FakeStreamingAsr {
    let partials = |t: &'static str| vec![(t, 0); 80];
    let mut script = AsrScript::new(partials(texts[0]).into_iter().map(|(t, _)| t), texts[0]);
    for t in &texts[1..] {
        script = script.then_round(partials(t), t);
    }
    FakeStreamingAsr::new(clock.clone(), script.then_round([], ""))
}

#[test]
fn a_filler_in_the_middle_of_a_segment_joins_the_next_unit_and_is_not_lost() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let mt = mt.with_script("\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002} \u{6211}\u{4eec}\u{53bb}\u{516c}\u{56ed}", MtScript::tokens(["Um, let's go to the park."]));
    let rig = Rig::start_asr(
        &clock,
        vad_runs(&[(0.0, 10), (0.9, 20), (0.0, 11), (0.9, 20), (0.0, 25)]),
        rounds(&clock, &["\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002}", "\u{6211}\u{4eec}\u{53bb}\u{516c}\u{56ed}"]),
        mt,
        tts,
    );
    rig.wait_running();
    rig.feed(86);
    let snap = rig.wait_snapshot("the merged unit is translated", |s| {
        s.clauses.first().is_some_and(|c| !c.translation.is_empty())
    });
    assert_eq!(snap.counters.filler_dropped, 0, "nothing was dropped");
    assert_eq!(snap.clauses.len(), 1, "one unit: the filler joined the speech after it");
    assert_eq!(snap.clauses[0].source_text, "\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002} \u{6211}\u{4eec}\u{53bb}\u{516c}\u{56ed}");
    assert!(rig.stop().clean);
}

#[test]
fn a_filler_that_ends_the_segment_is_dropped_once() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let mt = mt.with_script("\u{4eca}\u{5929}\u{5929}\u{6c14}\u{5f88}\u{597d}", MtScript::tokens(["The weather is nice."]));
    let rig = Rig::start_asr(
        &clock,
        vad_runs(&[(0.0, 10), (0.9, 20), (0.0, 11), (0.9, 20), (0.0, 25)]),
        rounds(&clock, &["\u{4eca}\u{5929}\u{5929}\u{6c14}\u{5f88}\u{597d}", "\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002}"]),
        mt,
        tts,
    );
    rig.wait_running();
    rig.feed(86);
    let snap = rig.wait_snapshot("the trailing filler is dropped", |s| s.counters.filler_dropped >= 1);
    assert_eq!(snap.counters.filler_dropped, 1);
    assert_eq!(snap.clauses.len(), 1, "the speech before it is a unit of its own");
    assert!(rig.stop().clean);
}

#[test]
fn a_second_filler_while_one_waits_is_dropped_and_counted_once() {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let mt = mt.with_script("\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002} \u{6211}\u{4eec}\u{53bb}\u{516c}\u{56ed}", MtScript::tokens(["Um, let's go to the park."]));
    let rig = Rig::start_asr(
        &clock,
        three_runs(),
        rounds(&clock, &["\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002}", "\u{554a}\u{554a}\u{554a}\u{554a}\u{3002}", "\u{6211}\u{4eec}\u{53bb}\u{516c}\u{56ed}"]),
        mt,
        tts,
    );
    rig.wait_running();
    rig.feed(130);
    let snap = rig.wait_snapshot("the speech is translated", |s| {
        s.clauses.first().is_some_and(|c| !c.translation.is_empty())
    });
    assert_eq!(snap.counters.filler_dropped, 1, "the second hum only");
    assert_eq!(snap.clauses[0].source_text, "\u{55ef}\u{55ef}\u{55ef}\u{55ef}\u{3002} \u{6211}\u{4eec}\u{53bb}\u{516c}\u{56ed}");
    assert!(rig.stop().clean);
}

const IN: Direction = Direction::Incoming;
const SENTENCE: &str = "The weather is nice today,";

fn en_script(texts: &[&'static str]) -> AsrScript {
    let partials = |t: &'static str| vec![(t, 0); 80];
    let mut script = AsrScript::new(partials(texts[0]).into_iter().map(|(t, _)| t), texts[0]);
    for t in &texts[1..] {
        script = script.then_round(partials(t), t);
    }
    script.then_round([], "")
}

/// One English speech run whose recogniser text first holds `first`, with that part stable, then
/// `first` and `last` together: the first part is committed from the stable prefix and `last` is
/// what the segment end commits. Returns the final snapshot.
fn english_tail(first: &str, last: &str, drop_filler: bool) -> std::sync::Arc<prxvc_session::pipeline::DirectionSnapshot> {
    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let both = format!("{first} {last}");
    let mut asr = AsrScript::new([], &both);
    asr.partials = (0..60)
        .map(|i| if i < 20 { (first.to_owned(), first.len()) } else { (both.clone(), first.len()) })
        .collect();
    let rig = Rig::start_with(
        &clock,
        IN,
        vad_runs(&[(0.0, 10), (0.9, 40), (0.0, 25)]),
        asr.then_round([], ""),
        mt,
        tts,
        |p| {
            p.drop_filler = drop_filler;
            p.commit.clause_pause = std::time::Duration::from_secs(30);
        },
    );
    rig.wait_running();
    rig.feed(80);
    let snap = rig.wait_snapshot("the segment ended", |s| {
        s.counters.filler_dropped >= 1 || s.clauses.len() >= 2
    });
    assert!(rig.stop().clean);
    snap
}

#[test]
fn a_stray_english_fragment_ending_a_segment_is_dropped_like_a_filler() {
    for tail in ["I.", "I'm.", "I'll.", "I've.", "But", "So.", "Still.", "Well,"] {
        let snap = english_tail(SENTENCE, tail, true);
        assert_eq!(snap.counters.filler_dropped, 1, "{tail:?} {:?}", snap.clauses.iter().map(|c| c.source_text.clone()).collect::<Vec<_>>());
        assert_eq!(snap.clauses.len(), 1, "{tail:?}: only the sentence is a unit");
    }
}

#[test]
fn the_same_fragment_is_kept_with_the_filter_off() {
    let snap = english_tail(SENTENCE, "Still.", false);
    assert_eq!(snap.counters.filler_dropped, 0);
    assert_eq!(snap.clauses.len(), 2);
    assert_eq!(snap.clauses[1].source_text, "Still.");
}

#[test]
fn a_one_word_utterance_that_is_the_only_unit_is_kept() {
    for word in ["Still.", "I'm."] {
        let clock = ManualClock::new();
        let (mt, tts) = providers(&clock);
        let rig = Rig::start_with(
            &clock,
            IN,
            utterances(1, 0),
            en_script(&[word]),
            mt,
            tts,
            |_| {},
        );
        rig.wait_running();
        rig.feed(55);
        let snap = rig.wait_snapshot("the word is a unit", |s| !s.clauses.is_empty());
        assert_eq!(snap.counters.filler_dropped, 0, "{word:?}");
        assert_eq!(snap.clauses[0].source_text, word);
        assert!(rig.stop().clean);
    }
}

#[test]
fn a_fragment_in_the_middle_of_a_segment_is_kept() {
    let snap = english_tail(SENTENCE, "Still. We should go home now", true);
    assert_eq!(snap.counters.filler_dropped, 0);
    assert_eq!(snap.clauses.len(), 2);
    assert_eq!(snap.clauses[1].source_text, "Still. We should go home now");
}
