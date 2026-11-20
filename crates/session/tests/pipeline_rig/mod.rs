//! Test rig for the direction pipeline: fake providers on a manual clock, an event recorder and a player that plays audio on that clock.
#![allow(dead_code)] // each test file uses a different part of the rig

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use prxvc_media::{
    AudioFrame, ClauseId, Clock, Direction, Lang, ManualClock, MonoTime, SampleRate,
};
use prxvc_ports::fakes::{
    AsrScript, FakeSpeechSynth, FakeStreamingAsr, FakeTraceSink, FakeTranslator, FakeVadProvider,
    VadScript,
};
use prxvc_ports::{CancellationToken, TraceStage, VoiceSelection};
use prxvc_session::pipeline::{
    DirectionPipeline, DirectionSnapshot, EventReceiver, PipelineEvent, PipelineParams,
    PipelineProviders, SnapshotSubscriber, StopReport,
};
use prxvc_session::{CancelAction, DirectionState};

/// A frame is 512 samples at 16 kHz: 32 ms, one VAD frame.
pub const FRAME_MS: u64 = 32;
/// Safety net against a hung test; never part of what a test waits for.
const WATCHDOG: Duration = Duration::from_secs(8);

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The probability of each VAD frame: `(probability, number of frames)` runs.
pub fn vad_runs(runs: &[(f32, usize)]) -> VadScript {
    VadScript::probabilities(runs.iter().flat_map(|(p, n)| std::iter::repeat_n(*p, *n)))
}

/// `n` growing partials of `text` word by word, each with the whole text so far
/// stable except the last `unstable_words`.
pub fn growing(text: &str, per_word: usize, unstable_words: usize) -> Vec<(String, usize)> {
    let words: Vec<&str> = text.split(' ').collect();
    let mut out = Vec::new();
    for upto in 1..=words.len() {
        let so_far = words[..upto].join(" ");
        let stable_words = upto.saturating_sub(unstable_words);
        let stable = words[..stable_words].join(" ").len();
        for _ in 0..per_word {
            out.push((so_far.clone(), stable));
        }
    }
    out
}

/// Everything the pipeline produced, in order, with a wait that does not poll the wall clock.
pub struct Recorder {
    events: Mutex<Vec<PipelineEvent>>,
    changed: Condvar,
    /// Counts `Flush` events, so that the player can abandon what it plays.
    flushes: AtomicU64,
}

impl Recorder {
    pub fn events(&self) -> Vec<PipelineEvent> {
        self.events.lock().clone()
    }

    /// Block until `ready` holds for the events so far; panics at the watchdog.
    pub fn wait(&self, what: &str, ready: impl Fn(&[PipelineEvent]) -> bool) {
        let deadline = Instant::now() + WATCHDOG;
        let mut events = self.events.lock();
        while !ready(&events) {
            assert!(
                !self.changed.wait_until(&mut events, deadline).timed_out(),
                "timed out waiting for: {what}; events so far:\n{}",
                summarize(&events)
            );
        }
    }

    pub fn count(&self, pred: impl Fn(&PipelineEvent) -> bool) -> usize {
        self.events.lock().iter().filter(|e| pred(e)).count()
    }

    /// Clauses in the order their first chunk was handed out.
    pub fn audio_clause_order(&self) -> Vec<ClauseId> {
        let mut order = Vec::new();
        for e in self.events.lock().iter() {
            if let PipelineEvent::Audio(a) = e
                && !order.contains(&a.payload.clause)
            {
                order.push(a.payload.clause);
            }
        }
        order
    }

    /// The number of chunks handed out for `clause`.
    pub fn chunks_of(&self, clause: ClauseId) -> usize {
        self.count(|e| matches!(e, PipelineEvent::Audio(a) if a.payload.clause == clause))
    }

