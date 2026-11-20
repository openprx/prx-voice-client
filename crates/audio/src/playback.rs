//! Playback of assistant (TTS) audio delivered by a transport.
//!
//! [`AssistantPlayer`] turns [`PcmChunk`]s (mono f32 at the rate the server
//! announced) into frames for a [`StreamSink`], which converts them to the
//! device format. The output device is opened lazily on the first chunk, so a
//! session that never speaks never touches audio hardware, and a device that
//! cannot be opened is reported instead of aborting the app.

use std::time::{Duration, Instant};

use prxvc_media::{AudioFrame, Clock, PcmChunk, SystemClock};

use crate::AudioError;
use crate::output::OutputStats;
use crate::stream_sink::StreamSink;

/// Opens (but does not start) the output device used for playback.
pub type OutputFactory = Box<dyn FnMut() -> Result<Box<dyn StreamSink>, AudioError> + Send>;

/// How long to wait before trying to open a device that failed to open.
const REOPEN_BACKOFF: Duration = Duration::from_secs(5);

/// Plays assistant audio through a [`StreamSink`].
pub struct AssistantPlayer {
    factory: OutputFactory,
    output: Option<Box<dyn StreamSink>>,
    next_open_at: Option<Instant>,
    clock: SystemClock,
    sequence_no: u64,
}

impl AssistantPlayer {
    /// Create a player that opens its device with `factory` on first use.
    pub fn new(factory: OutputFactory) -> Self {
        Self {
            factory,
            output: None,
            next_open_at: None,
            clock: SystemClock::new(),
            sequence_no: 0,
        }
    }

    /// Queue one chunk for playback.
    ///
    /// # Errors
    /// Fails if the device cannot be opened (retried after a short backoff,
    /// during which chunks are dropped silently) or the chunk holds samples
    /// that are not finite numbers. An empty chunk is ignored.
    pub fn play(&mut self, chunk: &PcmChunk) -> Result<(), AudioError> {
        if chunk.samples.is_empty() {
            return Ok(());
        }
        let sequence_no = self.sequence_no;
        self.sequence_no += 1;
        let frame = AudioFrame::new(chunk.samples.clone(), chunk.rate, self.clock.now(), sequence_no)
            .map_err(|e| AudioError::FormatMismatch {
                expected: "non-empty finite samples".to_owned(),
                got: e.to_string(),
            })?;
        if let Some(output) = self.ensure_output()? {
            // Samples the sink could not queue are counted in its stats.
            output.write(&frame);
        }
        Ok(())
    }

