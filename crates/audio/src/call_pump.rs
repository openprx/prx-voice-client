//! Audio pump of one call path: a worker thread that reads an `AudioSource`, meters it and, when a route is given, mixes it into an `AudioSink`.
//!
//! One pump serves one path of a call:
//!
//! * outgoing: physical microphone source, mixer (original lane only), the
//!   virtual microphone sink ([`PumpRoute::Mix`]);
//! * incoming: the call application's tap source with nothing played by us
//!   ([`PumpRoute::MeterOnly`]); the same pump with a mixer and the default
//!   output sink would play the tap ([`PumpRoute::Mix`]).
//!
//! # Threads and queues
//!
//! The pump thread is an ordinary (non-real-time) thread. The device callbacks
//! that feed the source and drain the sink run elsewhere and talk to it through
//! the endpoints' own rings (code standard CS-4.1.1/CS-4.1.2). Inside the
//! pump the only queues are the mixer's lanes, bounded by `max_buffer`: when the
//! sink is slow the sink refuses samples, the mixer stops feeding, the lane
//! fills up and drops the excess, and every drop is counted in [`PumpStats`].
//! The loop never waits on the sink, so a slow sink never delays reading the
//! source or cancellation.
//!
//! # Cancellation
//!
//! [`Pump::stop`] sets a flag the loop checks at least once per
//! [`PumpConfig::read_wait`]; the thread then stops both endpoints and ends.
//! `stop` joins with a bound and reports a thread that did not end in time.
//!
//! # Signal
//!
//! While a mixer route runs, the sink is fed on every loop, so the virtual
//! microphone always carries a defined signal: the original voice, or silence
//! (code `PD-4.2.4`).

mod level;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_media::MonoTime;
use prxvc_ports::{AudioSink, AudioSource, EndpointError};

pub use level::{Level, LevelMeter, SharedLevel};

use crate::mix::{MixMode, Mixer};

/// Events kept for the owner; the oldest is dropped when a slow owner lets more pile up.
const EVENT_CAPACITY: usize = 64;

/// Shortest time a loop with nothing to read takes, so that a source whose
/// `read` returns at once (a script, a closed source) cannot make the thread spin.
const MIN_IDLE: Duration = Duration::from_millis(1);

/// Something that happened to a path that its owner must know about.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PumpEvent {
    /// The source failed while reading; the pump has stopped.
    SourceLost {
        /// Why.
        reason: String,
    },
    /// The mixer refused a frame (conversion failure); counted, the pump goes on.
    MixFailed {
        /// Why.
        reason: String,
    },
    /// The microphone in use changed (the input policy moved capture).
    DeviceChanged {
        /// The device now in use.
        device: String,
    },
    /// No microphone can be captured now; capture resumes when one appears.
    DeviceUnavailable {
        /// Why.
        message: String,
    },
    /// The tapped application is playing but the tap delivers only zeros.
    NoSignal {
        /// Text for the log and the debug view.
        detail: String,
    },
    /// Real audio arrived again after [`PumpEvent::NoSignal`].
    SignalRestored,
    /// The tapped application is no longer running.
    TargetGone,
    /// The tap runs again after the application came back.
    TargetBack,
    /// The tap stopped delivering buffers.
    Stalled {
        /// Milliseconds without a buffer.
        idle_ms: u64,
    },
    /// Capture of the call application is not permitted (consent revoked or missing).
    CaptureDenied {
        /// Text for the log and the debug view.
        detail: String,
    },
    /// Capture of the call application could not be rebuilt for another reason.
    CaptureFailed {
        /// Text for the log and the debug view.
        detail: String,
    },
}

/// A bounded queue of [`PumpEvent`]s shared by the pump, its source wrapper and the owner.
#[derive(Clone, Debug, Default)]
pub struct EventQueue(Arc<Mutex<VecDeque<PumpEvent>>>);

impl EventQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event; the oldest goes when the queue is full.
    pub fn push(&self, event: PumpEvent) {
        let mut q = self.0.lock();
        if q.len() >= EVENT_CAPACITY {
            q.pop_front();
        }
        q.push_back(event);
    }

    /// Take every queued event, oldest first.
    pub fn drain(&self) -> Vec<PumpEvent> {
        self.0.lock().drain(..).collect()
    }
}

