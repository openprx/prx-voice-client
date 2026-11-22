//! Deterministic tests of the control logic: events are fed to the core directly, in any order a set of concurrent providers could produce.

use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{
    ClauseId, Clock, Direction, Envelope, ManualClock, MonoTime, PcmChunk, SampleRate, SegmentId,
};
use prxvc_ports::VoiceSelection;
use prxvc_ports::fakes::FakeTraceSink;

use super::*;
use crate::fault::{DegradeReason, FaultKind, Stage};
use crate::output::OutputSource;
use crate::pipeline::events::PipelineEvent;
use crate::pipeline::messages::{
    ClauseSamples, CommitReason, ContinuationOutcome, StageEvent, StageFailure,
};
use crate::pipeline::queue::Policy;
use crate::pipeline::snapshot::{SnapshotSubscriber, snapshot_channel};
use crate::segment::ClauseState;

const OUT: Direction = Direction::Outgoing;

fn seg(n: u64) -> SegmentId {
    SegmentId::new(OUT, n).expect("seq >= 1")
}

fn cl(s: u64, i: u16) -> ClauseId {
    ClauseId::new(seg(s), i)
}

struct Harness {
    core: Core,
    w: Wiring2,
    snaps: SnapshotSubscriber,
    clock: ManualClock,
}

/// The handles the tests keep after the wiring moved into the core.
struct Wiring2 {
    tree: Arc<TokenTree>,
    mt_in: Arc<BoundedQueue<MtItem>>,
    events: Arc<BoundedQueue<PipelineEvent>>,
    gate: Arc<Gate>,
}

fn voice() -> VoiceSelection {
    VoiceSelection::Preset {
        name: "preset-a".to_owned(),
    }
}

fn harness(customize: impl FnOnce(&mut PipelineParams)) -> Harness {
    let mut p = PipelineParams::new(OUT, prxvc_media::Lang::Zh, prxvc_media::Lang::En, voice());
    customize(&mut p);
    let clock = ManualClock::new();
    let tree = TokenTree::new(CancellationToken::new());
    let mt_in = Arc::new(BoundedQueue::new("mt_in", 8, Policy::DropOldest));
    let tts_in = Arc::new(BoundedQueue::new("tts_in", 8, Policy::Block));
    let events = Arc::new(BoundedQueue::new("events", 4096, Policy::Block));
    let gate = Arc::new(Gate::default());
    let stats = Arc::new(Stats::default());
    let (publisher, snaps) = snapshot_channel();
    let wiring = Wiring {
        tree: Arc::clone(&tree),
        inbox: Arc::new(BoundedQueue::new("inbox", 16, Policy::Block)),
        audio_in: Arc::new(BoundedQueue::new("audio", 16, Policy::DropOldest)),
        refine_in: Arc::new(BoundedQueue::new("refine", 8, Policy::DropOldest)),
        mt_in: Arc::clone(&mt_in),
        tts_in,
        events: Arc::clone(&events),
        gate: Arc::clone(&gate),
        snapshots: publisher,
        stats: Arc::clone(&stats),
        tracer: Tracer::new(Arc::new(FakeTraceSink::new(false)), Arc::new(clock.clone())),
    };
    let mut core = Core::new(wiring, p);
    core.handle(Inbound::Command(Command::Start));
    core.handle(Inbound::Worker(WorkerEvent::RecognizerReady));
    Harness {
        core,
        w: Wiring2 {
            tree,
            mt_in,
            events,
            gate,
        },
        snaps,
        clock,
    }
}

impl Harness {
    fn drain(&self) -> Vec<PipelineEvent> {
        std::iter::from_fn(|| self.w.events.try_pop()).collect()
    }

    fn speech(&mut self, s: u64) {
        self.w.tree.open_segment(seg(s));
        let e = Envelope::new(
            seg(s),
            0,
            MonoTime::from_millis(100),
            RecognitionEvent::SpeechStarted,
        );
        self.core.handle(Inbound::Recognition(e));
    }

    /// Commit a clause whose speech lasted `len_ms`, ending `end_ms` after the segment began.
    fn commit(&mut self, s: u64, text: &str, start_ms: u64, end_ms: u64) {
        let c = Commit {
            text: text.to_owned(),
            source_start: MonoTime::from_millis(start_ms),
            source_end: MonoTime::from_millis(end_ms),
            audio: None::<Arc<ClauseSamples>>,
            reason: CommitReason::ClausePause,
            continues: false,
        };
        let e = Envelope::new(
            seg(s),
            1,
            MonoTime::from_millis(100),
            RecognitionEvent::Commit(c),
        );
        self.core.handle(Inbound::Recognition(e));
    }

