//! Provider failures, overflow policies, two directions at once and the trace and snapshot contracts, on fakes and a manual clock.

mod pipeline_rig;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use prxvc_media::{ClauseId, Direction, ManualClock, MonoTime, SegmentId};
use prxvc_ports::fakes::{
    AsrScript, EventKind, FakeSpeechSynth, FakeStreamingAsr, FakeTraceSink, FakeTranslator,
    FakeVadProvider, MtScript, TtsScript,
};
use prxvc_ports::{AsrError, CancellationToken, MtError, MtPhase, TraceStage, TtsError};
use prxvc_session::pipeline::{PipelineEvent, PipelineProviders};
use prxvc_session::{ClauseState, DegradeReason, DirectionState, FaultKind, OutputSource, Stage};

use pipeline_rig::flaky::{FlakyAsr, FlakyVad};
use pipeline_rig::{
    Fakes, OUTPUT_CHAIN, Rig, asr_script, ms, params, simple_providers, utterances, wait_log,
};

const SRC: &str = "今天天气很好";
const OUT: Direction = Direction::Outgoing;
const UNAVAILABLE: fn() -> String = || "down".to_owned();

fn clause(dir: Direction, seg: u64, index: u16) -> ClauseId {
    ClauseId::new(SegmentId::new(dir, seg).expect("seq >= 1"), index)
}

fn failure_asr() -> AsrScript {
    AsrScript::new(vec![SRC; 60], SRC).then_round([], "")
}

/// Which provider breaks, and how.
struct Case {
    name: &'static str,
    stage: Stage,
    fault: FaultKind,
}

const CASES: [Case; 12] = [
    Case {
        name: "vad_open",
        stage: Stage::Vad,
        fault: FaultKind::Unavailable,
    },
    Case {
        name: "asr_open",
        stage: Stage::Asr,
        fault: FaultKind::Unavailable,
    },
    Case {
        name: "asr_mid_stream",
        stage: Stage::Asr,
        fault: FaultKind::Crashed,
    },
    Case {
        name: "asr_timeout",
        stage: Stage::Asr,
        fault: FaultKind::Timeout,
    },
    Case {
        name: "mt_open",
        stage: Stage::Mt,
        fault: FaultKind::Unavailable,
    },
    Case {
        name: "mt_mid_stream",
        stage: Stage::Mt,
        fault: FaultKind::Crashed,
    },
    Case {
        name: "mt_timeout",
        stage: Stage::Mt,
        fault: FaultKind::Timeout,
    },
    Case {
        name: "tts_open",
        stage: Stage::Tts,
        fault: FaultKind::Unavailable,
    },
    Case {
        name: "tts_mid_stream",
        stage: Stage::Tts,
        fault: FaultKind::Crashed,
    },
    Case {
        name: "tts_timeout",
        stage: Stage::Tts,
        fault: FaultKind::Timeout,
    },
    Case {
        name: "mt_suspect",
        stage: Stage::Mt,
        fault: FaultKind::Rejected,
    },
    Case {
        name: "tts_rejected",
        stage: Stage::Tts,
        fault: FaultKind::Rejected,
    },
];

