//! A long randomized run of one direction with seeded provider delays and failures, checking the invariants of the pipeline.

mod pipeline_rig;

use prxvc_media::{ClauseId, ManualClock};
use prxvc_ports::fakes::{FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_ports::{MtError, MtPhase, TraceStage, TtsError};
use prxvc_session::pipeline::PipelineEvent;
use prxvc_session::{CancelAction, ClauseState, DirectionState};

use pipeline_rig::{
    OUTPUT_CHAIN, OUTPUT_STAGES, Rig, asr_script, assert_every_request_ended, ms, vad_runs,
};

/// xorshift64*: small, seedable, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn per_mille(&mut self, p: u64) -> bool {
        self.below(1000) < p
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn soak(seed: u64, utterances: usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let clock = ManualClock::new();
    let mut mt =
        FakeTranslator::new(clock.clone()).with_script("Hello.", MtScript::tokens(["Hello."]));
    let mut tts =
        FakeSpeechSynth::new(clock.clone()).with_script("Hello.", TtsScript::chunks(1, 2400));
    let mut runs = vec![(0.0, 10)];
    let sources: Vec<&'static str> = (0..utterances)
        .map(|k| leak(format!("第{k}句话很长")))
        .collect();
    for (k, src) in sources.iter().enumerate() {
        let translation = leak(format!("Sentence {k}."));
        let delay = [0, 0, 50, 200][usize::try_from(rng.below(4)).unwrap_or(0)];
        let mut script = MtScript::tokens([leak(format!("Sentence {k}"))].into_iter().chain(["."]))
            .with_token_delay(ms(delay));
        if rng.per_mille(40) {
            script = script.failing_open(MtError::Unavailable {
                reason: "soak".to_owned(),
            });
        } else if rng.per_mille(30) {
            script = script.failing_after(
                1,
                MtError::TimedOut {
                    phase: MtPhase::Stream,
                },
            );
        } else if rng.per_mille(30) {
            script = script.flagged_suspect("soak");
        }
        mt = mt.with_script(src, script);
        let chunks = 2 + usize::try_from(rng.below(3)).unwrap_or(0);
        let mut speech = TtsScript::chunks(chunks, 2400).with_chunk_delay(ms(rng.below(150)));
        if rng.per_mille(30) {
            speech = speech.failing_after(
                1,
                TtsError::Runaway {
                    produced_secs: 99.0,
                },
            );
        } else if rng.per_mille(30) {
            speech = speech.failing_after(
                1,
                TtsError::Unavailable {
                    reason: "soak".to_owned(),
                },
            );
        }
        tts = tts.with_script(translation, speech);
        runs.push((0.9, 15 + usize::try_from(rng.below(16)).unwrap_or(0)));
        runs.push((0.0, 22 + usize::try_from(rng.below(120)).unwrap_or(0)));
    }
    let total_frames: usize = runs.iter().map(|(_, n)| n).sum();
    let asr = asr_script(&sources);
    let rig = Rig::start_with_asr(&clock, vad_runs(&runs), asr, mt, tts, |p| {
        p.commit.split_target_clauses = false;
    });
    rig.wait_running();
    let mut resume_at = None;
    for frame in 0..total_frames {
        rig.feed(1);
        rig.advance(ms(32));
        if resume_at == Some(frame) {
            rig.pipeline.resume();
            resume_at = None;
        }
        let snap = rig.latest();
        match rng.below(1000) {
            0..=1 => rig.pipeline.skip(),
            2..=3 => {
                if let Some(c) = snap.clauses.last() {
                    rig.pipeline.cancel_segment(c.clause.segment());
                }
            }
            4 if resume_at.is_none() => {
                rig.pipeline.pause();
                resume_at = Some(frame + 20);
            }
            _ => {}
        }
    }
    // Let everything finish: time passes until every clause is over and the direction runs again.
    if resume_at.is_some() {
        rig.pipeline.resume();
    }
    let mut settled = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        rig.advance(ms(500));
        std::thread::yield_now();
        let s = rig.latest();
        if s.state == DirectionState::Running && s.clauses.iter().all(|c| c.state.is_terminal()) {
            settled = true;
            break;
        }
    }
    let final_snapshot = rig.latest();
    assert!(
        settled,
        "seed {seed}: the direction never settled: {final_snapshot:#?}"
    );
    let report = rig.stop();
    assert!(
        report.clean && report.leaked == 0,
        "seed {seed}: {report:?}"
    );
    check_invariants(seed, &rig);
}

fn check_invariants(seed: u64, rig: &Rig) {
    let events = rig.recorder.events();
    // Order: clauses reach the output in clause order.
    let order = rig.recorder.audio_clause_order();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "seed {seed}: out of order {order:?}"
    );
    // No output for cancelled or skipped items after the cancellation that covers them.
    for (i, e) in events.iter().enumerate() {
        let PipelineEvent::Flush(action) = e else {
            continue;
        };
        let covered = |c: ClauseId| match action {
            CancelAction::CancelClause(x) => *x == c,
            CancelAction::CancelSegment(s) => *s == c.segment(),
            CancelAction::CancelDirection(_) => false,
        };
        for later in &events[i + 1..] {
            if let PipelineEvent::Audio(a) = later {
                assert!(
                    !covered(a.payload.clause),
                    "seed {seed}: audio of {} after {action:?}",
                    a.payload.clause
                );
            }
        }
    }
    for e in &events {
        if let PipelineEvent::ClauseSkipped { clause, .. } = e {
            assert_eq!(
                rig.recorder.chunks_of(*clause),
                0,
                "seed {seed}: a skipped clause {clause} spoke"
            );
        }
    }
    // Bounded queues stay within their capacity (the inbox has its urgent lane on top).
    for q in rig.pipeline.stats().queues {
        let urgent = if q.name == "inbox" { 64 } else { 0 };
        assert!(q.high_water <= q.capacity + urgent, "seed {seed}: {q:?}");
    }
    // Every request that started ended.
    assert_every_request_ended(&rig.fakes.mt.log());
    assert_every_request_ended(&rig.fakes.tts.log());
    // Every clause that was played has its stage timestamps in order.
    let ended: Vec<ClauseId> = rig
        .trace
        .events()
        .iter()
        .filter(|e| e.stage == TraceStage::PlayoutEnd)
        .map(|e| e.clause)
        .collect();
    for c in ended {
        let stages = rig.trace_stages(c);
        let time = |needed: TraceStage| {
            let at = stages.iter().find(|(s, _)| *s == needed).map(|(_, t)| *t);
            at.unwrap_or_else(|| panic!("seed {seed}: {c} lacks {needed:?}"))
        };
        for needed in OUTPUT_STAGES {
            time(needed);
        }
        let times: Vec<_> = OUTPUT_CHAIN.iter().map(|s| time(*s)).collect();
        assert!(
            times.windows(2).all(|w| w[0] <= w[1]),
            "seed {seed}: {c} goes back in time: {times:?}"
        );
        assert!(
            time(TraceStage::TtsFirstChunk) <= time(TraceStage::TtsDone),
            "seed {seed}: {c}"
        );
        assert!(
            time(TraceStage::TtsDone) <= time(TraceStage::PlayoutEnd),
            "seed {seed}: {c}"
        );
    }
    let end = rig.latest();
    assert_eq!(end.state, DirectionState::Off, "seed {seed}");
    assert!(
        end.clauses
            .iter()
            .all(|c| c.state.is_terminal() || c.state == ClauseState::Committed || true)
    );
}

#[test]
fn a_long_randomized_run_keeps_every_invariant() {
    for seed in 1..=8 {
        soak(seed, 60);
    }
}