    fn stage(&mut self, c: ClauseId, ev: StageEvent) {
        let e = Envelope::new(c.segment(), 1, MonoTime::from_millis(100), ev);
        self.core.handle(Inbound::Stage(e));
    }

    fn command(&mut self, c: Command) {
        self.core.handle(Inbound::Command(c));
    }

    /// Run a clause through translation up to the start of synthesis.
    fn translate(&mut self, c: ClauseId) {
        self.stage(c, StageEvent::MtStarted { clause: c });
        self.stage(
            c,
            StageEvent::MtText {
                clause: c,
                delta: "Hi.".to_owned(),
            },
        );
        self.stage(c, StageEvent::MtDone { clause: c });
        self.stage(
            c,
            StageEvent::TtsStarted {
                clause: c,
                voice: voice(),
            },
        );
    }

    fn chunk(&mut self, c: ClauseId, ms: u64) {
        let chunk = PcmChunk {
            samples: vec![0.1; usize::try_from(ms).unwrap_or(0) * 24],
            rate: SampleRate::HZ_24000,
        };
        self.stage(c, StageEvent::TtsChunk { clause: c, chunk });
    }

    fn snapshot(&self) -> Arc<crate::pipeline::snapshot::DirectionSnapshot> {
        self.snaps.latest().expect("a snapshot was published")
    }

    fn late(&self) -> u64 {
        self.snapshot().counters.late_dropped
    }
}

fn audio_clauses(events: &[PipelineEvent]) -> Vec<ClauseId> {
    events
        .iter()
        .filter_map(|e| match e {
            PipelineEvent::Audio(a) => Some(a.payload.clause),
            PipelineEvent::AudioEnd(a) => Some(a.payload),
            _ => None,
        })
        .collect()
}

#[test]
fn a_later_clause_that_finishes_synthesis_first_is_released_second() {
    let mut h = harness(|_| {});
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    h.commit(1, "第二句话也长", 600, 1_100);
    let (c0, c1) = (cl(1, 0), cl(1, 1));
    h.translate(c0);
    h.translate(c1);
    // The second clause is synthesised completely before the first one starts.
    h.chunk(c1, 100);
    h.stage(c1, StageEvent::TtsDone { clause: c1 });
    assert!(
        audio_clauses(&h.drain()).is_empty(),
        "nothing is released while the first clause is not ready"
    );
    h.chunk(c0, 100);
    h.stage(c0, StageEvent::TtsDone { clause: c0 });
    assert_eq!(
        audio_clauses(&h.drain()),
        vec![c0, c0],
        "the first clause is released alone"
    );
    let now = MonoTime::from_millis(1);
    h.command(Command::PlaybackStarted {
        clause: c0,
        at: now,
    });
    h.command(Command::PlaybackFinished {
        clause: c0,
        at: now,
    });
    assert_eq!(
        audio_clauses(&h.drain()),
        vec![c1, c1],
        "the second follows once the first has played"
    );
}

#[test]
fn a_result_for_a_cancelled_clause_is_dropped_counted_and_never_spoken() {
    let mut h = harness(|_| {});
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    let c0 = cl(1, 0);
    h.translate(c0);
    h.command(Command::CancelSegment(seg(1)));
    assert_eq!(h.snapshot().clauses[0].state, ClauseState::Cancelled);
    assert!(h.w.tree.clause(c0).is_cancelled());
    let before = h.late();
    h.chunk(c0, 100);
    h.stage(c0, StageEvent::TtsDone { clause: c0 });
    h.stage(
        c0,
        StageEvent::MtText {
            clause: c0,
            delta: "late".to_owned(),
        },
    );
    assert_eq!(h.late(), before + 3);
    let events = h.drain();
    assert!(audio_clauses(&events).is_empty());
    assert!(events.iter().any(
        |e| matches!(e, PipelineEvent::Flush(CancelAction::CancelSegment(s)) if *s == seg(1))
    ));
}