fn run_case(case: &Case) {
    let clock = ManualClock::new();
    let (mut mt, mut tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 1);
    let mut asr = failure_asr();
    let broken = Arc::new(AtomicBool::new(false));
    let mt_script = || MtScript::tokens(["Nice ", "day."]);
    let tts_script = || TtsScript::chunks(2, 2400);
    match case.name {
        "vad_open" | "asr_open" => broken.store(true, Ordering::SeqCst),
        "asr_mid_stream" => {
            asr = asr.failing_feed_at(
                3,
                AsrError::Unavailable {
                    reason: UNAVAILABLE(),
                },
            )
        }
        "asr_timeout" => asr = asr.failing_finalize(AsrError::Timeout),
        "mt_open" => {
            mt = mt.with_script(
                SRC,
                mt_script().failing_open(MtError::Unavailable {
                    reason: UNAVAILABLE(),
                }),
            )
        }
        "mt_mid_stream" => {
            mt = mt.with_script(
                SRC,
                mt_script().failing_after(
                    1,
                    MtError::Unavailable {
                        reason: UNAVAILABLE(),
                    },
                ),
            )
        }
        "mt_timeout" => {
            mt = mt.with_script(
                SRC,
                mt_script().failing_after(
                    0,
                    MtError::TimedOut {
                        phase: MtPhase::FirstToken,
                    },
                ),
            )
        }
        "mt_suspect" => mt = mt.with_script(SRC, mt_script().flagged_suspect("refusal")),
        "tts_open" => {
            tts = tts.with_script(
                "Nice day.",
                tts_script().failing_open(TtsError::Unavailable {
                    reason: UNAVAILABLE(),
                }),
            )
        }
        "tts_mid_stream" => {
            tts = tts.with_script(
                "Nice day.",
                tts_script().failing_after(
                    1,
                    TtsError::Unavailable {
                        reason: UNAVAILABLE(),
                    },
                ),
            )
        }
        "tts_timeout" => {
            tts = tts.with_script(
                "Nice day.",
                tts_script().failing_after(0, TtsError::Timeout),
            )
        }
        _ => {
            tts = tts.with_script(
                "Nice day.",
                tts_script().failing_after(0, TtsError::Rejected { status: 429 }),
            )
        }
    }
    let fakes = Fakes {
        vad: FakeVadProvider::new(utterances(1, 0)),
        asr: FakeStreamingAsr::new(clock.clone(), asr),
        mt,
        tts,
    };
    let providers = PipelineProviders {
        vad: Arc::new(FlakyVad {
            inner: fakes.vad.clone(),
            broken: Arc::new(AtomicBool::new(
                case.name == "vad_open" && broken.load(Ordering::SeqCst),
            )),
        }),
        asr: Arc::new(FlakyAsr {
            inner: fakes.asr.clone(),
            broken: Arc::new(AtomicBool::new(case.name == "asr_open")),
        }),
        refine: None,
        mt: Arc::new(fakes.mt.clone()),
        tts: Arc::new(fakes.tts.clone()),
    };
    // Keep handles on the flags the wrappers read, to repair them later.
    let vad_flag = Arc::new(AtomicBool::new(case.name == "vad_open"));
    let asr_flag = Arc::new(AtomicBool::new(case.name == "asr_open"));
    let providers = PipelineProviders {
        vad: Arc::new(FlakyVad {
            inner: fakes.vad.clone(),
            broken: Arc::clone(&vad_flag),
        }),
        asr: Arc::new(FlakyAsr {
            inner: fakes.asr.clone(),
            broken: Arc::clone(&asr_flag),
        }),
        ..providers
    };
    let rig = Rig::launch_with_providers(
        clock.clone(),
        fakes,
        providers,
        params(OUT),
        &CancellationToken::new(),
        FakeTraceSink::new(false),
    );
    let opens_broken = matches!(case.name, "vad_open" | "asr_open");
    if !opens_broken {
        rig.wait_running();
        rig.feed(55);
    }
    let failed = rig.wait_snapshot(case.name, |s| {
        matches!(s.state, DirectionState::Failed { .. })
    });
    assert_eq!(
        failed.state,
        DirectionState::Failed {
            stage: case.stage,
            fault: case.fault
        },
        "{}",
        case.name
    );
    assert_eq!(
        failed.output,
        OutputSource::Original,
        "{}: the failure policy plays the original",
        case.name
    );
    assert_eq!(
        failed.last_error.as_ref().map(|e| (e.stage, e.fault)),
        Some((case.stage, case.fault)),
        "{}",
        case.name
    );
    rig.wait_events(case.name, |e| e.iter().any(|e| matches!(e, PipelineEvent::Failed { stage, fault } if *stage == case.stage && *fault == case.fault)));
    if case.name == "mt_suspect" {
        assert_eq!(failed.counters.mt_suspect, 1);
    }
    assert_eq!(
        rig.recorder
            .count(|e| matches!(e, PipelineEvent::Audio(a) if a.payload.chunk.samples.is_empty())),
        0
    );
    vad_flag.store(false, Ordering::SeqCst);
    asr_flag.store(false, Ordering::SeqCst);
    // The probe is due after 2 s on the injected clock; the provider works again.
    rig.advance(ms(2_000));
    let healthy = rig.wait_snapshot(case.name, |s| matches!(s.state, DirectionState::Running));
    assert_eq!(
        healthy.output,
        OutputSource::Translation,
        "{}: recovered",
        case.name
    );
    rig.wait_events(case.name, |e| {
        e.iter()
            .filter(|e| matches!(e, PipelineEvent::Output { .. }))
            .count()
            >= if opens_broken { 1 } else { 3 }
    });
    let outputs = rig.recorder.outputs();
    assert_eq!(
        outputs.last(),
        Some(&(OutputSource::Original, OutputSource::Translation)),
        "{}: {outputs:?}",
        case.name
    );
    assert!(rig.stop().clean, "{}", case.name);
}

#[test]
fn every_provider_failing_applies_the_failure_policy_and_recovers_when_it_works_again() {
    for case in &CASES {
        run_case(case);
    }
}

