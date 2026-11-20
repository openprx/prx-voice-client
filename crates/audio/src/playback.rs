//! Playback of assistant (TTS) audio delivered by a transport.
//!
//! [`AssistantPlayer`] turns [`TtsAudioChunk`]s (mono PCM16 at the rate the
//! server announced) into frames for an [`AudioOutput`], which converts them
//! to the device format. The output device is opened lazily on the first
//! chunk, so a session that never speaks never touches audio hardware, and a
//! device that cannot be opened is reported instead of aborting the app.

use std::time::{Duration, Instant};

use prxvc_protocol::transport::TtsAudioChunk;

use crate::output::OutputStats;
use crate::pipeline::AudioOutput;
use crate::{AudioError, AudioFormat, AudioFrame, AudioOutputFrame, AudioState};

/// Opens (but does not start) the output device used for playback.
pub type OutputFactory = Box<dyn FnMut() -> Result<Box<dyn AudioOutput>, AudioError> + Send>;

/// How long to wait before trying to open a device that failed to open.
const REOPEN_BACKOFF: Duration = Duration::from_secs(5);

/// Plays assistant audio through an [`AudioOutput`].
pub struct AssistantPlayer {
    factory: OutputFactory,
    output: Option<Box<dyn AudioOutput>>,
    next_open_at: Option<Instant>,
    sequence_no: u64,
}

impl AssistantPlayer {
    /// Create a player that opens its device with `factory` on first use.
    pub fn new(factory: OutputFactory) -> Self {
        Self {
            factory,
            output: None,
            next_open_at: None,
            sequence_no: 0,
        }
    }

    /// Queue one chunk for playback.
    ///
    /// # Errors
    /// Fails if the device cannot be opened (retried after a short backoff,
    /// during which chunks are dropped silently) or the write fails.
    pub fn play(&mut self, chunk: &TtsAudioChunk) -> Result<(), AudioError> {
        if chunk.sample_rate == 0 {
            return Err(AudioError::FormatMismatch {
                expected: "non-zero sample rate".to_owned(),
                got: "0 Hz".to_owned(),
            });
        }
        let sequence_no = self.sequence_no;
        self.sequence_no += 1;
        // PCM16 samples are two bytes; ignore a dangling odd byte.
        let even = chunk.pcm16.len() & !1;
        let frame = AudioOutputFrame {
            session_id: chunk.session_id,
            frame: AudioFrame {
                sequence_no,
                sample_rate: chunk.sample_rate,
                channels: 1,
                format: AudioFormat::I16,
                timestamp_ms: 0,
                data: chunk.pcm16[..even].to_vec(),
            },
        };
        match self.ensure_output()? {
            Some(output) => output.write_frame(&frame),
            None => Ok(()),
        }
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
            if let Err(e) = output.flush() {
                tracing::warn!("flushing assistant audio failed: {e}");
            }
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
            let _ = output.stop();
        }
    }

    /// Open and start the output if needed. `Ok(None)` means "device is in
    /// its retry backoff; drop the chunk".
    fn ensure_output(&mut self) -> Result<Option<&mut Box<dyn AudioOutput>>, AudioError> {
        if self.output.is_none() {
            if self.next_open_at.is_some_and(|at| Instant::now() < at) {
                return Ok(None);
            }
            let opened = (self.factory)().and_then(|mut output| {
                if output.state() == AudioState::OutputClosed {
                    output.start()?;
                }
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
    use uuid::Uuid;

    #[derive(Default)]
    struct Spy {
        frames: Vec<AudioFrame>,
        flushes: u32,
        ends: u32,
    }

    struct SpyOutput {
        spy: Arc<parking_lot::Mutex<Spy>>,
        state: AudioState,
    }

    impl AudioOutput for SpyOutput {
        fn start(&mut self) -> Result<(), AudioError> {
            self.state = AudioState::OutputPlaying;
            Ok(())
        }
        fn stop(&mut self) -> Result<(), AudioError> {
            self.state = AudioState::OutputClosed;
            Ok(())
        }
        fn write_frame(&mut self, frame: &AudioOutputFrame) -> Result<(), AudioError> {
            self.spy.lock().frames.push(frame.frame.clone());
            Ok(())
        }
        fn flush(&mut self) -> Result<(), AudioError> {
            self.spy.lock().flushes += 1;
            Ok(())
        }
        fn end_stream(&mut self) {
            self.spy.lock().ends += 1;
        }
        fn state(&self) -> AudioState {
            self.state
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
                state: AudioState::OutputClosed,
            }) as Box<dyn AudioOutput>)
        }));
        (p, spy, opens)
    }

    fn chunk(rate: u32, bytes: usize) -> TtsAudioChunk {
        TtsAudioChunk {
            session_id: Uuid::nil(),
            sample_rate: rate,
            pcm16: (0..bytes).map(|i| i as u8).collect(),
        }
    }

    #[test]
    fn chunks_are_played_as_mono_pcm16_at_the_announced_rate() {
        let (mut p, spy, _) = player();
        p.play(&chunk(24_000, 640)).unwrap();
        p.play(&chunk(16_000, 321)).unwrap();
        let spy = spy.lock();
        assert_eq!(spy.frames.len(), 2);
        let f = &spy.frames[0];
        assert_eq!((f.sample_rate, f.channels, f.format), (24_000, 1, AudioFormat::I16));
        assert_eq!(f.data.len(), 640);
        assert_eq!(spy.frames[1].sample_rate, 16_000);
        assert_eq!(spy.frames[1].data.len(), 320, "odd trailing byte dropped");
        assert_eq!(spy.frames[1].sequence_no, 1);
    }

    #[test]
    fn the_device_is_opened_lazily_and_once() {
        let (mut p, _, opens) = player();
        assert_eq!(*opens.lock(), 0);
        p.flush();
        p.end_stream();
        assert_eq!(*opens.lock(), 0, "flush/end_stream do not open the device");
        p.play(&chunk(16_000, 64)).unwrap();
        p.play(&chunk(16_000, 64)).unwrap();
        assert_eq!(*opens.lock(), 1);
    }

    #[test]
    fn flush_and_end_stream_reach_the_output() {
        let (mut p, spy, _) = player();
        p.play(&chunk(16_000, 64)).unwrap();
        p.end_stream();
        p.flush();
        let spy = spy.lock();
        assert_eq!((spy.ends, spy.flushes), (1, 1));
    }

    #[test]
    fn a_device_that_cannot_open_is_reported_then_retried_later() {
        let mut p = AssistantPlayer::new(Box::new(|| Err(AudioError::OutputDeviceUnavailable)));
        assert!(p.play(&chunk(16_000, 64)).is_err());
        // Inside the backoff, chunks are dropped without another open attempt.
        assert!(p.play(&chunk(16_000, 64)).is_ok());
        assert!(p.is_drained());
    }

    #[test]
    fn zero_rate_is_rejected() {
        let (mut p, _, _) = player();
        assert!(p.play(&chunk(0, 64)).is_err());
    }

    #[test]
    fn close_releases_the_device_and_the_next_chunk_reopens_it() {
        let (mut p, _, opens) = player();
        p.play(&chunk(16_000, 64)).unwrap();
        p.close();
        p.play(&chunk(16_000, 64)).unwrap();
        assert_eq!(*opens.lock(), 2);
    }
}