/// What the pump does with the audio it reads.
pub enum PumpRoute {
    /// Mix it (original lane) into `sink`.
    Mix {
        /// The mixer of this path.
        mixer: Box<Mixer>,
        /// The endpoint that plays the mix.
        sink: Box<dyn AudioSink>,
    },
    /// Read and meter only; nothing is played.
    MeterOnly,
}

/// Settings of a pump.
#[derive(Clone, Debug)]
pub struct PumpConfig {
    /// Names the thread and the log lines (`outgoing`, `incoming`).
    pub name: String,
    /// How much audio the sink is kept supplied with.
    pub ahead: Duration,
    /// Longest a loop waits for the source, and so the longest `stop` waits for the thread.
    pub read_wait: Duration,
}

impl PumpConfig {
    /// Defaults: the sink is kept 40 ms ahead and the loop waits at most 10 ms.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ahead: Duration::from_millis(40),
            read_wait: Duration::from_millis(10),
        }
    }
}

/// Everything a pump is started with.
pub struct PumpSpec {
    /// Settings.
    pub config: PumpConfig,
    /// The source, not yet started.
    pub source: Box<dyn AudioSource>,
    /// What to do with the audio.
    pub route: PumpRoute,
    /// Where the pump (and the source wrapper, if it shares the queue) reports events.
    pub events: EventQueue,
}

/// Which endpoint failed to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpStage {
    /// The sink (opened first so that the device is never left without a feeder).
    Sink,
    /// The source.
    Source,
}

/// A pump could not be started; nothing is left running.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the {stage:?} of the {name} pump did not start: {error}")]
pub struct PumpStartError {
    /// The path.
    pub name: String,
    /// The endpoint.
    pub stage: PumpStage,
    /// Its error.
    pub error: EndpointError,
}

/// The pump thread did not end within the bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the pump thread did not end within the time allowed")]
pub struct PumpStopTimeout;

/// A reading of the pump's counters.
#[derive(Clone, Debug, PartialEq)]
pub struct PumpStats {
    /// The thread is running.
    pub running: bool,
    /// The source failed and the pump ended by itself.
    pub failed: bool,
    /// The device the source reads from.
    pub source_device: String,
    /// The device the sink plays to, when there is a sink.
    pub sink_device: Option<String>,
    /// Frames read.
    pub frames: u64,
    /// Samples read.
    pub samples: u64,
    /// Level of the last full window of audio.
    pub level: Level,
    /// Capture stamp of the last frame read.
    pub last_frame_ts: Option<MonoTime>,
    /// Samples the source lost to overrun.
    pub source_dropped_samples: u64,
    /// Samples the sink refused because its queue was full.
    pub sink_dropped_samples: u64,
    /// Times the sink ran dry.
    pub sink_underruns: u64,
    /// Times the mixer's original lane ran dry in a stream.
    pub lane_underruns: u64,
    /// Samples the mixer dropped because its lane was full.
    pub lane_dropped_samples: u64,
    /// Audio queued in the sink.
    pub sink_queued: Duration,
    /// The mixer's mode; `None` without a mixer.
    pub mode: Option<MixMode>,
}

const NO_TS: u64 = u64::MAX;

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    running: AtomicBool,
    failed: AtomicBool,
    mode: AtomicU8,
    frames: AtomicU64,
    samples: AtomicU64,
    last_ts: AtomicU64,
    source_dropped: AtomicU64,
    sink_dropped: AtomicU64,
    sink_underruns: AtomicU64,
    lane_underruns: AtomicU64,
    lane_dropped: AtomicU64,
    sink_queued_us: AtomicU64,
    level: SharedLevel,
}

fn mode_code(mode: MixMode) -> u8 {
    match mode {
        MixMode::Translation => 0,
        MixMode::Both => 1,
        MixMode::Original => 2,
    }
}

fn mode_of(code: u8) -> MixMode {
    match code {
        0 => MixMode::Translation,
        1 => MixMode::Both,
        _ => MixMode::Original,
    }
}

