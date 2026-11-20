//! Deterministic end-to-end tests of one direction with fake providers on a manual clock (pipeline-design 9.2).

mod pipeline_rig;

use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::TraceStage;
use prxvc_ports::fakes::{AsrScript, FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::pipeline::PipelineEvent;
use prxvc_session::{ClauseState, DirectionState, OutputSource};

use pipeline_rig::{OUTPUT_STAGES, Rig, asr_script, ms, simple_providers, utterances, vad_runs};

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

fn wait_audio_end(rig: &Rig, c: ClauseId) {
    rig.wait_events("audio end", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == c))
    });
}

#[test]
fn a_stable_prefix_commits_a_clause_before_the_segment_ends_and_a_later_rewrite_does_not_disturb_it()
 {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(
        &clock,
        &[
            ("alpha bravo charlie delta", &["Alpha ", "bravo."]),
            ("echo fox trot golf hotel", &["Echo ", "golf."]),
        ],
        1,
    );
    let partials: Vec<(&'static str, usize)> = std::iter::repeat_n(("alpha bravo", 0), 10)
        .chain(std::iter::repeat_n(
            ("alpha bravo charlie delta echo foxtrot", 25),
            30,
        ))
        .chain(std::iter::repeat_n(
            ("alpha bravo charlie delta echo fox trot golf", 25),
            40,
        ))
        .collect();
    let mut asr =
        AsrScript::new([], "alpha bravo charlie delta echo fox trot golf hotel").then_round([], "");
    asr.partials = partials
        .into_iter()
        .map(|(t, n)| (t.to_owned(), n))
        .collect();
    let vad = vad_runs(&[(0.0, 10), (0.9, 60), (0.0, 25)]);
    let rig = Rig::start_with_asr(&clock, vad, asr, mt, tts, |p| {
        p.commit.max_clause = ms(1200);
    });
    rig.wait_running();
    rig.feed(10 + 45);
    let early = rig.wait_snapshot("a clause before the segment ends", |s| {
        !s.clauses.is_empty()
    });
    assert_eq!(early.clauses[0].source_text, "alpha bravo charlie delta");
    assert!(
        early.partial.is_some() || early.clauses.len() == 1,
        "the segment is still open"
    );
    assert_eq!(
        rig.latest().clauses.len(),
        1,
        "no pause has been fed yet, so only the stable prefix can have committed"
    );
    rig.feed(15 + 25);
    let snap = rig.wait_snapshot("the rest committed", |s| s.clauses.len() == 2);
    assert_eq!(snap.clauses[0].source_text, "alpha bravo charlie delta");
    assert_eq!(snap.clauses[1].source_text, "echo fox trot golf hotel");
    assert_eq!(snap.counters.stable_violations, 0);
    assert!(rig.stop().clean);
}

#[test]
fn a_recognizer_with_finals_only_still_commits_at_the_pause() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[("今天天气很好", &["Nice ", "weather."])], 1);
    let asr = AsrScript::new([], "今天天气很好").then_round([], "");
    let rig = Rig::start(&clock, utterances(1, 0), asr, mt, tts);
    rig.wait_running();
    rig.feed(55);
    let c0 = clause(1, 0);
    wait_audio_end(&rig, c0);
    rig.advance(ms(100));
    let snap = rig.wait_snapshot("played", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.state == ClauseState::Played)
    });
    assert_eq!(snap.clauses[0].translation, "Nice weather.");
    assert!(
        snap.partial.is_none(),
        "a recognizer with finals only never shows a partial"
    );
    assert!(rig.stop().clean);
}

#[test]
fn translation_tokens_are_split_into_clauses_without_cutting_numbers_or_abbreviations() {
    let clock = ManualClock::new();
    let tokens: &'static [&'static str] = &[
        "The ", "total ", "is ", "3", ".", "5 ", "dollars", ". ", "Dr", ".", " Smith ", "said ",
        "so.",
    ];
    let (mt, _) = simple_providers(&clock, &[("今天天气很好", tokens)], 1);
    let tts = FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400));
    let asr = AsrScript::new([], "今天天气很好").then_round([], "");
    let rig = Rig::start_with(&clock, OUT, utterances(1, 0), asr, mt, tts, |p| {
        p.commit.split_target_clauses = true;
        p.commit.split_min_tokens_after = 1;
    });
    rig.wait_running();
    rig.feed(55);
    let c0 = clause(1, 0);
    wait_audio_end(&rig, c0);
    let spoken: Vec<String> = rig
        .fakes
        .tts
        .requests()
        .into_iter()
        .map(|r| r.text)
        .collect();
    assert_eq!(
        spoken,
        vec!["The total is 3.5 dollars.", "Dr. Smith said so."]
    );
    assert_eq!(
        rig.recorder.chunks_of(c0),
        2,
        "both pieces are spoken as one clause, in order"
    );
    assert_eq!(rig.fakes.tts.requests().len(), 2);
    rig.advance(ms(300));
    let snap = rig.wait_snapshot("played", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.state == ClauseState::Played)
    });
    assert_eq!(
        snap.counters.rejected_events, 0,
        "early synthesis is legal in the clause table"
    );
    assert!(rig.stop().clean);
}

