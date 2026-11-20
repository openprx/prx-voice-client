//! Fake audio endpoints: a source that plays back scripted frames and a sink that records what is written, with a queue that drains on the injected clock.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{AudioFrame, Clock, MonoTime, SampleRate};

use crate::endpoint::{
    AudioSink, AudioSource, EndpointError, EndpointHealth, SinkInfo, SourceInfo, WriteOutcome,
};

/// An [`AudioSource`] that delivers scripted frames.
///
/// `read` never waits: it hands over up to `frames_per_read` of the remaining
/// frames and returns, so a test controls pacing by how often it reads.
pub struct FakeAudioSource {
    rate: SampleRate,
    frames: VecDeque<AudioFrame>,
    frames_per_read: usize,
    fail_read_at: Option<(usize, EndpointError)>,
    start_error: Option<EndpointError>,
    reads: usize,
    delivered: u64,
    running: bool,
    last_error: Option<String>,
}

impl FakeAudioSource {
    /// A source that will deliver `frames`, one per `read`, at `rate`.
    pub fn new(rate: SampleRate, frames: Vec<AudioFrame>) -> Self {
        Self {
            rate,
            frames: frames.into(),
            frames_per_read: 1,
            fail_read_at: None,
            start_error: None,
            reads: 0,
            delivered: 0,
            running: false,
            last_error: None,
        }
    }

    /// Deliver up to `n` frames per `read`.
    #[must_use]
    pub fn with_frames_per_read(mut self, n: usize) -> Self {
        self.frames_per_read = n.max(1);
        self
    }

    /// Fail the `read` call with index `index` (counted from 0).
    #[must_use]
    pub fn failing_read_at(mut self, index: usize, error: EndpointError) -> Self {
        self.fail_read_at = Some((index, error));
        self
    }

    /// Fail `start`.
    #[must_use]
    pub fn failing_start(mut self, error: EndpointError) -> Self {
        self.start_error = Some(error);
        self
    }

    /// Frames not yet delivered.
    pub fn remaining(&self) -> usize {
        self.frames.len()
    }
}

impl AudioSource for FakeAudioSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        if let Some(e) = &self.start_error {
            self.last_error = Some(e.to_string());
            return Err(e.clone());
        }
        self.running = true;
        Ok(SourceInfo {
            rate: self.rate,
            device: "fake-source".to_owned(),
        })
    }

    fn read(
        &mut self,
        out: &mut Vec<AudioFrame>,
        _max_wait: Duration,
    ) -> Result<(), EndpointError> {
        if !self.running {
            return Err(EndpointError::NotStarted);
        }
        let index = self.reads;
        self.reads += 1;
        if let Some((at, error)) = &self.fail_read_at
            && *at == index
        {
            self.running = false;
            self.last_error = Some(error.to_string());
            return Err(error.clone());
        }
        for _ in 0..self.frames_per_read {
            let Some(frame) = self.frames.pop_front() else {
                break;
            };
            self.delivered += 1;
            out.push(frame);
        }
        Ok(())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: self.running,
            frames: self.delivered,
            last_error: self.last_error.clone(),
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

/// One `write` call as the sink saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenFrame {
    /// The injected clock's time of the write.
    pub written_at: MonoTime,
    /// The frame's own capture timestamp.
    pub frame_ts: MonoTime,
    /// The frame's sequence number.
    pub seq: u64,
    /// Samples accepted into the queue.
    pub accepted: usize,
    /// Samples refused because the queue was full.
    pub dropped: usize,
}

#[derive(Default)]
struct SinkState {
    running: bool,
    written: Vec<WrittenFrame>,
    queued: Duration,
    drained_to: MonoTime,
    flushes: Vec<MonoTime>,
    accepted_frames: u64,
    dropped_samples: u64,
    last_error: Option<String>,
}

impl SinkState {
    /// Playing consumes the queue in real (injected) time.
    fn drain(&mut self, now: MonoTime) {
        self.queued = self
            .queued
            .saturating_sub(now.saturating_since(self.drained_to));
        self.drained_to = now;
    }
}

/// An [`AudioSink`] that records writes. Its queue drains as the injected
/// clock advances, as a real device would, so `queued()` and the capacity
/// limit are deterministic.
pub struct FakeAudioSink {
    rate: SampleRate,
    clock: Arc<dyn Clock>,
    capacity: Option<Duration>,
    start_error: Option<EndpointError>,
    state: Arc<Mutex<SinkState>>,
}