    /// The current reply has no more audio: running dry is not an underrun.
    pub fn end_stream(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.end_stream();
        }
    }

    /// Stop playing right now and discard whatever is queued.
    pub fn flush(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.flush();
        }
    }

    /// Whether everything queued has been played.
    pub fn is_drained(&self) -> bool {
        self.output.as_ref().is_none_or(|o| o.is_drained())
    }

    /// Block until [`is_drained`](Self::is_drained) or `timeout` passes.
    pub fn wait_drained(&self, timeout: Duration) -> bool {
        self.output.as_ref().is_none_or(|o| o.wait_drained(timeout))
    }

    /// Underrun / overrun counters of the output (zero before first use).
    pub fn stats(&self) -> OutputStats {
        self.output.as_ref().map(|o| o.stats()).unwrap_or_default()
    }

    /// Stop the device and release it. The next chunk reopens it.
    pub fn close(&mut self) {
        if let Some(mut output) = self.output.take() {
            output.stop();
        }
    }

    /// Open and start the output if needed. `Ok(None)` means "device is in
    /// its retry backoff; drop the chunk".
    fn ensure_output(&mut self) -> Result<Option<&mut Box<dyn StreamSink>>, AudioError> {
        if self.output.is_none() {
            if self.next_open_at.is_some_and(|at| Instant::now() < at) {
                return Ok(None);
            }
            let opened = (self.factory)().and_then(|mut output| {
                output.start()?;
                Ok(output)
            });
            match opened {
                Ok(output) => {
                    self.next_open_at = None;
                    self.output = Some(output);
                }
                Err(e) => {
                    self.next_open_at = Some(Instant::now() + REOPEN_BACKOFF);
                    return Err(e);
                }
            }
        }
        Ok(self.output.as_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use prxvc_media::SampleRate;
    use prxvc_ports::{AudioSink, EndpointError, EndpointHealth, SinkInfo, WriteOutcome};

    #[derive(Default)]
    struct Spy {
        frames: Vec<AudioFrame>,
        starts: u32,
        flushes: u32,
        ends: u32,
    }

    struct SpyOutput {
        spy: Arc<parking_lot::Mutex<Spy>>,
    }

    impl AudioSink for SpyOutput {
        fn start(&mut self) -> Result<SinkInfo, EndpointError> {
            self.spy.lock().starts += 1;
            Ok(SinkInfo {
                rate: SampleRate::HZ_48000,
                device: "spy".to_owned(),
            })
        }
        fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
            self.spy.lock().frames.push(frame.clone());
            WriteOutcome {
                accepted: frame.len(),
                dropped: 0,
            }
        }
        fn flush(&mut self) {
            self.spy.lock().flushes += 1;
        }
        fn queued(&self) -> Duration {
            Duration::ZERO
        }
        fn health(&self) -> EndpointHealth {
            EndpointHealth::default()
        }
        fn stop(&mut self) {}
    }

    impl StreamSink for SpyOutput {
        fn end_stream(&mut self) {
            self.spy.lock().ends += 1;
        }
        fn is_drained(&self) -> bool {
            true
        }
        fn wait_drained(&self, _timeout: Duration) -> bool {
            true
        }
        fn stats(&self) -> OutputStats {
            OutputStats::default()
        }
    }

    fn player() -> (AssistantPlayer, Arc<parking_lot::Mutex<Spy>>, Arc<parking_lot::Mutex<u32>>) {
        let spy = Arc::new(parking_lot::Mutex::new(Spy::default()));
        let opens = Arc::new(parking_lot::Mutex::new(0u32));
        let (s, o) = (Arc::clone(&spy), Arc::clone(&opens));
        let p = AssistantPlayer::new(Box::new(move || {
            *o.lock() += 1;
            Ok(Box::new(SpyOutput {
                spy: Arc::clone(&s),
            }) as Box<dyn StreamSink>)
        }));
        (p, spy, opens)
    }

    fn chunk(rate: u32, samples: usize) -> PcmChunk {
        PcmChunk {
            samples: (0..samples).map(|i| i as f32 / 1000.0).collect(),
            rate: SampleRate::new(rate).expect("non-zero rate"),
        }
    }

    #[test]
    fn chunks_are_played_as_mono_frames_at_the_announced_rate() {
        let (mut p, spy, _) = player();
        p.play(&chunk(24_000, 320)).unwrap();
        p.play(&chunk(16_000, 160)).unwrap();
        let spy = spy.lock();
        assert_eq!(spy.frames.len(), 2);
        let f = &spy.frames[0];
        assert_eq!((f.rate().hz(), f.len(), f.seq()), (24_000, 320, 0));
        assert_eq!(spy.frames[1].rate().hz(), 16_000);
        assert_eq!(spy.frames[1].len(), 160);
        assert_eq!(spy.frames[1].seq(), 1);
        assert!(spy.frames[1].ts() >= f.ts(), "frames carry the clock's time");
    }

    #[test]
    fn the_device_is_opened_lazily_and_once() {
        let (mut p, spy, opens) = player();
        assert_eq!(*opens.lock(), 0);
        p.flush();
        p.end_stream();
        assert_eq!(*opens.lock(), 0, "flush/end_stream do not open the device");
        p.play(&chunk(16_000, 32)).unwrap();
        p.play(&chunk(16_000, 32)).unwrap();
        assert_eq!(*opens.lock(), 1);
        assert_eq!(spy.lock().starts, 1, "the sink is started once");
    }

    #[test]
    fn flush_and_end_stream_reach_the_output() {
        let (mut p, spy, _) = player();
        p.play(&chunk(16_000, 32)).unwrap();
        p.end_stream();
        p.flush();
        let spy = spy.lock();
        assert_eq!((spy.ends, spy.flushes), (1, 1));
    }

    #[test]
    fn a_device_that_cannot_open_is_reported_then_retried_later() {
        let mut p = AssistantPlayer::new(Box::new(|| Err(AudioError::OutputDeviceUnavailable)));
        assert!(p.play(&chunk(16_000, 32)).is_err());
        // Inside the backoff, chunks are dropped without another open attempt.
        assert!(p.play(&chunk(16_000, 32)).is_ok());
        assert!(p.is_drained());
    }

    #[test]
    fn non_finite_samples_are_rejected_and_empty_chunks_ignored() {
        let (mut p, spy, _) = player();
        let mut bad = chunk(16_000, 4);
        bad.samples[2] = f32::NAN;
        assert!(p.play(&bad).is_err());
        assert!(p.play(&chunk(16_000, 0)).is_ok());
        assert!(spy.lock().frames.is_empty());
    }

    #[test]
    fn close_releases_the_device_and_the_next_chunk_reopens_it() {
        let (mut p, _, opens) = player();
        p.play(&chunk(16_000, 32)).unwrap();
        p.close();
        p.play(&chunk(16_000, 32)).unwrap();
        assert_eq!(*opens.lock(), 2);
    }
}