#[test]
fn audio_held_beyond_the_limit_fails_that_clause_and_not_the_direction() {
    let mut h = harness(|p| p.queues.hold = Duration::from_millis(250));
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    h.commit(1, "第二句话也长", 600, 1_100);
    let (c0, c1) = (cl(1, 0), cl(1, 1));
    h.translate(c0);
    h.translate(c1);
    for _ in 0..3 {
        h.chunk(c1, 100);
    }
    let snap = h.snapshot();
    assert_eq!(snap.counters.hold_overflows, 1);
    assert_eq!(snap.clauses[1].state, ClauseState::Failed(Stage::Tts));
    assert_eq!(snap.state, DirectionState::Running);
    assert!(
        h.drain()
            .iter()
            .any(|e| matches!(e, PipelineEvent::ClauseFailed { clause, .. } if *clause == c1))
    );
}

/// Twenty Chinese characters: about 4.4 s of speech at the synthesiser's rate.
const LONG: &str = "今天下午我们要去机场接一位从上海来的老朋友";

fn backlog_harness(enabled: bool) -> Harness {
    harness(|p| {
        p.backlog = crate::backlog::BacklogParams {
            enabled,
            max_backlog: Duration::from_secs(6),
            max_lag: Duration::from_secs(10),
        };
    })
}

#[test]
fn a_long_clause_followed_by_a_short_one_is_not_skipped() {
    let mut h = backlog_harness(true);
    h.speech(1);
    h.commit(1, &LONG.repeat(2), 100, 600);
    h.commit(1, "嗯", 600, 700);
    let snap = h.snapshot();
    assert!(snap.backlog_ms > 6_000, "{}", snap.backlog_ms);
    assert_eq!(snap.clauses[0].state, ClauseState::Recognized);
    assert_eq!(snap.counters.skipped, 0);
    assert!(snap.recent_skips.is_empty());
    assert!(!snap.reasons.contains(&DegradeReason::BacklogSkipping));
}

#[test]
fn continuous_speech_builds_a_backlog_and_the_stale_oldest_clause_is_skipped_visibly() {
    let mut h = backlog_harness(true);
    h.speech(1);
    h.commit(1, LONG, 100, 1_000);
    h.commit(1, LONG, 1_000, 5_000);
    h.commit(1, LONG, 5_000, 9_000);
    let snap = h.snapshot();
    assert!(snap.backlog_ms > 6_000);
    assert_eq!(snap.counters.skipped, 0, "over the bound but not stale yet");
    assert_eq!(
        h.core.next_deadline(),
        Some(MonoTime::from_millis(11_000)),
        "the oldest clause becomes stale 10 s after its speech ended"
    );
    h.clock.advance(Duration::from_millis(12_000));
    h.core.on_timer(h.clock.now());
    let snap = h.snapshot();
    assert_eq!(snap.clauses[0].state, ClauseState::Skipped);
    assert_eq!(snap.clauses[0].skipped, Some(SkipReason::Backlog));
    assert_eq!(snap.clauses[0].source_text, LONG);
    assert_eq!(snap.clauses[1].state, ClauseState::Recognized);
    assert_eq!(snap.counters.backlog_skips, 1);
    assert_eq!(snap.counters.skipped, 1);
    assert_eq!(snap.recent_skips.len(), 1);
    assert_eq!(snap.recent_skips[0].clause, cl(1, 0));
    assert_eq!(snap.recent_skips[0].source_text, LONG);
    assert_eq!(snap.recent_skips[0].lag_ms, 11_000);
    assert!(snap.reasons.contains(&DegradeReason::BacklogSkipping));
    let events = h.drain();
    assert!(events.contains(&PipelineEvent::Degraded(DegradeReason::BacklogSkipping)));
    assert!(events.iter().any(|e| matches!(
        e,
        PipelineEvent::ClauseSkipped {
            reason: SkipReason::Backlog,
            ..
        }
    )));
    // The condition ends when what is left is below half the bound.
    h.command(Command::CancelSegment(seg(1)));
    assert!(!h.snapshot().reasons.contains(&DegradeReason::BacklogSkipping));
    assert!(h.drain().contains(&PipelineEvent::Recovered(DegradeReason::BacklogSkipping)));
}

