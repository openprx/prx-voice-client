//! Tests of the pump: ordering and latency through the real mixer, back-pressure, cancellation, failure and start-up errors.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_media::{AudioFrame, ManualClock, MonoTime, SampleRate};
use prxvc_ports::fakes::{FakeAudioSink, FakeAudioSource};
use prxvc_ports::{
    AudioSink, AudioSource, EndpointError, EndpointHealth, SinkInfo, SourceInfo, WriteOutcome,
};

use super::*;
use crate::mix::{MixMode, MixerConfig};

const RATE: SampleRate = SampleRate::HZ_48000;
const FRAME: usize = 480; // 10 ms

fn frame(seq: u64, samples: Vec<f32>) -> AudioFrame {
    let ts = MonoTime::from_millis(seq * 10);
    AudioFrame::new(samples, RATE, ts, seq).unwrap()
}

/// `n` frames of 10 ms carrying a rising ramp from 0 to 0.5.
fn ramp_frames(n: usize) -> Vec<AudioFrame> {
    let total = (n * FRAME) as f32;
    (0..n)
        .map(|f| {
            let samples = (0..FRAME)
                .map(|i| 0.5 * ((f * FRAME + i) as f32) / total)
                .collect();
            frame(f as u64, samples)
        })
        .collect()
}

fn mixer(cfg: MixerConfig) -> Box<Mixer> {
    Box::new(Mixer::new(MixerConfig {
        mode: MixMode::Original,
        ..cfg
    }))
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

type Collected = Arc<Mutex<Vec<(Instant, f32)>>>;

/// A sink that keeps every sample and drains in real time, like a device.
struct CollectSink {
    started: Instant,
    written: Duration,
    out: Collected,
    stopped: Arc<AtomicBool>,
}

impl CollectSink {
    fn new() -> (Self, Collected, Arc<AtomicBool>) {
        let out = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        (
            Self {
                started: Instant::now(),
                written: Duration::ZERO,
                out: Arc::clone(&out),
                stopped: Arc::clone(&stopped),
            },
            out,
            stopped,
        )
    }
}

impl AudioSink for CollectSink {
    fn start(&mut self) -> Result<SinkInfo, EndpointError> {
        self.started = Instant::now();
        Ok(SinkInfo {
            rate: RATE,
            device: "collect".into(),
        })
    }

    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
        let now = Instant::now();
        let mut out = self.out.lock();
        out.extend(frame.samples().iter().map(|s| (now, *s)));
        self.written += frame.duration();
        WriteOutcome {
            accepted: frame.len(),
            dropped: 0,
        }
    }

    fn flush(&mut self) {}

    fn queued(&self) -> Duration {
        self.written.saturating_sub(self.started.elapsed())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: true,
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

/// A source with flags for start and stop and a scripted read.
struct FlagSource {
    started: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    reads: Arc<AtomicU64>,
    read_takes: Duration,
    fail_start: bool,
}

impl FlagSource {
    fn idle(read_takes: Duration) -> Self {
        Self {
            started: Arc::default(),
            stopped: Arc::default(),
            reads: Arc::default(),
            read_takes,
            fail_start: false,
        }
    }
}

impl AudioSource for FlagSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        if self.fail_start {
            return Err(EndpointError::Unavailable {
                reason: "no device".into(),
            });
        }
        self.started.store(true, Ordering::SeqCst);
        Ok(SourceInfo {
            rate: RATE,
            device: "flag".into(),
        })
    }

    fn read(
        &mut self,
        _out: &mut Vec<AudioFrame>,
        _max_wait: Duration,
    ) -> Result<(), EndpointError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.read_takes);
        Ok(())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth::default()
    }

    fn stop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

fn spec(name: &str, source: Box<dyn AudioSource>, route: PumpRoute) -> PumpSpec {
    PumpSpec {
        config: PumpConfig::new(name),
        source,
        route,
        events: EventQueue::new(),
        echo: None,
    }
}