/// The test's handle on a sink that a pipeline owns.
#[derive(Clone)]
pub struct SinkProbe {
    state: Arc<Mutex<SinkState>>,
}

impl FakeAudioSink {
    /// A sink at `rate`, timed on `clock`, with an unlimited queue.
    pub fn new(rate: SampleRate, clock: impl Clock + 'static) -> Self {
        Self {
            rate,
            clock: Arc::new(clock),
            capacity: None,
            start_error: None,
            state: Arc::new(Mutex::new(SinkState::default())),
        }
    }

    /// Refuse samples that would make the queue longer than `capacity`.
    #[must_use]
    pub fn with_capacity(mut self, capacity: Duration) -> Self {
        self.capacity = Some(capacity);
        self
    }

    /// Fail `start`.
    #[must_use]
    pub fn failing_start(mut self, error: EndpointError) -> Self {
        self.start_error = Some(error);
        self
    }

    /// A handle for the test to inspect the sink.
    pub fn probe(&self) -> SinkProbe {
        SinkProbe {
            state: Arc::clone(&self.state),
        }
    }
}

impl SinkProbe {
    /// Every write, in order.
    pub fn written(&self) -> Vec<WrittenFrame> {
        self.state.lock().written.clone()
    }

    /// The injected-clock times of every `flush`.
    pub fn flushes(&self) -> Vec<MonoTime> {
        self.state.lock().flushes.clone()
    }
}