fn is_skipped(e: &PipelineEvent) -> bool {
    matches!(e, PipelineEvent::ClauseSkipped { .. })
}

#[test]
fn a_new_utterance_does_not_cut_the_playing_translation_and_drops_only_a_stale_queued_clause() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(
        &clock,
        &[
            ("今天天气很好", &["First ", "one."]),
            ("我们去公园", &["Second ", "one."]),
            ("你好吗朋友", &["Third ", "one."]),
        ],
        3,
    );
    let asr = asr_script(&["今天天气很好", "我们去公园", "你好吗朋友"]);
    let vad = vad_runs(&[
        (0.0, 10),
        (0.9, 20),
        (0.0, 120),
        (0.9, 20),
        (0.0, 120),
        (0.9, 20),
        (0.0, 25),
    ]);
    let rig = Rig::start_with_asr(&clock, vad, asr, mt, tts, |_| {});
    rig.wait_running();
    let (a, b, c) = (clause(1, 0), clause(2, 0), clause(3, 0));
    rig.feed(150);
    rig.recorder.wait("A is playing", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::Audio(a2) if a2.payload.clause == a))
    });
    rig.feed(140);
    rig.wait_snapshot("B is queued behind A", |s| {
        s.clauses
            .iter()
            .any(|x| x.clause == b && x.state == ClauseState::Synthesizing)
    });
    rig.feed(20);
    rig.recorder
        .wait("B skipped as stale", |e| e.iter().any(is_skipped));
    let snap = rig.wait_snapshot("B skipped", |s| {
        s.clauses
            .iter()
            .any(|x| x.clause == b && x.state == ClauseState::Skipped)
    });
    assert_eq!(snap.counters.stale_skips, 1);
    assert_eq!(
        snap.clauses[0].state,
        ClauseState::Playing,
        "the playing clause is never cut"
    );
    assert!(rig.recorder.events().iter().all(|e| !matches!(
        e,
        PipelineEvent::Flush(
            prxvc_session::CancelAction::CancelSegment(_)
                | prxvc_session::CancelAction::CancelDirection(_)
        )
    )));
    rig.feed(25);
    rig.advance(ms(300));
    wait_audio_end(&rig, c);
    rig.advance(ms(300));
    rig.wait_snapshot("A and C played", |s| s.counters.clauses_played == 2);
    assert_eq!(rig.recorder.audio_clause_order(), vec![a, c]);
    assert_eq!(rig.recorder.chunks_of(b), 0);
    assert!(rig.stop().clean);
}

#[test]
fn cancelling_a_segment_mid_translation_cancels_the_request_and_the_next_segment_proceeds() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(
            "今天天气很好",
            MtScript::tokens(["Slow ", "one."]).with_token_delay(ms(100)),
        )
        .with_script("我们去公园", MtScript::tokens(["Next ", "one."]));
    let tts = FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400));
    let asr = asr_script(&["今天天气很好", "我们去公园"]);
    let rig = Rig::start_with_asr(&clock, utterances(2, 30), asr, mt, tts, |_| {});
    rig.wait_running();
    rig.feed(55);
    pipeline_rig::wait_log(
        &rig.fakes.mt.log(),
        prxvc_ports::fakes::EventKind::Opened,
        1,
    );
    rig.pipeline
        .cancel_segment(SegmentId::new(OUT, 1).expect("seq"));
    pipeline_rig::wait_log(
        &rig.fakes.mt.log(),
        prxvc_ports::fakes::EventKind::Cancelled,
        1,
    );
    rig.wait_snapshot("cancelled", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.state == ClauseState::Cancelled)
    });
    assert_eq!(
        rig.recorder.count(|e| matches!(e, PipelineEvent::Audio(_))),
        0,
        "nothing of it reaches the output"
    );
    rig.feed(55);
    let c1 = clause(2, 0);
    wait_audio_end(&rig, c1);
    assert_eq!(rig.recorder.audio_clause_order(), vec![c1]);
    assert!(rig.stop().clean);
    pipeline_rig::assert_every_request_ended(&rig.fakes.mt.log());
}