#[test]
fn the_clause_that_is_playing_is_never_cut_and_the_newest_is_kept() {
    let mut h = backlog_harness(true);
    h.speech(1);
    h.commit(1, LONG, 100, 1_000);
    let c0 = cl(1, 0);
    h.translate(c0);
    h.chunk(c0, 100);
    h.command(Command::PlaybackStarted {
        clause: c0,
        at: MonoTime::from_millis(1_100),
    });
    h.commit(1, LONG, 1_000, 5_000);
    h.commit(1, LONG, 5_000, 9_000);
    h.commit(1, LONG, 9_000, 13_000);
    h.clock.advance(Duration::from_millis(60_000));
    h.core.on_timer(h.clock.now());
    let snap = h.snapshot();
    assert_eq!(snap.clauses[0].state, ClauseState::Playing);
    assert_eq!(snap.clauses[0].skipped, None);
    assert_eq!(snap.clauses[1].state, ClauseState::Skipped);
    assert_eq!(snap.clauses[2].state, ClauseState::Skipped);
    assert_eq!(snap.clauses[3].state, ClauseState::Recognized);
    assert_eq!(snap.counters.backlog_skips, 2);
    let order: Vec<ClauseId> = snap.recent_skips.iter().map(|s| s.clause).collect();
    assert_eq!(order, vec![cl(1, 1), cl(1, 2)], "oldest first");
}

#[test]
fn with_skipping_disabled_everything_is_kept_however_late() {
    let mut h = backlog_harness(false);
    h.speech(1);
    for i in 0..4_u64 {
        h.commit(1, LONG, i * 4_000, (i + 1) * 4_000);
    }
    h.clock.advance(Duration::from_millis(600_000));
    h.core.on_timer(h.clock.now());
    let snap = h.snapshot();
    assert!(snap.backlog_ms > 15_000);
    assert!(snap
        .clauses
        .iter()
        .all(|c| c.state == ClauseState::Recognized && c.skipped.is_none()));
    assert_eq!(snap.counters.skipped, 0);
    assert!(snap.recent_skips.is_empty());
    assert!(!snap.reasons.contains(&DegradeReason::BacklogSkipping));
    assert_eq!(h.core.next_deadline(), None);
}

#[test]
fn the_backlog_of_a_synthesised_clause_is_its_real_audio_not_the_speech_that_was_spoken() {
    let mut h = backlog_harness(true);
    h.speech(1);
    h.commit(1, &LONG.repeat(3), 100, 20_000);
    let c0 = cl(1, 0);
    h.translate(c0);
    h.chunk(c0, 400);
    h.stage(c0, StageEvent::TtsDone { clause: c0 });
    h.commit(1, "好", 20_000, 20_500);
    // Another clause is playing ahead of it, so the first one waits, as queued audio of 400 ms.
    let snap = h.snapshot();
    assert!(snap.backlog_ms < 1_500, "{}", snap.backlog_ms);
}

#[test]
fn a_failed_translation_applies_the_failure_policy_and_recovers_when_the_probe_works() {
    let mut h = harness(|_| {});
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    let c0 = cl(1, 0);
    h.stage(c0, StageEvent::MtStarted { clause: c0 });
    let failure = StageFailure::new(FaultKind::Unavailable, "down");
    h.stage(
        c0,
        StageEvent::MtFailed {
            clause: c0,
            failure: failure.clone(),
            suspect: false,
        },
    );
    let snap = h.snapshot();
    assert_eq!(
        snap.state,
        DirectionState::Failed {
            stage: Stage::Mt,
            fault: FaultKind::Unavailable
        }
    );
    assert_eq!(snap.output, OutputSource::Original);
    assert!(!h.w.gate.is_active());
    assert_eq!(snap.last_error.as_ref().map(|e| e.stage), Some(Stage::Mt));
    let events = h.drain();
    assert!(events.iter().any(|e| matches!(
        e,
        PipelineEvent::Output {
            from: OutputSource::Translation,
            to: OutputSource::Original,
            ..
        }
    )));
    // The first probe is due after 2 s on the injected clock; a failed probe doubles the wait.
    let due = h.core.next_deadline().expect("a probe is scheduled");
    assert_eq!(due, MonoTime::from_millis(2_000));
    h.clock.advance(Duration::from_secs(2));
    h.core.on_timer(h.clock.now());
    assert!(matches!(h.w.mt_in.try_pop(), Some(MtItem::Probe)));
    h.core.handle(Inbound::Worker(WorkerEvent::ProbeResult {
        stage: Stage::Mt,
        result: Err(failure),
    }));
    assert!(matches!(h.snapshot().state, DirectionState::Failed { .. }));
    assert_eq!(
        h.core.next_deadline(),
        Some(MonoTime::from_millis(2_000 + 4_000))
    );
    h.clock.advance(Duration::from_secs(4));
    h.core.on_timer(h.clock.now());
    assert!(matches!(h.w.mt_in.try_pop(), Some(MtItem::Probe)));
    h.core.handle(Inbound::Worker(WorkerEvent::ProbeResult {
        stage: Stage::Mt,
        result: Ok(()),
    }));
    let snap = h.snapshot();
    assert_eq!(snap.state, DirectionState::Running);
    assert_eq!(snap.output, OutputSource::Translation);
    assert!(h.w.gate.is_active());
    assert_eq!(h.core.next_deadline(), None);
}