#[test]
fn frames_flow_through_the_mixer_in_order_with_bounded_latency() {
    let (sink, out, _stopped) = CollectSink::new();
    let source = FakeAudioSource::new(RATE, ramp_frames(30)).with_frames_per_read(30);
    let begin = Instant::now();
    let mut pump = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    ))
    .expect("start");

    // 300 ms of input plays out in about 300 ms; wait for the end of the ramp.
    wait_until("the ramp to reach the sink", || {
        out.lock().iter().any(|(_, s)| *s > 0.45)
    });
    pump.stop(Duration::from_secs(1)).expect("stop");

    let out = out.lock();
    let first = out
        .iter()
        .position(|(_, s)| s.abs() > 1e-4)
        .expect("audio came out");
    let latency = out[first].0.duration_since(begin);
    assert!(latency < Duration::from_millis(250), "latency {latency:?}");

    // After the fade-in the ramp must be non-decreasing: nothing reordered, repeated or lost.
    let body: Vec<f32> = out[first + 2 * FRAME..].iter().map(|(_, s)| *s).collect();
    let peak_at = body.iter().position(|s| *s > 0.45).expect("the ramp rose");
    for pair in body[..peak_at].windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-6,
            "samples went backwards: {pair:?}"
        );
    }
    let stats = pump.stats();
    assert_eq!(stats.frames, 30);
    assert_eq!(stats.samples, 30 * FRAME as u64);
    assert!(stats.level.peak > 0.3, "level {:?}", stats.level);
    assert_eq!(stats.mode, Some(MixMode::Original));
    assert_eq!(stats.source_device, "fake-source");
    assert!(!stats.running);
}

#[test]
fn a_slow_sink_applies_back_pressure_and_never_blocks_the_pump() {
    let clock = ManualClock::new();
    let sink = FakeAudioSink::new(RATE, clock.clone()).with_capacity(Duration::from_millis(20));
    let probe = sink.probe();
    // 2 s of input into a sink that holds 20 ms (less than the 40 ms the pump
    // tries to keep queued) and a lane that holds 200 ms.
    let source = FakeAudioSource::new(RATE, ramp_frames(200)).with_frames_per_read(200);
    let cfg = MixerConfig {
        max_buffer: Duration::from_millis(200),
        ..MixerConfig::default()
    };
    let mut pump = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(cfg),
            sink: Box::new(sink),
        },
    ))
    .expect("start");

    wait_until("the sink to fill and refuse samples", || {
        let s = pump.stats();
        s.sink_dropped_samples > 0 && s.frames == 200
    });
    let stats = pump.stats();
    assert!(stats.running, "a full sink must not end the pump");
    assert!(
        stats.sink_queued <= Duration::from_millis(20),
        "queued {:?}",
        stats.sink_queued
    );
    assert!(
        stats.lane_dropped_samples > 0,
        "the lane must drop what cannot be played"
    );

    // The device starts draining: writes are accepted again.
    let accepted_before = probe.written().iter().filter(|w| w.accepted > 0).count();
    clock.advance(Duration::from_millis(500));
    wait_until("writes after the sink drained", || {
        probe.written().iter().filter(|w| w.accepted > 0).count() > accepted_before
    });
    let begin = Instant::now();
    pump.stop(Duration::from_secs(1)).expect("stop");
    assert!(begin.elapsed() < Duration::from_millis(500));
}

#[test]
fn cancellation_stops_both_endpoints_promptly() {
    let (sink, _out, sink_stopped) = CollectSink::new();
    let source = FlagSource::idle(Duration::from_millis(2));
    let (started, stopped, reads) = (
        Arc::clone(&source.started),
        Arc::clone(&source.stopped),
        Arc::clone(&source.reads),
    );
    let mut pump = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    ))
    .expect("start");
    assert!(started.load(Ordering::SeqCst));
    wait_until("the loop to run", || reads.load(Ordering::SeqCst) > 3);

    let begin = Instant::now();
    pump.stop(Duration::from_millis(500)).expect("stop");
    assert!(
        begin.elapsed() < Duration::from_millis(200),
        "took {:?}",
        begin.elapsed()
    );
    assert!(stopped.load(Ordering::SeqCst), "source stopped");
    assert!(sink_stopped.load(Ordering::SeqCst), "sink stopped");
    assert!(!pump.stats().running);
    pump.stop(Duration::from_millis(10))
        .expect("a second stop is harmless");
}

#[test]
fn a_source_that_ignores_the_wait_is_reported_as_a_stop_timeout() {
    let source = FlagSource::idle(Duration::from_millis(600));
    let mut pump =
        Pump::start(spec("incoming", Box::new(source), PumpRoute::MeterOnly)).expect("start");
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(pump.stop(Duration::from_millis(50)), Err(PumpStopTimeout));
}

