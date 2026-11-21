//! The rig: fake providers on a manual clock, a microphone the test feeds, an output the test drains, and an event reader.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_media::{AudioFrame, Lang, ManualClock, MonoTime, PcmChunk, SampleRate};
use prxvc_ports::fakes::{
    AsrScript, DialogueScript, FakeDialogue, FakeSpeechSynth, FakeStreamingAsr, FakeTraceSink,
    FakeVadProvider, TtsScript, VadScript,
};
use prxvc_ports::{
    AudioSource, ConversationId, EndpointError, EndpointHealth, SourceInfo, VoiceSelection,
};

use super::super::{
    AssistantDeps, AssistantEvent, AssistantHandle, AssistantParams, MicGate, MicState, OpenGate,
    Phase, ReplyOutput, StartError, start,
};

/// A frame is 512 samples at 16 kHz: 32 ms, one frame of the fake detector.
pub const FRAME_MS: u64 = 32;
/// Safety net against a hung test; never part of what a test waits for.
const WATCHDOG: Duration = Duration::from_secs(8);

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The probabilities of the detector's frames: `(probability, number of frames)` runs.
pub fn vad_runs(runs: &[(f32, usize)]) -> VadScript {
    VadScript::probabilities(runs.iter().flat_map(|(p, n)| std::iter::repeat_n(*p, *n)))
}

/// What one utterance looks like to the detector: some quiet, speech, then a pause that ends the turn.
pub const ONE_UTTERANCE: [(f32, usize); 3] = [(0.0, 6), (0.9, 24), (0.0, 30)];

/// A microphone the test feeds frame by frame.
pub struct ChannelSource {
    rx: Receiver<AudioFrame>,
    stopped: Arc<AtomicBool>,
    start_error: Option<EndpointError>,
    fail_read: Arc<Mutex<Option<EndpointError>>>,
    rate: SampleRate,
}

pub struct MicFeed {
    tx: Sender<AudioFrame>,
    seq: u64,
    pub stopped: Arc<AtomicBool>,
    fail_read: Arc<Mutex<Option<EndpointError>>>,
}

impl MicFeed {
    /// Push `n` frames of audio that is loud enough to count as speech when the detector says so.
    pub fn push(&mut self, n: usize) {
        for _ in 0..n {
            let ts = MonoTime::from_millis(self.seq * FRAME_MS);
            let frame =
                AudioFrame::new(vec![0.1; 512], SampleRate::HZ_16000, ts, self.seq).expect("frame");
            self.seq += 1;
            // The loop may have ended (a fault): nobody reads any more.
            let _ = self.tx.send(frame);
        }
    }

    /// The next read fails with `error`.
    pub fn fail_with(&self, error: EndpointError) {
        *self.fail_read.lock() = Some(error);
    }
}

pub fn mic() -> (ChannelSource, MicFeed) {
    let (tx, rx) = channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let fail_read = Arc::new(Mutex::new(None));
    (
        ChannelSource {
            rx,
            stopped: Arc::clone(&stopped),
            start_error: None,
            fail_read: Arc::clone(&fail_read),
            rate: SampleRate::HZ_16000,
        },
        MicFeed {
            tx,
            seq: 0,
            stopped,
            fail_read,
        },
    )
}

impl ChannelSource {
    pub fn fail_start(&mut self, error: EndpointError) {
        self.start_error = Some(error);
    }

    pub fn set_rate(&mut self, rate: SampleRate) {
        self.rate = rate;
    }
}

impl AudioSource for ChannelSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        if let Some(e) = self.start_error.take() {
            return Err(e);
        }
        Ok(SourceInfo {
            rate: self.rate,
            device: "test-mic".to_owned(),
        })
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        if let Some(e) = self.fail_read.lock().take() {
            return Err(e);
        }
        if let Ok(frame) = self.rx.recv_timeout(max_wait) {
            out.push(frame);
            while let Ok(more) = self.rx.try_recv() {
                out.push(more);
            }
        }
        Ok(())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth::default()
    }

    fn stop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

/// What the output saw.
#[derive(Default)]
pub struct OutputProbe {
    pub chunks: AtomicUsize,
    pub samples: AtomicUsize,
    pub ends: AtomicUsize,
    pub flushes: AtomicUsize,
    pub drained: AtomicBool,
    pub refuse: AtomicBool,
}