#[test]
fn a_suspect_translation_fails_the_clause_counts_and_applies_the_failure_policy() {
    let mut h = harness(|_| {});
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    let c0 = cl(1, 0);
    h.stage(c0, StageEvent::MtStarted { clause: c0 });
    let failure = StageFailure::new(FaultKind::Rejected, "suspect translation: refusal");
    h.stage(
        c0,
        StageEvent::MtFailed {
            clause: c0,
            failure,
            suspect: true,
        },
    );
    let snap = h.snapshot();
    assert_eq!(snap.counters.mt_suspect, 1);
    assert_eq!(snap.clauses[0].state, ClauseState::Failed(Stage::Mt));
    assert!(matches!(
        snap.state,
        DirectionState::Failed {
            stage: Stage::Mt,
            ..
        }
    ));
    assert!(
        audio_clauses(&h.drain()).is_empty(),
        "nothing wrong or empty is ever spoken"
    );
}

#[test]
fn a_runaway_synthesis_fails_its_clause_and_the_direction_carries_on() {
    let mut h = harness(|_| {});
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    let c0 = cl(1, 0);
    h.translate(c0);
    let failure = StageFailure::new(FaultKind::Crashed, "ran away");
    h.stage(
        c0,
        StageEvent::TtsFailed {
            clause: c0,
            failure,
            runaway: true,
        },
    );
    let snap = h.snapshot();
    assert_eq!(snap.counters.runaway_synth, 1);
    assert_eq!(snap.clauses[0].state, ClauseState::Failed(Stage::Tts));
    assert_eq!(snap.state, DirectionState::Running);
}

#[test]
fn pause_announces_the_passthrough_and_resume_restores_the_translation() {
    let mut h = harness(|_| {});
    h.drain();
    h.command(Command::Pause);
    assert!(matches!(h.snapshot().state, DirectionState::Paused { .. }));
    assert!(!h.w.gate.is_active());
    assert!(h.drain().iter().any(|e| matches!(
        e,
        PipelineEvent::Output {
            to: OutputSource::Original,
            ..
        }
    )));
    h.command(Command::Resume);
    assert_eq!(h.snapshot().state, DirectionState::Running);
    assert!(h.w.gate.is_active());
    assert!(h.drain().iter().any(|e| matches!(
        e,
        PipelineEvent::Output {
            to: OutputSource::Translation,
            ..
        }
    )));
}

#[test]
fn a_refine_failure_falls_back_to_the_streaming_text_and_three_in_a_row_degrade() {
    let mut h = harness(|p| p.recognition_mode = crate::pipeline::params::RecognitionMode::TwoPass);
    h.speech(1);
    for i in 0u16..3 {
        let text = format!("第{i}句话很长很长");
        let c = Commit {
            text: text.clone(),
            source_start: MonoTime::from_millis(100 + u64::from(i) * 500),
            source_end: MonoTime::from_millis(600 + u64::from(i) * 500),
            audio: Some(Arc::new(ClauseSamples {
                samples: vec![0.0; 160],
                rate: SampleRate::HZ_16000,
            })),
            reason: CommitReason::ClausePause,
            continues: false,
        };
        let e = Envelope::new(
            seg(1),
            1,
            MonoTime::from_millis(100),
            RecognitionEvent::Commit(c),
        );
        h.core.handle(Inbound::Recognition(e));
        let id = cl(1, i);
        assert_eq!(
            h.snapshot().clauses.last().map(|c| c.state),
            Some(ClauseState::Committed)
        );
        let failure = StageFailure::new(FaultKind::Crashed, "refine failed");
        h.stage(
            id,
            StageEvent::Refined {
                clause: id,
                result: Err(failure),
            },
        );
        assert_eq!(
            h.snapshot().clauses.last().map(|c| c.source_text.clone()),
            Some(text)
        );
    }
    let snap = h.snapshot();
    assert_eq!(snap.counters.refine_fallbacks, 3);
    assert!(snap.reasons.contains(&DegradeReason::RefineFailing));
    let id = cl(1, 0);
    // A refine result for a clause that was already skipped or cancelled is late.
    h.command(Command::CancelSegment(seg(1)));
    let before = h.late();
    h.stage(
        id,
        StageEvent::Refined {
            clause: id,
            result: Ok("late".to_owned()),
        },
    );
    assert_eq!(h.late(), before + 1);
}