#[test]
fn a_failing_source_ends_the_pump_and_reports_the_loss() {
    let (sink, _out, sink_stopped) = CollectSink::new();
    let source = FakeAudioSource::new(RATE, ramp_frames(10)).failing_read_at(
        3,
        EndpointError::DeviceLost {
            device: "usb mic".into(),
        },
    );
    let pump = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    ))
    .expect("start");
    wait_until("the pump to end", || !pump.stats().running);
    assert!(pump.stats().failed);
    let events = pump.take_events();
    assert!(
        matches!(events.as_slice(), [PumpEvent::SourceLost { reason }] if reason.contains("usb mic")),
        "{events:?}"
    );
    assert!(
        sink_stopped.load(Ordering::SeqCst),
        "the sink is released too"
    );
    assert!(pump.take_events().is_empty(), "events are taken once");
}

#[test]
fn meter_only_reads_and_measures_without_a_sink() {
    let source = FakeAudioSource::new(RATE, ramp_frames(20)).with_frames_per_read(20);
    let mut pump =
        Pump::start(spec("incoming", Box::new(source), PumpRoute::MeterOnly)).expect("start");
    wait_until("the frames to be read", || pump.stats().frames == 20);
    let stats = pump.stats();
    assert_eq!(stats.sink_device, None);
    assert_eq!(stats.mode, None);
    assert!(stats.level.peak > 0.0);
    assert_eq!(stats.last_frame_ts, Some(MonoTime::from_millis(190)));
    pump.set_mode(MixMode::Both); // no mixer: ignored
    assert_eq!(pump.stats().mode, None);
    pump.stop(Duration::from_secs(1)).expect("stop");
}

#[test]
fn a_sink_that_does_not_start_leaves_the_source_unstarted() {
    let clock = ManualClock::new();
    let sink = FakeAudioSink::new(RATE, clock).failing_start(EndpointError::Unavailable {
        reason: "driver missing".into(),
    });
    let source = FlagSource::idle(Duration::from_millis(1));
    let started = Arc::clone(&source.started);
    let err = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    ))
    .err()
    .expect("must fail");
    assert_eq!(err.stage, PumpStage::Sink);
    assert!(!started.load(Ordering::SeqCst));
}

#[test]
fn a_source_that_does_not_start_releases_the_sink() {
    let (sink, _out, sink_stopped) = CollectSink::new();
    let mut source = FlagSource::idle(Duration::from_millis(1));
    source.fail_start = true;
    let err = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    ))
    .err()
    .expect("must fail");
    assert_eq!(err.stage, PumpStage::Source);
    assert!(sink_stopped.load(Ordering::SeqCst));
}

#[test]
fn the_event_queue_is_bounded_and_keeps_the_newest() {
    let q = EventQueue::new();
    for i in 0..(EVENT_CAPACITY as u64 + 5) {
        q.push(PumpEvent::Stalled { idle_ms: i });
    }
    let events = q.drain();
    assert_eq!(events.len(), EVENT_CAPACITY);
    assert_eq!(events[0], PumpEvent::Stalled { idle_ms: 5 });
}