#[test]
fn a_runaway_synthesis_loses_the_clause_but_not_the_direction() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 1);
    let tts = tts.with_script(
        "Nice day.",
        TtsScript::chunks(2, 2400).failing_after(
            1,
            TtsError::Runaway {
                produced_secs: 60.0,
            },
        ),
    );
    let rig = Rig::start(&clock, utterances(1, 0), failure_asr(), mt, tts);
    rig.wait_running();
    rig.feed(55);
    let snap = rig.wait_snapshot("clause failed", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.state == ClauseState::Failed(Stage::Tts))
    });
    assert_eq!(snap.counters.runaway_synth, 1);
    assert_eq!(snap.state, DirectionState::Running);
    assert!(rig.stop().clean);
}

fn advance_until(rig: &Rig, step: u64, what: &str, ready: impl Fn(&[PipelineEvent]) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready(&rig.recorder.events()) {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        rig.advance(ms(step));
        std::thread::yield_now();
    }
}

#[test]
fn a_recognizer_that_cannot_keep_up_drops_the_oldest_audio_and_degrades_the_direction_until_it_catches_up()
 {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 1);
    let asr = failure_asr().with_final_delay(ms(5_000));
    let rig = Rig::start(&clock, utterances(1, 0), asr, mt, tts);
    rig.wait_running();
    rig.feed_nowait(40);
    wait_log(&rig.fakes.asr.log(), EventKind::Started, 1);
    rig.feed_nowait(120);
    assert!(
        rig.pipeline.stats().queues[0].evicted > 0,
        "Q1 drops the oldest frames when full"
    );
    advance_until(&rig, 5_000, "overrun raised", |e| {
        e.contains(&PipelineEvent::Degraded(DegradeReason::CaptureOverrun))
    });
    advance_until(&rig, 5_000, "overrun cleared", |e| {
        e.contains(&PipelineEvent::Recovered(DegradeReason::CaptureOverrun))
    });
    let snap = rig.latest();
    assert!(snap.counters.capture_overruns > 0);
    let stats = rig.pipeline.stats();
    assert!(stats.queues[0].high_water <= stats.queues[0].capacity);
    assert!(rig.stop().clean);
}

#[test]
fn a_full_translation_queue_skips_the_oldest_waiting_clause_and_reports_it() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone()).with_script(
        SRC,
        MtScript::tokens(["Slow ", "one."]).with_token_delay(ms(100_000)),
    );
    let tts = FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400));
    let asr = asr_script(&[SRC; 11]);
    let rig = Rig::start_with_asr(&clock, utterances(11, 25), asr, mt, tts, |p| {
        p.max_backlog = std::time::Duration::from_secs(10_000);
        p.interrupt.barge_in = false;
    });
    rig.wait_running();
    rig.feed(55 * 11);
    let snap = rig.wait_snapshot("clauses skipped", |s| s.counters.skipped >= 2);
    assert_eq!(
        snap.counters.skipped, 2,
        "one in flight and eight queued leave two of eleven to make room"
    );
    assert_eq!(rig.pipeline.stats().queues[3].evicted, 2);
    assert!(rig.recorder.events().iter().any(|e| matches!(
        e,
        PipelineEvent::ClauseSkipped {
            reason: prxvc_session::pipeline::SkipReason::QueueFull,
            ..
        }
    )));
    assert!(rig.stop().clean);
}

#[test]
fn a_slow_synthesiser_blocks_translation_instead_of_losing_pieces() {
    let clock = ManualClock::new();
    let tokens: &'static [&'static str] = &[
        "One ", "two. ", "One ", "two. ", "One ", "two. ", "One ", "two. ", "One ", "two. ",
        "One ", "two. ", "One ", "two. ", "One ", "two. ", "One ", "two. ", "One ", "two. ",
        "One ", "two. ", "End ", "now.",
    ];
    let (mt, _) = simple_providers(&clock, &[(SRC, tokens)], 1);
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_default_script(TtsScript::chunks(1, 2400).with_chunk_delay(ms(100_000)));
    let rig = Rig::start_with(&clock, OUT, utterances(1, 0), failure_asr(), mt, tts, |p| {
        p.commit.split_target_clauses = true;
        p.commit.split_min_tokens_after = 1;
    });
    rig.wait_running();
    rig.feed(55);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while rig.pipeline.stats().queues[4].len < 8 {
        assert!(
            std::time::Instant::now() < deadline,
            "the synthesis queue never filled"
        );
        std::thread::yield_now();
    }
    let q = rig.pipeline.stats().queues[4].clone();
    assert_eq!(
        (q.evicted, q.rejected),
        (0, 0),
        "the queue before synthesis blocks, it never drops"
    );
    assert_eq!(
        rig.fakes.mt.log().count(EventKind::Completed),
        0,
        "translation waits for room"
    );
    assert!(rig.stop().clean);
}