pub struct FakeOutput(pub Arc<OutputProbe>);

impl ReplyOutput for FakeOutput {
    fn play(&mut self, chunk: &PcmChunk) -> Result<(), String> {
        if self.0.refuse.load(Ordering::SeqCst) {
            return Err("the device refused".to_owned());
        }
        self.0.chunks.fetch_add(1, Ordering::SeqCst);
        self.0
            .samples
            .fetch_add(chunk.samples.len(), Ordering::SeqCst);
        self.0.drained.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn end_stream(&mut self) {
        self.0.ends.fetch_add(1, Ordering::SeqCst);
    }

    fn flush(&mut self) {
        self.0.flushes.fetch_add(1, Ordering::SeqCst);
        self.0.drained.store(true, Ordering::SeqCst);
    }

    fn is_drained(&self) -> bool {
        self.0.drained.load(Ordering::SeqCst)
    }
}

/// A gate the test closes and opens.
#[derive(Default)]
pub struct GateProbe {
    pub closed: AtomicBool,
    pub reopened: AtomicBool,
    pub activity: AtomicUsize,
    pub ended: AtomicUsize,
    pub flushed: AtomicUsize,
}

pub struct TestGate(pub Arc<GateProbe>);

impl MicGate for TestGate {
    fn reply_activity(&mut self, _now: MonoTime) {
        self.0.activity.fetch_add(1, Ordering::SeqCst);
    }
    fn reply_ended(&mut self, _now: MonoTime) {
        self.0.ended.fetch_add(1, Ordering::SeqCst);
    }
    fn flushed(&mut self, _now: MonoTime) {
        self.0.flushed.fetch_add(1, Ordering::SeqCst);
    }
    fn update(&mut self, _drained: bool, _now: MonoTime) {}
    fn poll(&mut self, _now: MonoTime) -> MicState {
        if self.0.closed.load(Ordering::SeqCst) {
            MicState::Closed
        } else {
            MicState::Open {
                just_reopened: self.0.reopened.swap(false, Ordering::SeqCst),
            }
        }
    }
}

/// Everything a test needs to run a session and look at it.
pub struct Rig {
    pub dialogue: FakeDialogue,
    pub tts: FakeSpeechSynth,
    pub trace: FakeTraceSink,
    pub out: Arc<OutputProbe>,
    pub gate: Arc<GateProbe>,
    pub feed: MicFeed,
    pub handle: Option<AssistantHandle>,
    pub seen: Vec<AssistantEvent>,
    deps: Option<AssistantDeps>,
    params: AssistantParams,
}

/// How a rig is put together.
pub struct RigConfig {
    pub lang: Lang,
    pub vad: VadScript,
    pub asr: AsrScript,
    pub dialogue: Option<FakeDialogue>,
    pub tts: Option<FakeSpeechSynth>,
    pub use_test_gate: bool,
}

impl RigConfig {
    /// One utterance recognised as `text`, answered by `reply` tokens, spoken in two chunks per clause.
    pub fn new(text: &str, reply: impl IntoIterator<Item = &'static str>) -> (Self, ManualClock) {
        let clock = ManualClock::new();
        let dialogue =
            FakeDialogue::new(clock.clone()).with_default_script(DialogueScript::tokens(reply));
        let tts =
            FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(2, 480));
        (
            Self {
                lang: Lang::En,
                vad: vad_runs(&ONE_UTTERANCE),
                asr: AsrScript::new(["wha", "what time"], text).then_round([], ""),
                dialogue: Some(dialogue),
                tts: Some(tts),
                use_test_gate: false,
            },
            clock,
        )
    }
}

impl Rig {
    /// A rig that is built but not started.
    pub fn build(cfg: RigConfig, clock: ManualClock) -> Self {
        let (source, feed) = mic();
        Self::build_with(cfg, clock, source, feed)
    }