    pub fn outputs(&self) -> Vec<(prxvc_session::OutputSource, prxvc_session::OutputSource)> {
        self.events
            .lock()
            .iter()
            .filter_map(|e| match e {
                PipelineEvent::Output { from, to, .. } => Some((*from, *to)),
                _ => None,
            })
            .collect()
    }

    fn push(&self, e: PipelineEvent) {
        if matches!(e, PipelineEvent::Flush(_)) {
            self.flushes.fetch_add(1, Ordering::SeqCst);
        }
        self.events.lock().push(e);
        self.changed.notify_all();
    }
}

/// One line per event, without sample data.
pub fn summarize(events: &[PipelineEvent]) -> String {
    events
        .iter()
        .map(|e| match e {
            PipelineEvent::Audio(a) => format!(
                "Audio {} rev {} {} samples",
                a.payload.clause,
                a.revision,
                a.payload.chunk.samples.len()
            ),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct PlayJob {
    clause: ClauseId,
    end: MonoTime,
    generation: u64,
}

#[derive(Default)]
struct PlayQueue {
    jobs: Mutex<(VecDeque<PlayJob>, bool)>,
    changed: Condvar,
}

/// The providers a rig runs on, with handles for the test to read their logs.
pub struct Fakes {
    pub vad: FakeVadProvider,
    pub asr: FakeStreamingAsr,
    pub mt: FakeTranslator,
    pub tts: FakeSpeechSynth,
}

pub struct Rig {
    pub clock: ManualClock,
    pub pipeline: Arc<DirectionPipeline>,
    pub snaps: SnapshotSubscriber,
    pub recorder: Arc<Recorder>,
    pub trace: FakeTraceSink,
    pub fakes: Fakes,
    pub call: CancellationToken,
    pushed: AtomicU64,
    threads: Mutex<Vec<JoinHandle<()>>>,
    play: Arc<PlayQueue>,
}

pub fn params(direction: Direction) -> PipelineParams {
    let (source, target) = match direction {
        Direction::Outgoing => (Lang::Zh, Lang::En),
        Direction::Incoming => (Lang::En, Lang::Zh),
    };
    PipelineParams::new(
        direction,
        source,
        target,
        VoiceSelection::Preset {
            name: "preset-a".to_owned(),
        },
    )
}

impl Rig {
    /// A running outgoing pipeline on the given fakes.
    pub fn start(
        clock: &ManualClock,
        vad: VadScript,
        asr: AsrScript,
        mt: FakeTranslator,
        tts: FakeSpeechSynth,
    ) -> Self {
        Self::start_with(clock, Direction::Outgoing, vad, asr, mt, tts, |_| {})
    }

    /// Like [`Rig::start`] with a recogniser the test built itself.
    pub fn start_asr(
        clock: &ManualClock,
        vad: VadScript,
        asr: FakeStreamingAsr,
        mt: FakeTranslator,
        tts: FakeSpeechSynth,
    ) -> Self {
        let fakes = Fakes {
            vad: FakeVadProvider::new(vad),
            asr,
            mt,
            tts,
        };
        let call = CancellationToken::new();
        Self::launch(
            clock.clone(),
            fakes,
            params(Direction::Outgoing),
            &call,
            FakeTraceSink::new(true),
        )
    }

    pub fn start_with(
        clock: &ManualClock,
        direction: Direction,
        vad: VadScript,
        asr: AsrScript,
        mt: FakeTranslator,
        tts: FakeSpeechSynth,
        customize: impl FnOnce(&mut PipelineParams),
    ) -> Self {
        let fakes = Fakes {
            vad: FakeVadProvider::new(vad),
            asr: FakeStreamingAsr::new(clock.clone(), asr),
            mt,
            tts,
        };
        let mut p = params(direction);
        customize(&mut p);
        let call = CancellationToken::new();
        Self::launch(clock.clone(), fakes, p, &call, FakeTraceSink::new(true))
    }

    /// Start on existing fakes (two rigs may share a translator and a synthesiser).
    pub fn launch(
        clock: ManualClock,
        fakes: Fakes,
        p: PipelineParams,
        call: &CancellationToken,
        trace: FakeTraceSink,
    ) -> Self {
        let providers = PipelineProviders {
            vad: Arc::new(fakes.vad.clone()),
            asr: Arc::new(fakes.asr.clone()),
            refine: None,
            mt: Arc::new(fakes.mt.clone()),
            tts: Arc::new(fakes.tts.clone()),
        };
        Self::launch_with_providers(clock, fakes, providers, p, call, trace)
    }

    pub fn launch_with_providers(
        clock: ManualClock,
        fakes: Fakes,
        providers: PipelineProviders,
        p: PipelineParams,
        call: &CancellationToken,
        trace: FakeTraceSink,
    ) -> Self {
        let started = DirectionPipeline::start(
            p,
            providers,
            Arc::new(clock.clone()),
            Arc::new(trace.clone()),
            call,
        )
        .expect("the pipeline starts");
        let pipeline = Arc::new(started.pipeline);
        let recorder = Arc::new(Recorder {
            events: Mutex::new(Vec::new()),
            changed: Condvar::new(),
            flushes: AtomicU64::new(0),
        });
        let play = Arc::new(PlayQueue::default());
        let rig = Self {
            clock: clock.clone(),
            pipeline: Arc::clone(&pipeline),
            snaps: started.snapshots,
            recorder: Arc::clone(&recorder),
            trace,
            fakes,
            call: call.clone(),
            pushed: AtomicU64::new(0),
            threads: Mutex::new(Vec::new()),
            play: Arc::clone(&play),
        };
        rig.threads.lock().push(spawn_recorder(
            started.events,
            Arc::clone(&recorder),
            Arc::clone(&pipeline),
            clock.clone(),
            Arc::clone(&play),
        ));
        rig.threads
            .lock()
            .push(spawn_player(play, recorder, pipeline, clock));
        rig
    }

    /// Push `n` frames and wait until the recogniser consumed them all.
    pub fn feed(&self, n: u64) {
        self.feed_nowait(n);
        self.wait_consumed();
    }

    pub fn feed_nowait(&self, n: u64) {
        for _ in 0..n {
            let i = self.pushed.fetch_add(1, Ordering::SeqCst);
            let ts = MonoTime::from_millis(i * FRAME_MS);
            let frame = AudioFrame::new(vec![0.1; 512], SampleRate::HZ_16000, ts, i)
                .expect("a valid frame");
            self.pipeline.audio().push(frame);
        }
    }

    pub fn wait_consumed(&self) {
        let target = self.pushed.load(Ordering::SeqCst);
        let deadline = Instant::now() + WATCHDOG;
        while self.pipeline.stats().frames_consumed < target {
            assert!(
                Instant::now() < deadline,
                "the recogniser did not consume the frames: {:#?}\n{:#?}",
                self.pipeline.stats(),
                self.latest()
            );
            thread::yield_now();
        }
    }

    pub fn frames_pushed(&self) -> u64 {
        self.pushed.load(Ordering::SeqCst)
    }

    pub fn advance(&self, d: Duration) {
        self.clock.advance(d);
    }

    /// Wait for a snapshot that satisfies `ready` (looks at the latest one; yields, never sleeps).
    pub fn wait_snapshot(
        &self,
        what: &str,
        mut ready: impl FnMut(&DirectionSnapshot) -> bool,
    ) -> Arc<DirectionSnapshot> {
        let deadline = Instant::now() + WATCHDOG;
        loop {
            if let Some(s) = self.snaps.latest()
                && ready(&s)
            {
                return s;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for snapshot: {what}\n{:#?}\nevents:\n{}",
                self.snaps.latest(),
                summarize(&self.recorder.events())
            );
            thread::yield_now();
        }
    }

    pub fn wait_running(&self) {
        self.wait_snapshot("running", |s| matches!(s.state, DirectionState::Running));
    }

    pub fn latest(&self) -> Arc<DirectionSnapshot> {
        self.snaps.latest().expect("a snapshot exists")
    }

    pub fn trace_stages(&self, clause: ClauseId) -> Vec<(TraceStage, MonoTime)> {
        self.trace
            .events()
            .into_iter()
            .filter(|e| e.clause == clause)
            .map(|e| (e.stage, e.at))
            .collect()
    }

    pub fn stop(&self) -> StopReport {
        let report = self.pipeline.stop(Duration::from_secs(10));
        {
            let mut jobs = self.play.jobs.lock();
            jobs.1 = true;
            self.play.changed.notify_all();
        }
        for t in self.threads.lock().drain(..) {
            t.join().expect("rig thread");
        }
        report
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.pipeline.stop(Duration::from_secs(10));
        self.call.cancel();
        {
            let mut jobs = self.play.jobs.lock();
            jobs.1 = true;
            self.play.changed.notify_all();
        }
    }
}

fn spawn_recorder(
    events: EventReceiver,
    recorder: Arc<Recorder>,
    pipeline: Arc<DirectionPipeline>,
    clock: ManualClock,
    play: Arc<PlayQueue>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut chunks: Vec<(ClauseId, Duration, MonoTime)> = Vec::new();
        while let Some(event) = events.recv() {
            match &event {
                PipelineEvent::Audio(a) => {
                    let clause = a.payload.clause;
                    match chunks.iter_mut().find(|(c, _, _)| *c == clause) {
                        Some(entry) => entry.1 += a.payload.chunk.duration(),
                        None => {
                            let now = clock.now();
                            chunks.push((clause, a.payload.chunk.duration(), now));
                            pipeline.playback_started(clause, now);
                        }
                    }
                }
                PipelineEvent::AudioEnd(e) => {
                    let clause = e.payload;
                    if let Some(i) = chunks.iter().position(|(c, _, _)| *c == clause) {
                        let (_, total, started) = chunks.remove(i);
                        let mut jobs = play.jobs.lock();
                        jobs.0.push_back(PlayJob {
                            clause,
                            end: started.saturating_add(total),
                            generation: recorder.flushes.load(Ordering::SeqCst),
                        });
                        play.changed.notify_all();
                    }
                }
                PipelineEvent::Flush(CancelAction::CancelClause(c)) => {
                    chunks.retain(|(clause, _, _)| clause != c);
                }
                PipelineEvent::Flush(_) => chunks.clear(),
                _ => {}
            }
            recorder.push(event);
        }
    })
}

/// Plays finished clauses one after the other on the manual clock; a flush abandons the clause being played.
fn spawn_player(
    play: Arc<PlayQueue>,
    recorder: Arc<Recorder>,
    pipeline: Arc<DirectionPipeline>,
    clock: ManualClock,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            let job = {
                let mut jobs = play.jobs.lock();
                loop {
                    if let Some(job) = jobs.0.pop_front() {
                        break job;
                    }
                    if jobs.1 {
                        return;
                    }
                    play.changed.wait(&mut jobs);
                }
            };
            let flushes = &recorder.flushes;
            let abandoned = || flushes.load(Ordering::SeqCst) != job.generation;
            let stopped = || play.jobs.lock().1;
            if clock.sleep_until(job.end, &|| abandoned() || stopped()) {
                pipeline.playback_finished(job.clause, job.end);
            }
        }
    })
}

/// Trace stages that every clause that reached the output must have, in causal order.
pub const OUTPUT_STAGES: [TraceStage; 10] = [
    TraceStage::ClauseCommitted,
    TraceStage::AsrFinalReady,
    TraceStage::MtRequestSent,
    TraceStage::MtFirstToken,
    TraceStage::MtDone,
    TraceStage::TtsRequestSent,
    TraceStage::TtsFirstChunk,
    TraceStage::TtsDone,
    TraceStage::PlayoutStart,
    TraceStage::PlayoutEnd,
];
