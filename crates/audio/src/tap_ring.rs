//! Bounded lock-free single-producer single-consumer ring of fixed-size audio slots: construction, shared state and the consumer side.
//!
//! All memory is allocated in [`tap_ring`]. The producer side
//! ([`RingProducer::push_with`], in [`crate::tap_ring_rt`]) never allocates,
//! locks or blocks: when the ring is full the new slot is dropped and `false`
//! is returned, and the caller counts the overrun. The consumer side runs on a
//! normal thread.

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Samples (not frames) one slot holds. A callback with more is split across slots.
pub const SLOT_SAMPLES: usize = 2048;

/// Metadata of a popped slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotMeta {
    /// Channels of the interleaved samples.
    pub channels: u32,
    /// Sample rate of the samples.
    pub sample_rate: u32,
    /// Capture time of the slot's first frame in nanoseconds of the producer's
    /// clock (the Core Audio host clock for the tap, an `Instant` origin for the
    /// microphone path), 0 if unknown.
    pub host_time: u64,
    /// Number of valid samples.
    pub len: usize,
}

pub(crate) struct Slot {
    pub(crate) meta: SlotMeta,
    pub(crate) data: Box<[f32]>,
}

pub(crate) struct Inner {
    pub(crate) slots: Box<[UnsafeCell<Slot>]>,
    /// Next slot index to write (monotonic); written by the producer only.
    pub(crate) head: AtomicUsize,
    /// Next slot index to read (monotonic); written by the consumer only.
    pub(crate) tail: AtomicUsize,
}

// SAFETY: a slot is accessed by at most one side at a time. The producer only
// touches slots in `tail..tail+cap` that are not yet published (index `head`),
// the consumer only touches published slots (`tail < head`), and publication
// and release go through Release/Acquire stores and loads of `head` and `tail`.
unsafe impl Sync for Inner {}
// SAFETY: `Inner` owns plain data; moving it between threads is sound.
unsafe impl Send for Inner {}

/// Write side; owned by the real-time callback's context.
pub struct RingProducer {
    pub(crate) inner: Arc<Inner>,
}

/// Read side; owned by a normal thread.
pub struct RingConsumer {
    inner: Arc<Inner>,
}

/// Create a ring of `capacity` slots (at least 2).
pub fn tap_ring(capacity: usize) -> (RingProducer, RingConsumer) {
    let capacity = capacity.max(2);
    let slots = (0..capacity)
        .map(|_| {
            UnsafeCell::new(Slot {
                meta: SlotMeta {
                    channels: 0,
                    sample_rate: 0,
                    host_time: 0,
                    len: 0,
                },
                data: vec![0.0; SLOT_SAMPLES].into_boxed_slice(),
            })
        })
        .collect();
    let inner = Arc::new(Inner {
        slots,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (
        RingProducer {
            inner: Arc::clone(&inner),
        },
        RingConsumer { inner },
    )
}

impl RingConsumer {
    /// Copy the oldest slot's samples into `out` (cleared first) and return its
    /// metadata, or `None` when the ring is empty.
    pub fn pop_into(&mut self, out: &mut Vec<f32>) -> Option<SlotMeta> {
        let inner = &*self.inner;
        let tail = inner.tail.load(Ordering::Relaxed);
        let head = inner.head.load(Ordering::Acquire);
        if tail == head {
            return None;
        }
        // SAFETY: slot `tail % cap` was published (tail < head) and the producer
        // will not reuse it until `tail` advances below; `&mut self` makes this
        // the only consumer.
        let slot = unsafe { &*inner.slots[tail % inner.slots.len()].get() };
        out.clear();
        out.extend_from_slice(&slot.data[..slot.meta.len]);
        let meta = slot.meta;
        inner.tail.store(tail + 1, Ordering::Release);
        Some(meta)
    }

    /// Discard everything queued.
    pub fn clear(&mut self) {
        let head = self.inner.head.load(Ordering::Acquire);
        self.inner.tail.store(head, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(p: &mut RingProducer, value: f32) -> bool {
        p.push_with(2, 48_000, 7, |buf| {
            buf[0] = value;
            1
        })
    }

    #[test]
    fn slots_come_out_in_order_with_metadata() {
        let (mut p, mut c) = tap_ring(4);
        assert!(push(&mut p, 1.0));
        assert!(push(&mut p, 2.0));
        let mut out = Vec::new();
        let meta = c.pop_into(&mut out).unwrap();
        assert_eq!(out, [1.0]);
        assert_eq!(
            meta,
            SlotMeta {
                channels: 2,
                sample_rate: 48_000,
                host_time: 7,
                len: 1
            }
        );
        c.pop_into(&mut out).unwrap();
        assert_eq!(out, [2.0]);
        assert!(c.pop_into(&mut out).is_none());
    }

    #[test]
    fn full_ring_drops_the_new_slot_and_reports_it() {
        let (mut p, mut c) = tap_ring(2);
        assert!(push(&mut p, 1.0));
        assert!(push(&mut p, 2.0));
        let mut called = false;
        let accepted = p.push_with(2, 48_000, 0, |_| {
            called = true;
            0
        });
        assert!(!accepted, "overrun must be reported");
        assert!(!called, "fill must not run when full");
        // The older slots are intact; space returns after a pop.
        let mut out = Vec::new();
        c.pop_into(&mut out).unwrap();
        assert_eq!(out, [1.0]);
        assert!(push(&mut p, 3.0));
    }

    #[test]
    fn wraps_around_many_times() {
        let (mut p, mut c) = tap_ring(3);
        let mut out = Vec::new();
        for i in 0..100 {
            assert!(push(&mut p, i as f32));
            c.pop_into(&mut out).unwrap();
            assert_eq!(out, [i as f32]);
        }
    }

    #[test]
    fn clear_discards_queued_slots() {
        let (mut p, mut c) = tap_ring(4);
        push(&mut p, 1.0);
        push(&mut p, 2.0);
        c.clear();
        assert!(c.pop_into(&mut Vec::new()).is_none());
    }

    #[test]
    fn producer_and_consumer_on_two_threads_lose_nothing_but_overruns() {
        let (mut p, mut c) = tap_ring(8);
        let total = 20_000usize;
        let producer = std::thread::spawn(move || {
            let mut dropped = 0usize;
            for i in 0..total {
                if !push(&mut p, i as f32) {
                    dropped += 1;
                }
            }
            dropped
        });
        let mut received = Vec::new();
        let mut out = Vec::new();
        loop {
            let finished = producer.is_finished();
            while c.pop_into(&mut out).is_some() {
                received.push(out[0]);
            }
            if finished {
                break;
            }
        }
        let dropped = producer.join().unwrap();
        assert_eq!(received.len() + dropped, total);
        assert!(received.windows(2).all(|w| w[0] < w[1]), "order preserved");
    }
}