/// A running pump. Stops and joins on drop.
pub struct Pump {
    shared: Arc<Shared>,
    events: EventQueue,
    thread: Option<JoinHandle<()>>,
    source_device: String,
    sink_device: Option<String>,
    has_mixer: bool,
    name: String,
}

impl Pump {
    /// Start the sink (if any), then the source, then the pump thread.
    ///
    /// # Errors
    /// [`PumpStartError`] naming the endpoint that did not start; the other
    /// one, if it had started, is stopped again.
    pub fn start(spec: PumpSpec) -> Result<Self, PumpStartError> {
        let PumpSpec {
            config,
            mut source,
            mut route,
            events,
        } = spec;
        let fail = |stage, error| PumpStartError {
            name: config.name.clone(),
            stage,
            error,
        };
        let mut sink_device = None;
        if let PumpRoute::Mix { sink, .. } = &mut route {
            let info = sink.start().map_err(|e| fail(PumpStage::Sink, e))?;
            sink_device = Some(info.device);
        }
        let source_info = match source.start() {
            Ok(info) => info,
            Err(e) => {
                if let PumpRoute::Mix { sink, .. } = &mut route {
                    sink.stop();
                }
                return Err(fail(PumpStage::Source, e));
            }
        };
        let shared = Arc::new(Shared::default());
        shared.last_ts.store(NO_TS, Ordering::Relaxed);
        shared.running.store(true, Ordering::Release);
        let has_mixer = matches!(route, PumpRoute::Mix { .. });
        if let PumpRoute::Mix { mixer, .. } = &route {
            shared
                .mode
                .store(mode_code(mixer.mode()), Ordering::Relaxed);
        }
        let name = config.name.clone();
        let worker = Worker {
            source,
            route,
            shared: Arc::clone(&shared),
            events: events.clone(),
            config,
        };
        let spawned = std::thread::Builder::new()
            .name(format!("prxvc-pump-{name}"))
            .spawn(move || worker.run());
        let thread = match spawned {
            Ok(t) => t,
            Err(e) => {
                // The worker (and with it both endpoints) was dropped by the failed spawn.
                return Err(PumpStartError {
                    name,
                    stage: PumpStage::Source,
                    error: EndpointError::Internal(format!("could not start the pump thread: {e}")),
                });
            }
        };
        Ok(Self {
            shared,
            events,
            thread: Some(thread),
            source_device: source_info.device,
            sink_device,
            has_mixer,
            name,
        })
    }

    /// The path's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Switch what the mixer plays (a crossfade). Does nothing without a mixer.
    pub fn set_mode(&self, mode: MixMode) {
        if self.has_mixer {
            self.shared.mode.store(mode_code(mode), Ordering::Relaxed);
        }
    }

    /// The counters now.
    pub fn stats(&self) -> PumpStats {
        let s = &self.shared;
        let ts = s.last_ts.load(Ordering::Relaxed);
        PumpStats {
            running: s.running.load(Ordering::Acquire),
            failed: s.failed.load(Ordering::Acquire),
            source_device: self.source_device.clone(),
            sink_device: self.sink_device.clone(),
            frames: s.frames.load(Ordering::Relaxed),
            samples: s.samples.load(Ordering::Relaxed),
            level: s.level.load(),
            last_frame_ts: (ts != NO_TS).then(|| MonoTime::from_nanos(ts)),
            source_dropped_samples: s.source_dropped.load(Ordering::Relaxed),
            sink_dropped_samples: s.sink_dropped.load(Ordering::Relaxed),
            sink_underruns: s.sink_underruns.load(Ordering::Relaxed),
            lane_underruns: s.lane_underruns.load(Ordering::Relaxed),
            lane_dropped_samples: s.lane_dropped.load(Ordering::Relaxed),
            sink_queued: Duration::from_micros(s.sink_queued_us.load(Ordering::Relaxed)),
            mode: self
                .has_mixer
                .then(|| mode_of(s.mode.load(Ordering::Relaxed))),
        }
    }

    /// Take the events reported since the last call, oldest first.
    pub fn take_events(&self) -> Vec<PumpEvent> {
        self.events.drain()
    }