/// Commit `text` with refine audio as the `i`th clause of segment 1 and answer the refine job with `result`.
fn refine_with(h: &mut Harness, i: u16, text: &str, result: Result<String, StageFailure>) {
    let c = Commit {
        text: text.to_owned(),
        source_start: MonoTime::from_millis(100 + u64::from(i) * 500),
        source_end: MonoTime::from_millis(600 + u64::from(i) * 500),
        audio: Some(Arc::new(ClauseSamples {
            samples: vec![0.0; 160],
            rate: SampleRate::HZ_16000,
        })),
        reason: CommitReason::ClausePause,
        continues: false,
    };
    let e = Envelope::new(
        seg(1),
        1,
        MonoTime::from_millis(100),
        RecognitionEvent::Commit(c),
    );
    h.core.handle(Inbound::Recognition(e));
    let id = cl(1, i);
    h.stage(id, StageEvent::Refined { clause: id, result });
}

#[test]
fn a_refine_pass_that_hears_nothing_keeps_the_streaming_text_and_is_not_a_failure() {
    let mut h = harness(|p| p.recognition_mode = crate::pipeline::params::RecognitionMode::TwoPass);
    h.speech(1);
    for i in 0u16..5 {
        refine_with(&mut h, i, "第一句话很长很长", Ok(String::new()));
        assert_eq!(
            h.snapshot().clauses.last().map(|c| c.source_text.clone()),
            Some("第一句话很长很长".to_owned())
        );
    }
    let snap = h.snapshot();
    assert_eq!(snap.counters.refine_fallbacks, 5);
    assert!(
        !snap.reasons.contains(&DegradeReason::RefineFailing),
        "an empty result is not three failures in a row"
    );
}

#[test]
fn a_refine_pass_that_hears_only_a_hum_does_not_replace_the_words_of_the_streaming_text() {
    let mut h = harness(|p| p.recognition_mode = crate::pipeline::params::RecognitionMode::TwoPass);
    h.speech(1);
    refine_with(&mut h, 0, "我们明天见面", Ok("嗯。".to_owned()));
    assert_eq!(
        h.snapshot().clauses.last().map(|c| c.source_text.clone()),
        Some("我们明天见面".to_owned())
    );
    assert_eq!(h.snapshot().counters.refine_fallbacks, 1);
}

#[test]
fn a_unit_that_continues_its_sentence_reaches_translation_as_such_and_its_outcome_is_counted() {
    let mut h = harness(|_| {});
    h.speech(1);
    let c = Commit {
        text: "我们明天见面".to_owned(),
        source_start: MonoTime::from_millis(100),
        source_end: MonoTime::from_millis(600),
        audio: None::<Arc<ClauseSamples>>,
        reason: CommitReason::Punctuation,
        continues: true,
    };
    let e = Envelope::new(
        seg(1),
        1,
        MonoTime::from_millis(100),
        RecognitionEvent::Commit(c),
    );
    h.core.handle(Inbound::Recognition(e));
    let Some(MtItem::Clause(job)) = h.w.mt_in.try_pop() else {
        panic!("the clause was not queued for translation");
    };
    assert!(job.payload.continues);
    let id = cl(1, 0);
    for outcome in [
        ContinuationOutcome::Continued,
        ContinuationOutcome::Standalone,
        ContinuationOutcome::Diverged,
        ContinuationOutcome::Empty,
    ] {
        h.stage(id, StageEvent::MtContinuation { clause: id, outcome });
    }
    let counters = h.snapshot().counters;
    assert_eq!(
        (
            counters.continued,
            counters.continuation_diverged,
            counters.remainder_empty
        ),
        (1, 1, 1)
    );
}

mod aging;
mod pass_through;
mod pace;
