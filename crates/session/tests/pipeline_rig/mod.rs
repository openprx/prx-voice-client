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
        // A clause-level flush does not touch what is playing (a skipped clause never played).
        if matches!(
            e,
            PipelineEvent::Flush(CancelAction::CancelSegment(_) | CancelAction::CancelDirection(_))
        ) {
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
    /// When the clause's last chunk arrived (its `AudioEnd`).
    released_at: MonoTime,
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
    let mut p = PipelineParams::new(
        direction,
        source,
        target,
        VoiceSelection::Preset {
            name: "preset-a".to_owned(),
        },
    );
    // Most fixtures script an English translation for the incoming direction, whatever its
    // target; the wrong-language retry has its own tests, which switch it on.
    p.retry_wrong_language = false;
    p
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
            FakeTraceSink::new(false),
        )
    }

    /// Like [`Rig::start_with`] for the outgoing direction.
    pub fn start_with_asr(
        clock: &ManualClock,
        vad: VadScript,
        asr: AsrScript,
        mt: FakeTranslator,
        tts: FakeSpeechSynth,
        customize: impl FnOnce(&mut PipelineParams),
    ) -> Self {
        Self::start_with(clock, Direction::Outgoing, vad, asr, mt, tts, customize)
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
        Self::launch(clock.clone(), fakes, p, &call, FakeTraceSink::new(false))
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
    /// In batches that fit the capture queue, so that the test never makes it drop frames.
    pub fn feed(&self, n: u64) {
        let mut left = n;
        while left > 0 {
            let batch = left.min(30);
            self.feed_nowait(batch);
            self.wait_consumed();
            left -= batch;
        }
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

    /// Wait for events; on timeout the latest snapshot and the stats are part of the message.
    pub fn wait_events(&self, what: &str, ready: impl Fn(&[PipelineEvent]) -> bool) {
        let deadline = Instant::now() + WATCHDOG;
        while !ready(&self.recorder.events()) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for: {what}\nsnapshot: {:#?}\nstats: {:?}\nevents:\n{}",
                self.snaps.latest(),
                self.pipeline.stats(),
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
                            released_at: clock.now(),
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
                // Playing cannot end before the last chunk exists.
                pipeline.playback_finished(job.clause, job.end.max(job.released_at));
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

/// `n` single-clause utterances of speech separated by silences long enough to end each segment.
pub fn utterances(n: usize, gap_frames: usize) -> VadScript {
    let mut runs = vec![(0.0, 10)];
    for i in 0..n {
        runs.push((0.9, 20));
        runs.push((0.0, if i + 1 == n { 25 } else { gap_frames }));
    }
    vad_runs(&runs)
}

/// The recogniser script of `texts`, one per utterance: every segment finalises twice (at the clause pause, then at the segment end).
pub fn asr_script(texts: &[&str]) -> AsrScript {
    let partials = |t: &'static str| -> Vec<(&'static str, usize)> { vec![(t, 0); 60] };
    let leak = |t: &str| -> &'static str { Box::leak(t.to_owned().into_boxed_str()) };
    let mut it = texts.iter();
    let first = it.next().map_or("", |t| leak(t));
    let mut script = AsrScript::new(partials(first).into_iter().map(|(t, _)| t), first);
    script = script.then_round([], "");
    for t in it {
        script = script.then_round(partials(leak(t)), t).then_round([], "");
    }
    script
}

/// A translator and a synthesiser that answer `pairs` of (source, translation) with `chunks` chunks of 100 ms each; the probe text works too.
pub fn simple_providers(
    clock: &ManualClock,
    pairs: &[(&str, &'static [&'static str])],
    chunks: usize,
) -> (FakeTranslator, FakeSpeechSynth) {
    use prxvc_ports::fakes::{MtScript, TtsScript};
    let mut mt =
        FakeTranslator::new(clock.clone()).with_script("Hello.", MtScript::tokens(["Hello."]));
    let mut tts =
        FakeSpeechSynth::new(clock.clone()).with_script("Hello.", TtsScript::chunks(1, 2400));
    for (src, tokens) in pairs {
        mt = mt.with_script(src, MtScript::tokens(tokens.iter().copied()));
        tts = tts.with_script(&tokens.concat(), TtsScript::chunks(chunks, 2400));
    }
    (mt, tts)
}

/// Wrappers that can be broken and repaired from a test: while `broken`, opening fails.
pub mod flaky {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use prxvc_media::Lang;
    use prxvc_ports::{
        AsrContext, AsrError, AsrStream, ProviderDescriptor, StreamingAsr, VadEngine, VadError,
        VadProvider,
    };

    #[derive(Clone)]
    pub struct FlakyVad<T> {
        pub inner: T,
        pub broken: Arc<AtomicBool>,
    }

    impl<T: VadProvider> VadProvider for FlakyVad<T> {
        fn descriptor(&self) -> ProviderDescriptor {
            self.inner.descriptor()
        }

        fn open(&self) -> Result<Box<dyn VadEngine>, VadError> {
            if self.broken.load(Ordering::SeqCst) {
                return Err(VadError::Unavailable {
                    reason: "broken for the test".to_owned(),
                });
            }
            self.inner.open()
        }
    }

    #[derive(Clone)]
    pub struct FlakyAsr<T> {
        pub inner: T,
        pub broken: Arc<AtomicBool>,
    }

    impl<T: StreamingAsr> StreamingAsr for FlakyAsr<T> {
        fn descriptor(&self) -> ProviderDescriptor {
            self.inner.descriptor()
        }

        fn open(&self, lang: Lang, ctx: AsrContext) -> Result<Box<dyn AsrStream>, AsrError> {
            if self.broken.load(Ordering::SeqCst) {
                return Err(AsrError::Unavailable {
                    reason: "broken for the test".to_owned(),
                });
            }
            self.inner.open(lang, ctx)
        }
    }
}

/// Wait until `log` holds `n` entries of `kind`; yields, panics at the watchdog.
pub fn wait_log(log: &prxvc_ports::fakes::EventLog, kind: prxvc_ports::fakes::EventKind, n: usize) {
    let deadline = Instant::now() + WATCHDOG;
    while log.count(kind) < n {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {n} x {kind:?}: {:?}",
            log.events()
        );
        thread::yield_now();
    }
}

/// Every request in `log` ended: its last entry is a terminal one.
pub fn assert_every_request_ended(log: &prxvc_ports::fakes::EventLog) {
    use prxvc_ports::fakes::EventKind as K;
    let events = log.events();
    let mut names: Vec<&str> = events.iter().map(|e| e.request.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    for name in names {
        let last = log.kinds_for(name).last().copied();
        assert!(
            matches!(
                last,
                Some(K::Completed | K::Cancelled | K::Failed | K::Dropped)
            ),
            "request {name} did not end: {:?}",
            log.kinds_for(name)
        );
    }
}

/// Stages that follow each other causally when the translation is not split: their times never go backwards.
/// (`TtsDone` is left out: playback starts at the first chunk, while synthesis goes on.)
pub const OUTPUT_CHAIN: [TraceStage; 9] = [
    TraceStage::ClauseCommitted,
    TraceStage::AsrFinalReady,
    TraceStage::MtRequestSent,
    TraceStage::MtFirstToken,
    TraceStage::MtDone,
    TraceStage::TtsRequestSent,
    TraceStage::TtsFirstChunk,
    TraceStage::PlayoutStart,
    TraceStage::PlayoutEnd,
];