    pub fn build_with(
        cfg: RigConfig,
        clock: ManualClock,
        source: ChannelSource,
        feed: MicFeed,
    ) -> Self {
        let dialogue = cfg
            .dialogue
            .unwrap_or_else(|| FakeDialogue::new(clock.clone()));
        let tts = cfg
            .tts
            .unwrap_or_else(|| FakeSpeechSynth::new(clock.clone()));
        let trace = FakeTraceSink::new(false);
        let out = Arc::new(OutputProbe {
            drained: AtomicBool::new(true),
            ..OutputProbe::default()
        });
        let gate = Arc::new(GateProbe::default());
        let vad = FakeVadProvider::new(cfg.vad);
        let asr = FakeStreamingAsr::new(clock.clone(), cfg.asr);
        let params = AssistantParams::new(
            ConversationId::new("session-1"),
            cfg.lang,
            VoiceSelection::Preset {
                name: "preset-a".to_owned(),
            },
            ms(600),
        );
        let deps = AssistantDeps {
            source: Box::new(source),
            vad: Arc::new(vad.clone()),
            asr: Arc::new(asr),
            dialogue: Arc::new(dialogue.clone()),
            tts: Arc::new(tts.clone()),
            output: Box::new(FakeOutput(Arc::clone(&out))),
            gate: if cfg.use_test_gate {
                Box::new(TestGate(Arc::clone(&gate)))
            } else {
                Box::new(OpenGate)
            },
            clock: Arc::new(clock.clone()),
            trace: Arc::new(trace.clone()),
        };
        Self {
            dialogue,
            tts,
            trace,
            out,
            gate,
            feed,
            handle: None,
            seen: Vec::new(),
            deps: Some(deps),
            params,
        }
    }

    /// The usual rig, started.
    pub fn started(text: &str, reply: impl IntoIterator<Item = &'static str>) -> Self {
        let (cfg, clock) = RigConfig::new(text, reply);
        let mut rig = Self::build(cfg, clock);
        rig.start().expect("the session starts");
        rig
    }

    pub fn take_deps(&mut self) -> AssistantDeps {
        self.deps.take().expect("not started")
    }

    pub fn params(&self) -> AssistantParams {
        self.params.clone()
    }

    pub fn start(&mut self) -> Result<(), StartError> {
        let deps = self.deps.take().expect("started once");
        self.handle = Some(start(deps, self.params.clone())?);
        Ok(())
    }

    pub fn handle(&self) -> &AssistantHandle {
        self.handle.as_ref().expect("started")
    }

    /// Read events until `pred` holds for one of them (it is returned); panics at the watchdog.
    pub fn wait_for(
        &mut self,
        what: &str,
        pred: impl Fn(&AssistantEvent) -> bool,
    ) -> AssistantEvent {
        let deadline = Instant::now() + WATCHDOG;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "timed out waiting for: {what}; events so far:\n{:#?}",
                self.seen
            );
            if let Some(e) = self.handle().next_event(left.min(ms(50))) {
                self.seen.push(e.clone());
                if pred(&e) {
                    return e;
                }
            }
        }
    }

    /// Keep reading events until `cond` holds (for something no event announces).
    pub fn wait_until(&mut self, what: &str, cond: impl Fn(&Rig) -> bool) {
        let deadline = Instant::now() + WATCHDOG;
        while !cond(self) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for: {what}; events so far:\n{:#?}",
                self.seen
            );
            if let Some(e) = self.handle().next_event(ms(2)) {
                self.seen.push(e);
            }
        }
    }

    pub fn wait_phase(&mut self, phase: Phase) {
        self.wait_for(&format!("phase {phase:?}"), |e| {
            *e == AssistantEvent::Phase(phase)
        });
    }

    /// Stop the session and read to its end.
    pub fn stop(&mut self) {
        self.handle().stop();
        self.wait_for("the end of the session", |e| *e == AssistantEvent::Ended);
    }

    /// The phases seen so far, in order.
    pub fn phases(&self) -> Vec<Phase> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                AssistantEvent::Phase(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    pub fn user_turns(&self) -> Vec<String> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                AssistantEvent::UserTurn { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn reply_text(&self) -> String {
        self.seen
            .iter()
            .filter_map(|e| match e {
                AssistantEvent::ReplyDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn faults(&self) -> Vec<super::super::AssistantFault> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                AssistantEvent::Fault(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }

    /// Feed one utterance's worth of frames (the detector's script decides what they are).
    pub fn speak(&mut self) {
        self.feed.push(ONE_UTTERANCE.iter().map(|(_, n)| n).sum());
    }
}
