//! Deterministic tests of how the recognition stage handles what the recogniser does by itself: it
//! ends a segment at its maximum length (pipeline-design 3.2, 3.3) and its final text differs from
//! text the pipeline already committed (3.1: committed text is never retracted).

mod pipeline_rig;

use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::TraceStage;
use prxvc_ports::fakes::{AsrScript, FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::pipeline::PipelineEvent;
use prxvc_session::{ClauseState, DirectionState};

use pipeline_rig::{Rig, ms, vad_runs};

const OUT: Direction = Direction::Outgoing;

fn clause(seg: u64, index: u16) -> ClauseId {
    ClauseId::new(SegmentId::new(OUT, seg).expect("seq >= 1"), index)
}

fn many(text: &'static str, n: usize) -> Vec<(&'static str, usize)> {
    vec![(text, 0); n]
}

fn wait_audio_end(rig: &Rig, c: ClauseId) {
    rig.wait_events("audio end", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == c))
    });
}

#[test]
fn a_segment_the_recogniser_cuts_is_committed_and_the_speech_goes_on_in_a_new_segment() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(
            "今天天气很好",
            MtScript::tokens(["The ", "weather ", "is ", "nice."]),
        )
        .with_script(
            "我们去公园",
            MtScript::tokens(["Let's ", "go ", "to ", "the ", "park."]),
        );
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_script("The weather is nice.", TtsScript::chunks(2, 2400))
        .with_script("Let's go to the park.", TtsScript::chunks(3, 2400));
    // The recogniser cuts at its 31st feed with the final of the first part; the speech goes on
    // without a pause, so the rest is a new segment that ends at the pause.
    let mut asr = AsrScript::new([], "").ending_at_max_length(30, "今天天气很好");
    asr.partials = many("今天天气", 30)
        .into_iter()
        .map(|(t, n)| (t.to_owned(), n))
        .collect();
    let asr = asr
        .then_round(many("我们去", 40), "我们去公园")
        .then_round([], "");
    let vad = vad_runs(&[(0.0, 10), (0.9, 70), (0.0, 25)]);
    let rig = Rig::start_with_asr(&clock, vad, asr, mt, tts, |_| {});
    rig.wait_running();
    rig.feed(10 + 70 + 25);

    let (c1, c2) = (clause(1, 0), clause(2, 0));
    wait_audio_end(&rig, c1);
    rig.advance(ms(200));
    wait_audio_end(&rig, c2);
    rig.advance(ms(400));
    let snap = rig.wait_snapshot("both clauses played", |s| {
        s.clauses.len() == 2 && s.clauses.iter().all(|c| c.state == ClauseState::Played)
    });

    // Nothing is lost or repeated: one clause per part, in order, with the right text.
    assert_eq!(snap.clauses[0].clause, c1);
    assert_eq!(snap.clauses[0].source_text, "今天天气很好");
    assert_eq!(snap.clauses[1].clause, c2);
    assert_eq!(snap.clauses[1].source_text, "我们去公园");
    assert_eq!(snap.clauses[0].translation, "The weather is nice.");
    assert_eq!(snap.clauses[1].translation, "Let's go to the park.");
    assert_eq!(rig.recorder.audio_clause_order(), vec![c1, c2]);
    assert_eq!(rig.recorder.chunks_of(c1), 2);
    assert_eq!(rig.recorder.chunks_of(c2), 3);
    assert_eq!(snap.counters.forced_cuts, 1);
    assert_eq!(snap.counters.stable_violations, 0);
    assert_eq!(snap.counters.rejected_events, 0);
    assert_eq!(snap.counters.late_dropped, 0);
    assert_eq!(snap.state, DirectionState::Running);
    // The cut is on the trace, against the clause it committed.
    let stages: Vec<TraceStage> = rig.trace_stages(c1).into_iter().map(|(s, _)| s).collect();
    assert!(stages.contains(&TraceStage::SegmentCut), "{stages:?}");
    // Both segments announced their start: the speech did not silently continue in the old one.
    let starts = rig
        .trace
        .events()
        .iter()
        .filter(|e| e.stage == TraceStage::SpeechStart)
        .count();
    assert_eq!(starts, 2);
    assert!(rig.stop().clean);
}

#[test]
fn a_cut_after_a_clause_was_committed_commits_only_the_rest_of_the_cut_part() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(
            "alpha bravo charlie delta",
            MtScript::tokens(["Alpha ", "bravo."]),
        )
        .with_script("echo foxtrot", MtScript::tokens(["Echo ", "fox."]))
        .with_script("golf hotel", MtScript::tokens(["Golf ", "hotel."]));
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_script("Alpha bravo.", TtsScript::chunks(1, 2400))
        .with_script("Echo fox.", TtsScript::chunks(1, 2400))
        .with_script("Golf hotel.", TtsScript::chunks(1, 2400));
    // 25 feeds say "alpha bravo" (nothing stable), then the first part grows with a stable
    // prefix of four words, which the pipeline commits at the length limit; the recogniser then
    // cuts with a final that holds those four words and two more.
    let mut asr =
        AsrScript::new([], "").ending_at_max_length(70, "alpha bravo charlie delta echo foxtrot");
    asr.partials = std::iter::repeat_n(("alpha bravo".to_owned(), 0), 25)
        .chain(std::iter::repeat_n(
            ("alpha bravo charlie delta echo fox".to_owned(), 25),
            45,
        ))
        .collect();
    let asr = asr
        .then_round(many("golf", 10), "golf hotel")
        .then_round([], "");
    let vad = vad_runs(&[(0.0, 10), (0.9, 100), (0.0, 25)]);
    let rig = Rig::start_with_asr(&clock, vad, asr, mt, tts, |p| {
        p.commit.max_clause = ms(1200);
    });
    rig.wait_running();
    rig.feed(10 + 100 + 25);
    let snap = rig.wait_snapshot("three clauses", |s| s.clauses.len() == 3);
    let texts: Vec<&str> = snap
        .clauses
        .iter()
        .map(|c| c.source_text.as_str())
        .collect();
    assert_eq!(
        texts,
        ["alpha bravo charlie delta", "echo foxtrot", "golf hotel"],
        "the committed clause is not committed again, the cut commits only what follows it"
    );
    assert_eq!(
        snap.clauses.iter().map(|c| c.clause).collect::<Vec<_>>(),
        vec![clause(1, 0), clause(1, 1), clause(2, 0)]
    );
    assert_eq!(snap.counters.forced_cuts, 1);
    assert!(rig.stop().clean);
}