#[test]
fn cancelling_a_segment_mid_synthesis_cancels_the_request_and_the_next_segment_proceeds() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script("今天天气很好", MtScript::tokens(["Slow ", "one."]))
        .with_script("我们去公园", MtScript::tokens(["Next ", "one."]));
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_script(
            "Slow one.",
            TtsScript::chunks(3, 2400).with_chunk_delay(ms(100)),
        )
        .with_script("Next one.", TtsScript::chunks(1, 2400));
    let asr = asr_script(&["今天天气很好", "我们去公园"]);
    let rig = Rig::start_with_asr(&clock, utterances(2, 30), asr, mt, tts, |_| {});
    rig.wait_running();
    rig.feed(55);
    pipeline_rig::wait_log(
        &rig.fakes.tts.log(),
        prxvc_ports::fakes::EventKind::Opened,
        1,
    );
    rig.pipeline
        .cancel_segment(SegmentId::new(OUT, 1).expect("seq"));
    pipeline_rig::wait_log(
        &rig.fakes.tts.log(),
        prxvc_ports::fakes::EventKind::Cancelled,
        1,
    );
    assert_eq!(
        rig.recorder.count(|e| matches!(e, PipelineEvent::Audio(_))),
        0
    );
    rig.feed(55);
    wait_audio_end(&rig, clause(2, 0));
    assert_eq!(rig.recorder.audio_clause_order(), vec![clause(2, 0)]);
    assert!(rig.stop().clean);
    pipeline_rig::assert_every_request_ended(&rig.fakes.tts.log());
}

#[test]
fn pause_and_resume_announce_the_passthrough_and_stop_new_segments_while_paused() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(
        &clock,
        &[
            ("今天天气很好", &["Nice ", "day."]),
            ("我们去公园", &["Go ", "on."]),
        ],
        1,
    );
    let asr = asr_script(&["今天天气很好", "我们去公园"]);
    let rig = Rig::start_with_asr(&clock, utterances(2, 30), asr, mt, tts, |_| {});
    rig.wait_running();
    rig.pipeline.pause();
    rig.wait_snapshot("paused", |s| {
        matches!(s.state, DirectionState::Paused { .. })
    });
    assert_eq!(rig.latest().output, OutputSource::Original);
    rig.feed(55);
    assert!(
        rig.latest().clauses.is_empty(),
        "no segment starts while paused"
    );
    rig.pipeline.resume();
    rig.wait_running();
    assert_eq!(rig.latest().output, OutputSource::Translation);
    rig.feed(55);
    wait_audio_end(&rig, clause(1, 0));
    assert_eq!(
        rig.recorder.outputs(),
        vec![
            (OutputSource::Original, OutputSource::Translation),
            (OutputSource::Translation, OutputSource::Original),
            (OutputSource::Original, OutputSource::Translation),
        ]
    );
    assert!(rig.stop().clean);
}

/// Stop with a request in flight at each stage: every request ends and no thread is left.
fn stop_in(stage: &str) {
    use prxvc_ports::fakes::EventKind as K;
    let clock = ManualClock::new();
    let mut mt_script = MtScript::tokens(["Nice ", "day."]);
    let mut tts_script = TtsScript::chunks(2, 2400);
    let mut asr = AsrScript::new([], "今天天气很好").then_round([], "");
    match stage {
        "asr" => asr = asr.with_final_delay(ms(5_000)),
        "mt" => mt_script = mt_script.with_token_delay(ms(5_000)),
        "tts" => tts_script = tts_script.with_chunk_delay(ms(5_000)),
        _ => {}
    }
    let mt = FakeTranslator::new(clock.clone()).with_script("今天天气很好", mt_script);
    let tts = FakeSpeechSynth::new(clock.clone()).with_script("Nice day.", tts_script);
    let rig = Rig::start_with_asr(&clock, utterances(1, 0), asr, mt, tts, |_| {});
    rig.wait_running();
    match stage {
        "idle" => {}
        "asr" => {
            rig.feed_nowait(40);
            pipeline_rig::wait_log(&rig.fakes.asr.log(), K::Started, 1);
        }
        "mt" => {
            rig.feed(55);
            pipeline_rig::wait_log(&rig.fakes.mt.log(), K::Opened, 1);
        }
        "tts" => {
            rig.feed(55);
            pipeline_rig::wait_log(&rig.fakes.tts.log(), K::Opened, 1);
        }
        _ => {
            rig.feed(55);
            wait_audio_end(&rig, clause(1, 0));
        }
    }
    let report = rig.stop();
    assert!(report.clean && report.leaked == 0, "{stage}: {report:?}");
    for log in [rig.fakes.mt.log(), rig.fakes.tts.log()] {
        pipeline_rig::assert_every_request_ended(&log);
    }
    if stage == "asr" {
        assert_eq!(
            rig.fakes.asr.log().count(K::Cancelled),
            1,
            "the finalize call was cancelled"
        );
    }
    assert_eq!(rig.latest().state, DirectionState::Off, "{stage}");
}

#[test]
fn stop_ends_cleanly_at_every_stage() {
    for stage in ["idle", "asr", "mt", "tts", "playing"] {
        stop_in(stage);
    }
}
