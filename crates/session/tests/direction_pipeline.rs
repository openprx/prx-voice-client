//! Deterministic end-to-end tests of one direction with fake providers on a manual clock (pipeline-design 9.2).

mod pipeline_rig;

use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::TraceStage;
use prxvc_ports::fakes::{AsrScript, FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::pipeline::PipelineEvent;
use prxvc_session::{ClauseState, DirectionState, OutputSource};

use pipeline_rig::{OUTPUT_STAGES, Rig, ms, vad_runs};

const OUT: Direction = Direction::Outgoing;

fn clause(seg: u64, index: u16) -> ClauseId {
    ClauseId::new(SegmentId::new(OUT, seg).expect("seq >= 1"), index)
}

fn many(text: &'static str, n: usize) -> Vec<&'static str> {
    vec![text; n]
}

/// One utterance of two clauses: speech, a 352 ms pause, speech, a pause that ends the segment.
fn two_clause_vad() -> prxvc_ports::fakes::VadScript {
    vad_runs(&[(0.0, 10), (0.9, 20), (0.0, 11), (0.9, 20), (0.0, 25)])
}

fn two_clause_asr(clock: &ManualClock) -> prxvc_ports::fakes::FakeStreamingAsr {
    prxvc_ports::fakes::FakeStreamingAsr::new(
        clock.clone(),
        AsrScript::new(many("今天天气很好", 80), "今天天气很好")
            .then_round(
                many("我们去公园", 80)
                    .into_iter()
                    .map(|t| (t, 0))
                    .collect::<Vec<_>>(),
                "我们去公园",
            )
            .then_round([], ""),
    )
}

#[test]
fn one_utterance_two_clauses_are_spoken_in_order() {
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
    let rig = Rig::start_asr(&clock, two_clause_vad(), two_clause_asr(&clock), mt, tts);
    rig.wait_running();
    rig.feed(86);

    let (c0, c1) = (clause(1, 0), clause(1, 1));
    rig.recorder.wait("clause 0 audio", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == c0))
    });
    assert_eq!(rig.recorder.chunks_of(c0), 2);
    assert_eq!(
        rig.recorder.chunks_of(c1),
        0,
        "clause 1 waits until clause 0 has played"
    );
    rig.advance(ms(200));
    rig.recorder.wait("clause 1 audio", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == c1))
    });
    rig.advance(ms(300));
    let snap = rig.wait_snapshot("both played", |s| {
        s.clauses.len() == 2 && s.clauses.iter().all(|c| c.state == ClauseState::Played)
    });

    assert_eq!(rig.recorder.audio_clause_order(), vec![c0, c1]);
    assert_eq!(rig.recorder.chunks_of(c1), 3);
    assert_eq!(snap.clauses[0].source_text, "今天天气很好");
    assert_eq!(snap.clauses[0].translation, "The weather is nice.");
    assert_eq!(snap.clauses[1].translation, "Let's go to the park.");
    assert_eq!(snap.counters.clauses_played, 2);
    assert_eq!(snap.counters.late_dropped, 0);
    assert_eq!(snap.counters.rejected_events, 0);
    assert_eq!(snap.state, DirectionState::Running);
    assert_eq!(
        rig.recorder.outputs(),
        vec![(OutputSource::Original, OutputSource::Translation)]
    );
    let report = rig.stop();
    assert!(report.clean, "{report:?}");
    for c in [c0, c1] {
        let stages: Vec<TraceStage> = rig.trace_stages(c).into_iter().map(|(s, _)| s).collect();
        for needed in OUTPUT_STAGES {
            assert!(
                stages.contains(&needed),
                "{c}: {needed:?} missing in {stages:?}"
            );
        }
    }
}
