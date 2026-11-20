//! Bounded lock-free playback queue of interleaved device-rate samples: the writer side, the counters and the drain queries (the device-callback side is in `playback_buffer_rt.rs`).
//!
//! The queue is a single-producer single-consumer ring. The producer is the
//! sink's writer thread ([`push`](PlaybackBuffer::push),
//! [`end_stream`](PlaybackBuffer::end_stream), [`flush`](PlaybackBuffer::flush));
//! the consumer is the device callback ([`pull`](PlaybackBuffer::pull)), which
//! takes no lock, allocates nothing and never waits.
//!
//! Positions are monotonic sample counters (`head` is written by the producer,
//! `tail` by the consumer). Two more counters let the producer steer the
//! consumer without a lock:
//!
//! * `discard` is the position the consumer must skip to: [`flush`](PlaybackBuffer::flush)
//!   stores the current `head` there and the very next callback drops
//!   everything before it. The producer treats samples before `discard` as
//!   already gone, so a flush is visible to the writer at once.
//! * `end_at` is the `head` at which [`end_stream`](PlaybackBuffer::end_stream)
//!   was called. Running dry at or before that position is a normal end, not
//!   an underrun; later pushes move `head` past it, which starts a new stream.
//!
//! The samples are stored as `f32` bit patterns in relaxed atomics. This keeps
//! the ring free of `unsafe`: the only race that can exist (a flush racing a
//! callback on a nearly full ring) can glitch a few samples, never corrupt
//! memory.
//!
//! Drain waits poll instead of sleeping on a condition variable, because the
//! callback must not signal one.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::output::OutputStats;

/// How often [`PlaybackBuffer::wait_drained`] re-checks the queue.
const DRAIN_POLL: Duration = Duration::from_millis(1);

/// Bounded queue of interleaved device-rate samples shared between the
/// writer and the device callback.
pub struct PlaybackBuffer {
    /// Sample storage: `f32` bit patterns, indexed by position modulo length.
    pub(crate) slots: Box<[AtomicU32]>,
    /// Next position to write; stored by the producer only.
    pub(crate) head: AtomicU64,
    /// Next position to read; stored by the consumer only.
    pub(crate) tail: AtomicU64,
    /// Position the consumer skips to; stored by the producer (flush).
    pub(crate) discard: AtomicU64,
    /// Position of `head` when the producer announced the end of the stream.
    pub(crate) end_at: AtomicU64,
    pub(crate) underruns: AtomicU64,
    overruns: AtomicU64,
    dropped: AtomicU64,
}

impl PlaybackBuffer {
    /// Create a buffer holding at most `capacity_samples` samples. All memory
    /// is allocated here.
    pub fn new(capacity_samples: usize) -> Self {
        let slots = (0..capacity_samples.max(1))
            .map(|_| AtomicU32::new(0))
            .collect();
        Self {
            slots,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            discard: AtomicU64::new(0),
            end_at: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Maximum number of samples the buffer holds.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// First position that still counts as queued: everything before the
    /// consumer's tail or the last flush is gone.
    fn floor(&self) -> u64 {
        self.tail
            .load(Ordering::Acquire)
            .max(self.discard.load(Ordering::Acquire))
    }

    /// Queue samples. Whatever does not fit is dropped (newest first) and
    /// counted as an overrun. Returns the number of samples accepted.
    ///
    /// Only one thread may push at a time.
    pub fn push(&self, samples: &[f32]) -> usize {
        let cap = self.slots.len();
        let head = self.head.load(Ordering::Relaxed);
        let used = usize::try_from(head.saturating_sub(self.floor())).unwrap_or(cap);
        let room = cap.saturating_sub(used);
        let accepted = samples.len().min(room);
        if accepted < samples.len() {
            self.overruns.fetch_add(1, Ordering::Relaxed);
            self.dropped
                .fetch_add((samples.len() - accepted) as u64, Ordering::Relaxed);
        }
        if accepted > 0 {
            for (i, sample) in samples[..accepted].iter().enumerate() {
                let index = ((head + i as u64) % cap as u64) as usize;
                self.slots[index].store(sample.to_bits(), Ordering::Relaxed);
            }
            self.head.store(head + accepted as u64, Ordering::Release);
        }
        accepted
    }

    /// Tell the buffer that the current stream has no more samples, so that
    /// running dry afterwards is a normal end and not an underrun.
    pub fn end_stream(&self) {
        let head = self.head.load(Ordering::Relaxed);
        if head > self.floor() {
            self.end_at.store(head, Ordering::Release);
        }
    }

    /// Discard everything queued; the device callback drops it on its next
    /// run, and the queue reads as empty immediately. Returns the number of
    /// samples discarded.
    pub fn flush(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let queued = head.saturating_sub(self.floor());
        self.discard.store(head, Ordering::Release);
        usize::try_from(queued).unwrap_or(usize::MAX)
    }

    /// Samples currently queued (all channels).
    pub fn buffered_samples(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        usize::try_from(head.saturating_sub(self.floor())).unwrap_or(usize::MAX)
    }

    /// Whether nothing is queued.
    pub fn is_drained(&self) -> bool {
        self.buffered_samples() == 0
    }

    /// Wait until the buffer is empty or `timeout` passes. Returns whether
    /// it is drained.
    pub fn wait_drained(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_drained() {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            std::thread::sleep(DRAIN_POLL.min(deadline - now));
        }
    }

    /// Snapshot of the counters.
    pub fn stats(&self) -> OutputStats {
        OutputStats {
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            dropped_samples: self.dropped.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests;