#[test]
fn stop_does_not_wait_for_a_consumer_that_never_reads_the_events() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 1);
    let rig = Rig::start_with(&clock, OUT, utterances(1, 0), failure_asr(), mt, tts, |p| {
        p.queues.events_out = 1
    });
    rig.wait_running();
    rig.feed(55);
    assert!(rig.stop().clean);
}

#[test]
fn two_directions_share_the_providers_and_cancelling_one_leaves_the_other_untouched() {
    let clock = ManualClock::new();
    let call = CancellationToken::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(SRC, MtScript::tokens(["Nice ", "day."]))
        .with_script("good morning", MtScript::tokens(["早上", "好。"]));
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_script(
            "Nice day.",
            TtsScript::chunks(3, 2400).with_chunk_delay(ms(1_000)),
        )
        .with_script("早上好。", TtsScript::chunks(2, 2400));
    let make = |dir: Direction, asr: AsrScript| {
        let fakes = Fakes {
            vad: FakeVadProvider::new(utterances(1, 0)),
            asr: FakeStreamingAsr::new(clock.clone(), asr),
            mt: mt.clone(),
            tts: tts.clone(),
        };
        Rig::launch(
            clock.clone(),
            fakes,
            params(dir),
            &call,
            FakeTraceSink::new(false),
        )
    };
    let out = make(OUT, failure_asr());
    let inc = make(
        Direction::Incoming,
        AsrScript::new(vec!["good morning"; 60], "good morning").then_round([], ""),
    );
    out.wait_running();
    inc.wait_running();
    out.feed(55);
    inc.feed(55);
    wait_log(&tts.log(), EventKind::Opened, 2);
    out.pipeline.skip();
    wait_log(&tts.log(), EventKind::Cancelled, 1);
    let in_clause = clause(Direction::Incoming, 1, 0);
    inc.wait_events("incoming audio", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == in_clause))
    });
    assert_eq!(inc.recorder.chunks_of(in_clause), 2);
    assert_eq!(
        out.recorder.count(|e| matches!(e, PipelineEvent::Audio(_))),
        0,
        "the cancelled direction speaks nothing"
    );
    assert_eq!(inc.latest().state, DirectionState::Running);
    assert_eq!(
        inc.recorder.count(|e| matches!(e, PipelineEvent::Flush(_))),
        0,
        "the other direction saw no cancellation"
    );
    assert!(out.stop().clean);
    assert!(inc.stop().clean);
}

#[test]
fn every_clause_that_reaches_the_output_has_all_its_stage_timestamps_in_order_and_no_text_unless_asked()
 {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 2);
    let rig = Rig::start(&clock, utterances(1, 0), failure_asr(), mt, tts);
    rig.wait_running();
    rig.feed(55);
    let c = clause(OUT, 1, 0);
    rig.wait_events("audio", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == c))
    });
    rig.advance(ms(200));
    rig.wait_snapshot("played", |s| s.counters.clauses_played == 1);
    let stages = rig.trace_stages(c);
    let at = |stage: TraceStage| -> MonoTime {
        stages
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, t)| *t)
            .unwrap_or_else(|| panic!("{stage:?} missing in {stages:?}"))
    };
    let times: Vec<MonoTime> = OUTPUT_CHAIN.iter().map(|s| at(*s)).collect();
    assert!(
        times.windows(2).all(|w| w[0] <= w[1]),
        "stage times go backwards: {times:?}"
    );
    assert!(at(TraceStage::PlayoutEnd) > at(TraceStage::PlayoutStart));
    assert!(stages.iter().any(|(s, _)| *s == TraceStage::SpeechStart));
    assert!(
        rig.trace.events().iter().all(|e| e.text.is_none()),
        "the product's sink never gets text"
    );
    assert!(rig.stop().clean);
}

#[test]
fn trace_text_is_attached_only_when_the_sink_asks_for_it() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 1);
    let fakes = Fakes {
        vad: FakeVadProvider::new(utterances(1, 0)),
        asr: FakeStreamingAsr::new(clock.clone(), failure_asr()),
        mt,
        tts,
    };
    let rig = Rig::launch(
        clock.clone(),
        fakes,
        params(OUT),
        &CancellationToken::new(),
        FakeTraceSink::new(true),
    );
    rig.wait_running();
    rig.feed(55);
    rig.wait_snapshot("committed", |s| !s.clauses.is_empty());
    let text = rig
        .trace
        .events()
        .into_iter()
        .find(|e| e.stage == TraceStage::ClauseCommitted)
        .and_then(|e| e.text);
    assert_eq!(text.as_deref(), Some(SRC));
    assert!(rig.stop().clean);
}

