//! One request's life: the sink that receives audio from the engine's callback thread, and the
//! consumer-side stream.
//!
//! # Back-pressure policy
//!
//! The chunk callback runs on the runtime's compute worker, which every batched request shares, so
//! it must never wait for a consumer. Each request therefore has its own bounded queue
//! (`std::sync::mpsc::sync_channel`) whose capacity is sized from the request's cap: a call cannot
//! produce more than `cap_frames` frames, every chunk is at least one frame, so a queue of
//! `cap_frames + QUEUE_HEADROOM` chunks cannot overflow while the engine behaves. Audio produced
//! faster than real time (about five times) therefore waits in the queue, bounded by the request's
//! own cap (at most a few hundred KB for a clause), until the consumer pulls it; a consumer that is
//! slow, or paused by its own downstream, never stalls the engine and never loses audio.
//!
//! If the queue does overflow anyway (an engine that sends chunks smaller than a frame by the
//! thousand), the callback does not drop audio and does not block: it counts an overrun, flags the
//! request, and the next cancel poll (a few milliseconds) stops the call. The consumer then reads
//! the audio that was queued, in order, followed by one `Unavailable` error that names the stall.
//! The overrun counter is part of [`TtsStats`](super::TtsStats).
//!
//! A consumer that disappears (the stream is dropped) abandons the request: the engine stops at
//! its next poll and the slot is free for the next request.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{PcmChunk, SampleRate};
use prxvc_ports::{CancellationToken, TtsError, TtsItem, TtsStream, VoiceSelection};

use super::cap;
use super::seam::CallControl;

/// Extra queue slots beyond the request's cap in frames.
pub const QUEUE_HEADROOM: usize = 8;
/// How often a waiting consumer looks at its cancellation token.
pub(super) const CANCEL_POLL: Duration = Duration::from_millis(2);

/// The terminal outcome of a request: the total audio, or the failure.
pub(super) type Outcome = Result<f32, TtsError>;

/// Capacity of the chunk queue of a request with `cap_frames`.
pub fn queue_capacity(cap_frames: u32) -> usize {
    (cap_frames as usize).saturating_add(QUEUE_HEADROOM)
}

/// Flags and counters shared between a request's stream, the worker and the engine's callbacks.
#[derive(Debug, Default)]
pub(super) struct RequestFlags {
    /// The consumer cancelled or dropped the stream.
    pub cancelled: AtomicBool,
}

/// Counters of the whole adapter.
#[derive(Debug, Default)]
pub(super) struct Counters {
    pub requests: AtomicU64,
    pub completed: AtomicU64,
    pub cancelled: AtomicU64,
    pub failed: AtomicU64,
    pub runaways: AtomicU64,
    pub retries: AtomicU64,
    pub overruns: AtomicU64,
    /// Requests the warm-up made (kept out of the counters above).
    pub warm_up_requests: AtomicU64,
}

/// The sink of one synthesis attempt: implements the engine's [`CallControl`].
pub(super) struct AttemptSink<'a> {
    flags: &'a RequestFlags,
    closing: &'a AtomicBool,
    counters: &'a Counters,
    tx: &'a SyncSender<PcmChunk>,
    /// Audio of the attempt, kept back until it ends inside its cap (retry mode).
    held: Option<Mutex<Vec<PcmChunk>>>,
    cap_frames: u32,
    produced: AtomicUsize,
    stalled: AtomicBool,
    runaway: AtomicBool,
    corrupt: AtomicBool,
}

impl<'a> AttemptSink<'a> {
    pub(super) fn new(
        flags: &'a RequestFlags,
        closing: &'a AtomicBool,
        counters: &'a Counters,
        tx: &'a SyncSender<PcmChunk>,
        cap_frames: u32,
        hold: bool,
    ) -> Self {
        Self {
            flags,
            closing,
            counters,
            tx,
            held: hold.then(|| Mutex::new(Vec::new())),
            cap_frames,
            produced: AtomicUsize::new(0),
            stalled: AtomicBool::new(false),
            runaway: AtomicBool::new(false),
            corrupt: AtomicBool::new(false),
        }
    }

    pub(super) fn produced_samples(&self) -> usize {
        self.produced.load(Ordering::Acquire)
    }

