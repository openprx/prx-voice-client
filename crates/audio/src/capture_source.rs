//! Capture endpoint: implements the `AudioSource` port over the capture ring, with the device opened by a pluggable function.
//!
//! The device callback (`cpal_input_rt.rs`) writes into a preallocated ring and
//! does nothing else. [`read`](AudioSource::read) runs on the caller's normal
//! thread: it drains the ring through the [`CaptureReader`] into timestamped
//! mono frames. The device is opened by the function the source is built with,
//! so tests drive the ring from a plain thread with no hardware.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use prxvc_media::{AudioFrame, Clock, SampleRate};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

use crate::capture_reader::CaptureReader;
use crate::cpal_input_rt::CaptureWriter;
use crate::cpal_stream::{OpenedStream, StreamHandle};
use crate::tap_ring::tap_ring;

/// Slots of the capture ring: 256 callbacks, over two seconds of 512-frame
/// stereo callbacks at 48 kHz. Overflow drops the newest audio and counts it.
const RING_SLOTS: usize = 256;

/// How often [`read`](AudioSource::read) looks at the ring while it waits.
const READ_POLL: Duration = Duration::from_millis(2);

/// What the opener hands the device: the callback's writer and the flag the
/// stream's error callback sets when the device disappears.
pub struct CaptureLinks {
    /// The ring end the device callback writes into.
    pub writer: CaptureWriter,
    /// Set (from a non-real-time error callback) when the device is gone.
    pub lost: Arc<AtomicBool>,
}

/// Opens the device stream and starts capturing into `links`.
pub type SourceOpener = Box<dyn FnMut(CaptureLinks) -> Result<OpenedStream, EndpointError> + Send>;

struct Running {
    /// First, so that the device stops before the ring's reader goes.
    _stream: StreamHandle,
    reader: CaptureReader,
    device: String,
    running: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
}

/// An [`AudioSource`] fed by a device callback through a ring.
pub struct CaptureSource {
    rate: SampleRate,
    channels: u16,
    opener: SourceOpener,
    clock: Arc<dyn Clock>,
    dropped: Arc<AtomicU64>,
    live: Option<Running>,
    frames: u64,
    last_error: Option<String>,
}

impl CaptureSource {
    /// A source of interleaved input at `rate` with `channels` channels,
    /// stamped on `clock`, opening its device with `opener`.
    pub fn new(
        rate: SampleRate,
        channels: u16,
        clock: Arc<dyn Clock>,
        opener: SourceOpener,
    ) -> Self {
        Self {
            rate,
            channels: channels.max(1),
            opener,
            clock,
            dropped: Arc::new(AtomicU64::new(0)),
            live: None,
            frames: 0,
            last_error: None,
        }
    }

    fn drain_ring(&mut self, out: &mut Vec<AudioFrame>) -> Result<usize, EndpointError> {
        let live = self.live.as_mut().ok_or(EndpointError::NotStarted)?;
        let n = live.reader.drain(out);
        self.frames += n as u64;
        if n == 0 && live.lost.load(Ordering::Acquire) {
            let e = EndpointError::DeviceLost {
                device: live.device.clone(),
            };
            self.last_error = Some(e.to_string());
            self.stop();
            return Err(e);
        }
        Ok(n)
    }
}

impl AudioSource for CaptureSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        if self.live.is_some() {
            let e = EndpointError::Internal("source is already started".to_owned());
            self.last_error = Some(e.to_string());
            return Err(e);
        }
        let (producer, consumer) = tap_ring(RING_SLOTS);
        let running = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(AtomicBool::new(false));
        self.dropped.store(0, Ordering::Relaxed);
        let origin = self.clock.now();
        let writer = CaptureWriter::new(
            producer,
            self.channels,
            self.rate.hz(),
            Arc::clone(&self.dropped),
            Arc::clone(&running),
            Instant::now(),
        );
        let links = CaptureLinks {
            writer,
            lost: Arc::clone(&lost),
        };
        match (self.opener)(links) {
            Ok(opened) => {
                self.live = Some(Running {
                    reader: CaptureReader::new(consumer, origin, Arc::clone(&self.dropped)),
                    _stream: opened.handle,
                    device: opened.device.clone(),
                    running,
                    lost,
                });
                self.last_error = None;
                Ok(SourceInfo {
                    rate: self.rate,
                    device: opened.device,
                })
            }
            Err(e) => {
                running.store(false, Ordering::Release);
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        let deadline = Instant::now() + max_wait;
        loop {
            if self.drain_ring(out)? > 0 {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            std::thread::sleep(READ_POLL.min(deadline - now));
        }
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: self.live.is_some(),
            frames: self.frames,
            dropped_samples: self.dropped.load(Ordering::Relaxed),
            underruns: 0,
            last_error: self.last_error.clone(),
        }
    }

    fn stop(&mut self) {
        if let Some(live) = self.live.take() {
            live.running.store(false, Ordering::Release);
            // `live` drops here: the stream stops, then the ring goes.
        }
    }
}

impl Drop for CaptureSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
