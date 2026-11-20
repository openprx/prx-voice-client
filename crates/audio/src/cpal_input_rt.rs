//! Input device callback body: copies captured samples into the preallocated capture ring and counts what does not fit.
//!
//! Real-time file: see the convention in the crate docs. Nothing here
//! allocates, locks, logs or sends on a channel; the callback stamps the host
//! time, copies, bumps atomic counters and returns.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use crate::tap_ring::{RingProducer, SLOT_SAMPLES};

/// The callback's end of the capture ring and its counters.
pub struct CaptureWriter {
    producer: RingProducer,
    channels: usize,
    rate: u32,
    /// Interleaved samples dropped because the ring was full.
    dropped: Arc<AtomicU64>,
    /// Cleared when the stream stops; the callback then ignores its input.
    running: Arc<AtomicBool>,
    /// Zero point of the host times stored in the ring.
    origin: Instant,
}

impl CaptureWriter {
    /// A writer for interleaved input of `channels` at `rate` Hz.
    pub fn new(
        producer: RingProducer,
        channels: u16,
        rate: u32,
        dropped: Arc<AtomicU64>,
        running: Arc<AtomicBool>,
        origin: Instant,
    ) -> Self {
        Self {
            producer,
            channels: usize::from(channels.max(1)),
            rate: rate.max(1),
            dropped,
            running,
            origin,
        }
    }

    /// Copy one device buffer (interleaved f32) into the ring.
    ///
    /// A buffer larger than a slot is split on a frame boundary. When the ring
    /// is full the rest of the buffer is dropped and counted; the reader marks
    /// the discontinuity.
    pub fn write(&mut self, data: &[f32]) {
        if !self.running.load(Ordering::Acquire) {
            return;
        }
        let usable = data.len() - data.len() % self.channels;
        let per_slot = (SLOT_SAMPLES / self.channels) * self.channels;
        if usable == 0 || per_slot == 0 {
            return;
        }
        // The buffer just completed: its first sample was captured one buffer
        // length ago.
        let rate = u64::from(self.rate);
        let buffer_nanos = (usable / self.channels) as u64 * 1_000_000_000 / rate;
        let now = self.origin.elapsed().as_nanos() as u64;
        let first = now.saturating_sub(buffer_nanos);
        let mut offset = 0;
        while offset < usable {
            let end = (offset + per_slot).min(usable);
            let chunk = &data[offset..end];
            let host_time = first + (offset / self.channels) as u64 * 1_000_000_000 / rate;
            let pushed =
                self.producer
                    .push_with(self.channels as u32, self.rate, host_time, |slot| {
                        slot[..chunk.len()].copy_from_slice(chunk);
                        chunk.len()
                    });
            if !pushed {
                self.dropped
                    .fetch_add((usable - offset) as u64, Ordering::Relaxed);
                return;
            }
            offset = end;
        }
    }
}

/// The cpal input callback: feed the ring.
pub(crate) fn capture_callback(writer: &mut CaptureWriter, data: &[f32]) {
    writer.write(data);
}
