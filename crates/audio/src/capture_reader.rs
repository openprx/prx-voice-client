//! Reader side of the capture ring: turns ring slots into timestamped mono `prxvc-media` frames of 20 ms on a normal thread.
//!
//! The reader downmixes interleaved samples to mono, re-cuts them into fixed
//! 20 ms frames (the ring holds whatever sizes the device callbacks had),
//! stamps each frame on the pipeline clock and marks a discontinuity on the
//! first frame after the ring overran.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use prxvc_media::{AudioFrame, MonoTime, SampleRate};

use crate::tap_ring::{RingConsumer, SLOT_SAMPLES};

/// Length of the frames the reader emits.
pub const FRAME_DURATION: Duration = Duration::from_millis(20);

/// Consumes the capture ring and builds frames.
pub struct CaptureReader {
    consumer: RingConsumer,
    scratch: Vec<f32>,
    /// Mono samples not yet forming a whole frame.
    pending: Vec<f32>,
    /// Pipeline time of `pending[0]`.
    pending_ts: MonoTime,
    /// Pipeline time of the writer's host-time origin.
    origin: MonoTime,
    dropped: Arc<AtomicU64>,
    dropped_seen: u64,
    gap: bool,
    seq: u64,
}

impl CaptureReader {
    /// A reader of `consumer`. `origin` is the pipeline time that matches the
    /// writer's host-time origin; `dropped` is the writer's overrun counter.
    pub fn new(consumer: RingConsumer, origin: MonoTime, dropped: Arc<AtomicU64>) -> Self {
        Self {
            consumer,
            scratch: Vec::with_capacity(SLOT_SAMPLES),
            pending: Vec::with_capacity(SLOT_SAMPLES * 2),
            pending_ts: origin,
            origin,
            dropped,
            dropped_seen: 0,
            gap: false,
            seq: 0,
        }
    }

    /// Append every complete frame the ring holds to `out`; returns how many.
    pub fn drain(&mut self, out: &mut Vec<AudioFrame>) -> usize {
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped != self.dropped_seen {
            self.dropped_seen = dropped;
            self.gap = true;
        }
        let before = out.len();
        while let Some(meta) = self.consumer.pop_into(&mut self.scratch) {
            let Ok(rate) = SampleRate::new(meta.sample_rate) else {
                continue;
            };
            let ts = self
                .origin
                .saturating_add(Duration::from_nanos(meta.host_time));
            if self.pending.is_empty() {
                self.pending_ts = ts;
            }
            let channels = (meta.channels as usize).max(1);
            self.pending
                .extend(self.scratch.chunks_exact(channels).map(|f| {
                    let mean = f.iter().sum::<f32>() / channels as f32;
                    if mean.is_finite() { mean } else { 0.0 }
                }));
            self.cut_frames(rate, out);
        }
        out.len() - before
    }

    /// Discard everything queued and any partial frame.
    pub fn clear(&mut self) {
        self.consumer.clear();
        self.pending.clear();
    }

    fn cut_frames(&mut self, rate: SampleRate, out: &mut Vec<AudioFrame>) {
        let frame_len = rate.samples_in(FRAME_DURATION).max(1);
        while self.pending.len() >= frame_len {
            let samples: Vec<f32> = self.pending.drain(..frame_len).collect();
            let frame = AudioFrame::new(samples, rate, self.pending_ts, self.seq)
                .map(|f| f.with_discontinuity(std::mem::take(&mut self.gap)));
            // A frame of finite, non-empty samples cannot fail to build.
            if let Ok(frame) = frame {
                out.push(frame);
                self.seq += 1;
            }
            self.pending_ts = self.pending_ts.saturating_add(rate.duration_of(frame_len));
        }
    }
}

#[cfg(test)]
mod tests;