impl AudioSink for FakeAudioSink {
    fn start(&mut self) -> Result<SinkInfo, EndpointError> {
        let mut s = self.state.lock();
        if let Some(e) = &self.start_error {
            s.last_error = Some(e.to_string());
            return Err(e.clone());
        }
        s.running = true;
        s.drained_to = self.clock.now();
        Ok(SinkInfo {
            rate: self.rate,
            device: "fake-sink".to_owned(),
        })
    }

    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
        let now = self.clock.now();
        let mut s = self.state.lock();
        if !s.running {
            s.last_error = Some(EndpointError::NotStarted.to_string());
            s.dropped_samples += frame.len() as u64;
            return WriteOutcome {
                accepted: 0,
                dropped: frame.len(),
            };
        }
        s.drain(now);
        let room = self
            .capacity
            .map_or(Duration::MAX, |c| c.saturating_sub(s.queued));
        let accepted = frame.len().min(self.rate.samples_in(room));
        let dropped = frame.len() - accepted;
        s.queued += self.rate.duration_of(accepted);
        s.accepted_frames += 1;
        s.dropped_samples += dropped as u64;
        s.written.push(WrittenFrame {
            written_at: now,
            frame_ts: frame.ts(),
            seq: frame.seq(),
            accepted,
            dropped,
        });
        WriteOutcome { accepted, dropped }
    }

    fn flush(&mut self) {
        let now = self.clock.now();
        let mut s = self.state.lock();
        s.queued = Duration::ZERO;
        s.drained_to = now;
        s.flushes.push(now);
    }

    fn queued(&self) -> Duration {
        let now = self.clock.now();
        let mut s = self.state.lock();
        s.drain(now);
        s.queued
    }

    fn health(&self) -> EndpointHealth {
        let s = self.state.lock();
        EndpointHealth {
            running: s.running,
            frames: s.accepted_frames,
            dropped_samples: s.dropped_samples,
            underruns: 0,
            last_error: s.last_error.clone(),
        }
    }

    fn stop(&mut self) {
        self.state.lock().running = false;
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::ManualClock;

    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;
    const RATE: SampleRate = SampleRate::HZ_16000;

    fn frame(seq: u64, ms: u64) -> AudioFrame {
        let samples = RATE.samples_in(MS(ms));
        AudioFrame::new(
            vec![0.1; samples],
            RATE,
            MonoTime::from_millis(seq * ms),
            seq,
        )
        .expect("frame")
    }

    fn frames(n: u64) -> Vec<AudioFrame> {
        (0..n).map(|i| frame(i, 20)).collect()
    }

    #[test]
    fn a_source_must_be_started_and_then_delivers_frames_in_order() {
        let mut source = FakeAudioSource::new(RATE, frames(3)).with_frames_per_read(2);
        let mut out = Vec::new();
        assert_eq!(
            source.read(&mut out, MS(10)),
            Err(EndpointError::NotStarted)
        );
        let info = source.start().expect("start");
        assert_eq!(info.rate, RATE);
        source.read(&mut out, MS(10)).expect("read");
        assert_eq!(
            out.iter().map(AudioFrame::seq).collect::<Vec<_>>(),
            vec![0, 1]
        );
        source.read(&mut out, MS(10)).expect("read");
        source
            .read(&mut out, MS(10))
            .expect("an exhausted source reads empty, not an error");
        assert_eq!(out.len(), 3);
        assert_eq!(source.remaining(), 0);
        let health = source.health();
        assert_eq!((health.running, health.frames), (true, 3));
        source.stop();
        assert!(!source.health().running);
    }

    #[test]
    fn a_source_failure_at_a_chosen_read_stops_the_source_and_is_reported_in_health() {
        let mut source = FakeAudioSource::new(RATE, frames(5)).failing_read_at(
            1,
            EndpointError::DeviceLost {
                device: "usb mic".to_owned(),
            },
        );
        source.start().expect("start");
        let mut out = Vec::new();
        source.read(&mut out, MS(10)).expect("first read");
        assert_eq!(
            source.read(&mut out, MS(10)),
            Err(EndpointError::DeviceLost {
                device: "usb mic".to_owned()
            })
        );
        let health = source.health();
        assert!(!health.running);
        assert!(health.last_error.is_some_and(|e| e.contains("usb mic")));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_source_can_fail_to_start() {
        let mut source =
            FakeAudioSource::new(RATE, vec![]).failing_start(EndpointError::Unavailable {
                reason: "no permission".to_owned(),
            });
        assert!(matches!(
            source.start(),
            Err(EndpointError::Unavailable { .. })
        ));
        assert!(!source.health().running);
    }

    #[test]
    fn the_sink_queue_drains_as_the_clock_advances() {
        let clock = ManualClock::new();
        let mut sink = FakeAudioSink::new(RATE, clock.clone());
        let probe = sink.probe();
        sink.start().expect("start");
        assert_eq!(
            sink.write(&frame(0, 100)),
            WriteOutcome {
                accepted: 1600,
                dropped: 0
            }
        );
        assert_eq!(sink.queued(), MS(100));
        clock.advance(MS(40));
        assert_eq!(sink.queued(), MS(60));
        sink.write(&frame(1, 100));
        assert_eq!(sink.queued(), MS(160));
        clock.advance(MS(500));
        assert_eq!(sink.queued(), Duration::ZERO);
        let written = probe.written();
        assert_eq!(written.len(), 2);
        assert_eq!(written[1].written_at, MonoTime::from_millis(40));
        assert_eq!(sink.health().frames, 2);
    }

    #[test]
    fn a_full_sink_drops_the_excess_and_counts_it() {
        let clock = ManualClock::new();
        let mut sink = FakeAudioSink::new(RATE, clock).with_capacity(MS(150));
        sink.start().expect("start");
        assert_eq!(sink.write(&frame(0, 100)).dropped, 0);
        let second = sink.write(&frame(1, 100));
        assert_eq!(
            second,
            WriteOutcome {
                accepted: 800,
                dropped: 800
            },
            "only 50 ms of room was left"
        );
        assert_eq!(sink.queued(), MS(150));
        assert_eq!(sink.health().dropped_samples, 800);
    }

    #[test]
    fn flush_discards_the_queue_and_is_recorded() {
        let clock = ManualClock::new();
        let mut sink = FakeAudioSink::new(RATE, clock.clone());
        let probe = sink.probe();
        sink.start().expect("start");
        sink.write(&frame(0, 100));
        clock.advance(MS(10));
        sink.flush();
        assert_eq!(sink.queued(), Duration::ZERO);
        assert_eq!(probe.flushes(), vec![MonoTime::from_millis(10)]);
    }

    #[test]
    fn writing_before_start_drops_everything_and_start_can_fail() {
        let clock = ManualClock::new();
        let mut sink = FakeAudioSink::new(RATE, clock.clone());
        assert_eq!(
            sink.write(&frame(0, 20)),
            WriteOutcome {
                accepted: 0,
                dropped: 320
            }
        );
        assert!(sink.health().last_error.is_some());

        let mut failing =
            FakeAudioSink::new(RATE, clock).failing_start(EndpointError::Unavailable {
                reason: "driver missing".to_owned(),
            });
        assert!(failing.start().is_err());
        assert!(!failing.health().running);
        sink.stop();
    }
}