    /// Ask the thread to end and wait up to `timeout` for it. The thread stops
    /// both endpoints on its way out. Stopping twice is harmless.
    ///
    /// # Errors
    /// [`PumpStopTimeout`] when the thread is still running after `timeout`;
    /// it is then detached and ends by itself when its blocked call returns.
    pub fn stop(&mut self, timeout: Duration) -> Result<(), PumpStopTimeout> {
        self.shared.stop.store(true, Ordering::Release);
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let deadline = Instant::now() + timeout;
        while !thread.is_finished() {
            if Instant::now() >= deadline {
                tracing::error!(pump = %self.name, "pump thread did not end in time; detached");
                return Err(PumpStopTimeout);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if thread.join().is_err() {
            tracing::error!(pump = %self.name, "pump thread panicked");
        }
        Ok(())
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        // A timeout here is already logged by `stop`; a drop has nobody to tell.
        let _ = self.stop(Duration::from_secs(2));
    }
}

struct Worker {
    source: Box<dyn AudioSource>,
    route: PumpRoute,
    shared: Arc<Shared>,
    events: EventQueue,
    config: PumpConfig,
}

impl Worker {
    fn run(mut self) {
        let mut frames = Vec::new();
        let mut meter = LevelMeter::new();
        let mut mix_failures = 0u64;
        while !self.shared.stop.load(Ordering::Acquire) {
            let started = Instant::now();
            frames.clear();
            if let Err(e) = self.source.read(&mut frames, self.config.read_wait) {
                self.events.push(PumpEvent::SourceLost {
                    reason: e.to_string(),
                });
                self.shared.failed.store(true, Ordering::Release);
                break;
            }
            for frame in &frames {
                self.account(frame, &mut meter);
                if let PumpRoute::Mix { mixer, .. } = &mut self.route
                    && let Err(e) = mixer.push_original(frame)
                {
                    mix_failures += 1;
                    if mix_failures == 1 {
                        self.events.push(PumpEvent::MixFailed {
                            reason: e.to_string(),
                        });
                    }
                }
            }
            self.feed_sink();
            self.shared
                .source_dropped
                .store(self.source.health().dropped_samples, Ordering::Relaxed);
            if frames.is_empty() {
                let spent = started.elapsed();
                if spent < MIN_IDLE {
                    std::thread::sleep(MIN_IDLE - spent);
                }
            }
        }
        self.close();
    }

    fn account(&self, frame: &prxvc_media::AudioFrame, meter: &mut LevelMeter) {
        let s = &self.shared;
        s.frames.fetch_add(1, Ordering::Relaxed);
        s.samples.fetch_add(frame.len() as u64, Ordering::Relaxed);
        s.last_ts.store(frame.ts().as_nanos(), Ordering::Relaxed);
        if let Some(level) = meter.feed(frame.samples(), frame.rate().hz()) {
            s.level.store(level);
        }
    }

    /// Apply a commanded mode and keep the sink supplied; publish the sink's and the lane's counters.
    fn feed_sink(&mut self) {
        let PumpRoute::Mix { mixer, sink } = &mut self.route else {
            return;
        };
        let s = &self.shared;
        let wanted = mode_of(s.mode.load(Ordering::Relaxed));
        if mixer.mode() != wanted {
            mixer.set_mode(wanted);
        }
        if let Err(e) = mixer.feed(sink.as_mut(), self.config.ahead) {
            tracing::warn!(pump = %self.config.name, error = %e, "the mixer could not render");
        }
        let health = sink.health();
        s.sink_dropped
            .store(health.dropped_samples, Ordering::Relaxed);
        s.sink_underruns.store(health.underruns, Ordering::Relaxed);
        s.sink_queued_us.store(
            u64::try_from(sink.queued().as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        let lane = mixer.stats().original;
        s.lane_underruns.store(lane.underruns, Ordering::Relaxed);
        s.lane_dropped
            .store(lane.dropped_samples, Ordering::Relaxed);
    }

    /// Release the endpoints; the sink first, so that nothing plays after the source is gone.
    fn close(&mut self) {
        if let PumpRoute::Mix { sink, .. } = &mut self.route {
            sink.flush();
            sink.stop();
        }
        self.source.stop();
        self.shared.running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