#[test]
fn snapshots_are_versioned_consistent_and_end_in_off() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[(SRC, &["Nice ", "day."])], 1);
    let rig = Rig::start(&clock, utterances(1, 0), failure_asr(), mt, tts);
    let subscriber = rig.snaps.clone();
    let reader = std::thread::spawn(move || {
        let mut seen = 0;
        let mut versions = Vec::new();
        while let Some(s) = subscriber.wait_newer(seen) {
            assert!(s.version > seen, "latest wins, versions only grow");
            assert!(
                s.clauses.windows(2).all(|w| w[0].clause < w[1].clause),
                "clauses are in order"
            );
            if let Some(p) = &s.partial {
                assert!(
                    p.stable_prefix_len <= p.text.len()
                        && p.text.is_char_boundary(p.stable_prefix_len)
                );
            }
            assert_eq!(s.reasons, s.state.reasons());
            seen = s.version;
            versions.push(s);
        }
        versions
    });
    rig.wait_running();
    rig.feed(55);
    rig.wait_snapshot("committed", |s| !s.clauses.is_empty());
    assert!(rig.stop().clean);
    let versions = reader.join().expect("reader");
    assert!(versions.len() >= 3);
    assert_eq!(
        versions.last().map(|s| s.state.clone()),
        Some(DirectionState::Off)
    );
}

#[test]
fn two_pass_recognition_translates_the_refined_text_of_the_clause_audio() {
    use prxvc_ports::fakes::{FakeRefineAsr, RefineScript};
    let clock = ManualClock::new();
    let refined = "今天天气不错呀";
    let (mt, tts) = simple_providers(&clock, &[(refined, &["Fine ", "day."])], 1);
    let refine = FakeRefineAsr::new(
        clock.clone(),
        RefineScript::new(refined, std::time::Duration::ZERO),
    );
    let fakes = Fakes {
        vad: FakeVadProvider::new(utterances(1, 0)),
        asr: FakeStreamingAsr::new(clock.clone(), failure_asr()),
        mt,
        tts,
    };
    let providers = PipelineProviders {
        vad: Arc::new(fakes.vad.clone()),
        asr: Arc::new(fakes.asr.clone()),
        refine: Some(Arc::new(refine.clone())),
        mt: Arc::new(fakes.mt.clone()),
        tts: Arc::new(fakes.tts.clone()),
    };
    let mut p = params(OUT);
    p.recognition_mode = prxvc_session::pipeline::RecognitionMode::TwoPass;
    let rig = Rig::launch_with_providers(
        clock.clone(),
        fakes,
        providers,
        p,
        &CancellationToken::new(),
        FakeTraceSink::new(false),
    );
    rig.wait_running();
    rig.feed(55);
    let c = clause(OUT, 1, 0);
    rig.wait_events("audio", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::AudioEnd(a) if a.payload == c))
    });
    let snap = rig.latest();
    assert_eq!(
        snap.clauses[0].source_text, refined,
        "the refine result replaces the streaming text"
    );
    assert!(
        refine.audio_lens().iter().all(|n| *n > 0),
        "the refine pass got the clause audio"
    );
    assert!(
        rig.trace_stages(c)
            .iter()
            .any(|(s, _)| *s == TraceStage::RefineDone)
    );
    assert!(rig.stop().clean);
}

#[test]
fn two_pass_without_a_refine_provider_is_refused_at_start() {
    let clock = ManualClock::new();
    let (mt, tts) = simple_providers(&clock, &[], 1);
    let providers = PipelineProviders {
        vad: Arc::new(FakeVadProvider::new(utterances(1, 0))),
        asr: Arc::new(FakeStreamingAsr::new(clock.clone(), failure_asr())),
        refine: None,
        mt: Arc::new(mt),
        tts: Arc::new(tts),
    };
    let mut p = params(OUT);
    p.recognition_mode = prxvc_session::pipeline::RecognitionMode::TwoPass;
    let result = prxvc_session::pipeline::DirectionPipeline::start(
        p,
        providers,
        Arc::new(clock),
        Arc::new(FakeTraceSink::new(false)),
        &CancellationToken::new(),
    );
    assert!(matches!(
        result,
        Err(prxvc_session::pipeline::PipelineError::MissingRefine)
    ));
}