#[test]
fn a_connected_pump_taps_its_frames_plays_translated_speech_and_reports_the_playback() {
    use prxvc_media::{ClauseId, Direction, PcmChunk, SegmentId, SystemClock};

    let (sink, out, _stopped) = CollectSink::new();
    // The source keeps delivering 10 ms frames for the whole test.
    let source = FakeAudioSource::new(RATE, ramp_frames(200)).with_frames_per_read(1);
    let pump = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: Box::new(Mixer::new(MixerConfig {
                mode: MixMode::Translation,
                jitter_buffer: Duration::from_millis(20),
                ..MixerConfig::default()
            })),
            sink: Box::new(sink),
        },
    ))
    .expect("start");

    let tapped: Arc<Mutex<Vec<AudioFrame>>> = Arc::default();
    let signals: Arc<Mutex<Vec<PlaybackSignal>>> = Arc::default();
    let feed = pump
        .connect(PumpLink {
            tap: FrameTap {
                rate: SampleRate::HZ_16000,
                sink: {
                    let tapped = Arc::clone(&tapped);
                    Box::new(move |f| tapped.lock().push(f))
                },
            },
            on_playback: {
                let signals = Arc::clone(&signals);
                Box::new(move |s| signals.lock().push(s))
            },
            clock: Arc::new(SystemClock::new()),
        })
        .expect("a pump with a mixer connects");

    wait_until("frames at the pipeline's rate", || tapped.lock().len() >= 3);
    assert!(
        tapped
            .lock()
            .iter()
            .all(|f| f.rate() == SampleRate::HZ_16000)
    );

    // 100 ms of translated speech at 24 kHz, a clause.
    let clause = ClauseId::new(SegmentId::first(Direction::Outgoing), 0);
    feed.push(
        clause,
        &PcmChunk {
            samples: vec![0.4; 2400],
            rate: SampleRate::HZ_24000,
        },
    );
    feed.end_clause(clause);
    wait_until("the clause to finish playing", || {
        signals
            .lock()
            .iter()
            .any(|s| matches!(s, PlaybackSignal::Finished { .. }))
    });
    let signals = signals.lock().clone();
    assert!(matches!(signals[0], PlaybackSignal::Started { clause: c, .. } if c == clause));
    assert!(matches!(signals[1], PlaybackSignal::Finished { clause: c, .. } if c == clause));
    assert!(
        out.lock().iter().any(|(_, s)| (*s - 0.4).abs() < 0.05),
        "the translated speech reached the sink"
    );
    assert_eq!(pump.stats().translated_underruns, 0);

    pump.disconnect();
    feed.push(
        clause,
        &PcmChunk {
            samples: vec![0.4; 2400],
            rate: SampleRate::HZ_24000,
        },
    );
    let mut pump = pump;
    pump.stop(Duration::from_secs(1)).expect("stop");
}

#[test]
fn a_pump_without_a_mixer_cannot_connect() {
    let source = FakeAudioSource::new(RATE, ramp_frames(5));
    let pump = Pump::start(spec("incoming", Box::new(source), PumpRoute::MeterOnly)).expect("start");
    let linked = pump.connect(PumpLink {
        tap: FrameTap {
            rate: SampleRate::HZ_16000,
            sink: Box::new(|_| {}),
        },
        on_playback: Box::new(|_| {}),
        clock: Arc::new(prxvc_media::SystemClock::new()),
    });
    assert!(linked.is_none());
}

#[test]
fn a_closed_echo_gate_keeps_the_microphone_from_the_mixer_and_the_pipeline() {
    use crate::echo_gate::{EchoGate, EchoGateHandle};
    use prxvc_media::SystemClock;

    let gate = EchoGateHandle::new(EchoGate::new(true, Duration::from_millis(50)));
    gate.with(|g| g.reply_activity(Instant::now())); // closed until the reply ends
    let (sink, out, _stopped) = CollectSink::new();
    let source = FakeAudioSource::new(RATE, ramp_frames(40)).with_frames_per_read(1);
    let mut spec = spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    );
    spec.echo = Some(EchoRole::Input(gate.clone()));
    let mut pump = Pump::start(spec).expect("start");
    let tapped = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&tapped);
    let _feed = pump
        .connect(PumpLink {
            tap: FrameTap {
                rate: SampleRate::HZ_16000,
                sink: Box::new(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
            },
            on_playback: Box::new(|_| {}),
            clock: Arc::new(SystemClock::new()),
        })
        .expect("connect");
    wait_until("the frames to be read", || pump.stats().frames >= 20);
    let stats = pump.stats();
    assert!(stats.gated, "the gate is closed");
    assert!(stats.gated_frames >= 20, "{stats:?}");
    assert_eq!(tapped.load(Ordering::SeqCst), 0, "nothing reaches the pipeline");
    assert!(
        out.lock().iter().all(|(_, s)| s.abs() < 1e-4),
        "nothing of the microphone reaches the sink while the gate is closed"
    );
    gate.with(|g| g.reply_ended(Instant::now()));
    pump.stop(Duration::from_secs(1)).expect("stop");
}

#[test]
fn a_playing_pump_closes_the_gate_while_its_output_is_audible_and_reopens_it_after_the_tail() {
    use crate::echo_gate::{EchoGate, EchoGateHandle};

    let gate = EchoGateHandle::new(EchoGate::new(true, Duration::from_millis(60)));
    let (sink, _out, _stopped) = CollectSink::new();
    // 300 ms of a loud tone.
    let frames: Vec<AudioFrame> = (0..30)
        .map(|i| frame(i, vec![0.5; FRAME]))
        .collect();
    let source = FakeAudioSource::new(RATE, frames).with_frames_per_read(1);
    let mut spec = spec(
        "incoming",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    );
    spec.echo = Some(EchoRole::Playback(gate.clone()));
    let mut pump = Pump::start(spec).expect("start");
    wait_until("the gate to close while the output is audible", || {
        gate.is_closed(Instant::now())
    });
    pump.stop(Duration::from_secs(1)).expect("stop");
}

