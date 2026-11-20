//! Producer side of the tap ring: fills the next free slot from the real-time callback, never allocating, locking or blocking.
//!
//! Real-time file: see the convention in the crate docs. The ring itself, its
//! shared state and the consumer are in [`crate::tap_ring`].

use std::sync::atomic::Ordering;

use crate::tap_ring::{RingProducer, SLOT_SAMPLES, SlotMeta};

impl RingProducer {
    /// Fill the next free slot. `fill` receives the slot's sample buffer
    /// ([`SLOT_SAMPLES`] long) and returns how many samples it wrote.
    ///
    /// Returns `false` without calling `fill` when the ring is full.
    pub fn push_with(
        &mut self,
        channels: u32,
        sample_rate: u32,
        host_time: u64,
        fill: impl FnOnce(&mut [f32]) -> usize,
    ) -> bool {
        let inner = &*self.inner;
        let head = inner.head.load(Ordering::Relaxed);
        let tail = inner.tail.load(Ordering::Acquire);
        if head - tail >= inner.slots.len() {
            return false;
        }
        // SAFETY: slot `head % cap` is free (head - tail < cap) and unpublished,
        // so the consumer does not touch it; `&mut self` makes this the only producer.
        let slot = unsafe { &mut *inner.slots[head % inner.slots.len()].get() };
        let len = fill(&mut slot.data).min(SLOT_SAMPLES);
        slot.meta = SlotMeta { channels, sample_rate, host_time, len };
        inner.head.store(head + 1, Ordering::Release);
        true
    }
}