    pub(super) fn produced_secs(&self) -> f32 {
        SampleRate::HZ_24000
            .duration_of(self.produced_samples())
            .as_secs_f32()
    }

    pub(super) fn stalled(&self) -> bool {
        self.stalled.load(Ordering::Acquire)
    }

    pub(super) fn runaway(&self) -> bool {
        self.runaway.load(Ordering::Acquire)
    }

    pub(super) fn corrupt(&self) -> bool {
        self.corrupt.load(Ordering::Acquire)
    }

    pub(super) fn cancelled(&self) -> bool {
        self.flags.cancelled.load(Ordering::Acquire)
    }

    pub(super) fn closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    /// Hand the held audio of a successful attempt to the consumer's queue. `false` when the queue
    /// is full (counted as an overrun) or the consumer is gone.
    pub(super) fn release_held(&self) -> bool {
        let Some(held) = &self.held else { return true };
        let chunks = std::mem::take(&mut *held.lock());
        for chunk in chunks {
            if !self.send(chunk) {
                return false;
            }
        }
        true
    }

    fn send(&self, chunk: PcmChunk) -> bool {
        match self.tx.try_send(chunk) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.counters.overruns.fetch_add(1, Ordering::Relaxed);
                self.stalled.store(true, Ordering::Release);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                // The stream was dropped: the request is abandoned.
                self.flags.cancelled.store(true, Ordering::Release);
                false
            }
        }
    }
}

impl CallControl for AttemptSink<'_> {
    fn on_chunk(&self, samples: &[f32]) {
        if samples.is_empty() || self.should_abort() {
            return;
        }
        if samples.iter().any(|s| !s.is_finite()) {
            self.corrupt.store(true, Ordering::Release);
            return;
        }
        let total = self.produced.fetch_add(samples.len(), Ordering::AcqRel) + samples.len();
        let chunk = PcmChunk {
            samples: samples.to_vec(),
            rate: SampleRate::HZ_24000,
        };
        match &self.held {
            Some(held) => held.lock().push(chunk),
            None => {
                self.send(chunk);
            }
        }
        if cap::is_runaway(total, self.cap_frames) {
            self.runaway.store(true, Ordering::Release);
        }
    }

    fn should_abort(&self) -> bool {
        self.closing() || self.cancelled() || self.stalled() || self.runaway() || self.corrupt()
    }
}

/// The consumer side of one request.
pub(super) struct QwenStream {
    rx: Receiver<PcmChunk>,
    outcome: Arc<Mutex<Option<Outcome>>>,
    flags: Arc<RequestFlags>,
    voice: VoiceSelection,
    finished: bool,
}

impl QwenStream {
    pub(super) fn new(
        rx: Receiver<PcmChunk>,
        outcome: Arc<Mutex<Option<Outcome>>>,
        flags: Arc<RequestFlags>,
        voice: VoiceSelection,
    ) -> Self {
        Self {
            rx,
            outcome,
            flags,
            voice,
            finished: false,
        }
    }

    fn cancel_now(&mut self) -> TtsError {
        self.finished = true;
        self.flags.cancelled.store(true, Ordering::Release);
        TtsError::Cancelled
    }
}

impl TtsStream for QwenStream {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<TtsItem, TtsError>> {
        if self.finished {
            return None;
        }
        loop {
            if cancel.is_cancelled() {
                return Some(Err(self.cancel_now()));
            }
            match self.rx.recv_timeout(CANCEL_POLL) {
                Ok(chunk) => return Some(Ok(TtsItem::Chunk(chunk))),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    // The worker stored the outcome before it dropped the queue's sender, and the
                    // queue delivers every buffered chunk before it reports the disconnect.
                    self.finished = true;
                    let outcome = self.outcome.lock().take();
                    return Some(match outcome {
                        Some(Ok(audio_secs)) => Ok(TtsItem::Done { audio_secs }),
                        Some(Err(e)) => Err(e),
                        None => Err(TtsError::Internal(
                            "synthesis worker ended without an outcome".to_owned(),
                        )),
                    });
                }
            }
        }
    }

    fn voice_used(&self) -> &VoiceSelection {
        &self.voice
    }
}

impl Drop for QwenStream {
    fn drop(&mut self) {
        // A stream dropped before its end abandons the request; after the end this is a no-op.
        self.flags.cancelled.store(true, Ordering::Release);
    }
}