/// Records the order in which the endpoints of a pump are stopped.
struct OrderSource {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl AudioSource for OrderSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        Ok(SourceInfo {
            rate: RATE,
            device: "order-source".into(),
        })
    }

    fn read(&mut self, _out: &mut Vec<AudioFrame>, wait: Duration) -> Result<(), EndpointError> {
        std::thread::sleep(wait.min(Duration::from_millis(2)));
        Ok(())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth::default()
    }

    fn stop(&mut self) {
        self.log.lock().push("source");
    }
}

struct OrderSink {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl AudioSink for OrderSink {
    fn start(&mut self) -> Result<SinkInfo, EndpointError> {
        Ok(SinkInfo {
            rate: RATE,
            device: "order-sink".into(),
        })
    }

    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
        WriteOutcome {
            accepted: frame.len(),
            dropped: 0,
        }
    }

    fn flush(&mut self) {}

    fn queued(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth::default()
    }

    fn stop(&mut self) {
        self.log.lock().push("sink");
    }
}

fn stop_order(source_first: bool) -> Vec<&'static str> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut spec = spec(
        "incoming",
        Box::new(OrderSource {
            log: Arc::clone(&log),
        }),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(OrderSink {
                log: Arc::clone(&log),
            }),
        },
    );
    spec.config.stop_source_first = source_first;
    let mut pump = Pump::start(spec).expect("start");
    pump.stop(Duration::from_secs(1)).expect("stop");
    log.lock().clone()
}

#[test]
fn a_path_ends_its_sink_first_unless_its_source_must_give_the_original_back_first() {
    assert_eq!(stop_order(false), ["sink", "source"]);
    assert_eq!(stop_order(true), ["source", "sink"]);
}

#[test]
fn the_mixer_reports_the_level_of_what_it_outputs() {
    let mut m = Mixer::new(MixerConfig {
        mode: MixMode::Translation,
        jitter_buffer: Duration::from_millis(20),
        ..MixerConfig::default()
    });
    let mut out = vec![0.0; 4800];
    m.render(&mut out);
    assert_eq!(m.stats().output_level.peak, 0.0);
    let chunk = AudioFrame::new(vec![0.5; 48_000], RATE, MonoTime::ZERO, 0).unwrap();
    m.push_translated(&chunk).unwrap();
    for _ in 0..6 {
        m.render(&mut out);
    }
    let stats = m.stats();
    assert!(stats.output_level.peak > 0.45, "{:?}", stats.output_level);
    assert!(stats.translated_level.peak > 0.45);
}

#[test]
fn a_panic_in_a_lent_closure_fails_the_pump_and_still_releases_both_endpoints() {
    use prxvc_media::SystemClock;

    let (sink, _out, sink_stopped) = CollectSink::new();
    let source = FakeAudioSource::new(RATE, ramp_frames(50)).with_frames_per_read(1);
    let mut pump = Pump::start(spec(
        "outgoing",
        Box::new(source),
        PumpRoute::Mix {
            mixer: mixer(MixerConfig::default()),
            sink: Box::new(sink),
        },
    ))
    .expect("start");
    let _feed = pump
        .connect(PumpLink {
            tap: FrameTap {
                rate: SampleRate::HZ_16000,
                sink: Box::new(|_| panic!("a closure of the pipeline panicked")),
            },
            on_playback: Box::new(|_| {}),
            clock: Arc::new(SystemClock::new()),
        })
        .expect("connect");
    wait_until("the pump to end", || !pump.stats().running);
    assert!(pump.stats().failed);
    assert!(sink_stopped.load(Ordering::SeqCst), "the sink is released");
    let events = pump.take_events();
    assert!(
        matches!(events.as_slice(), [PumpEvent::SourceLost { reason }] if reason.contains("panicked")),
        "{events:?}"
    );
    // The thread ended normally: joining it reports no panic and stopping is harmless.
    assert!(pump.stop(Duration::from_secs(2)).is_ok());
}
