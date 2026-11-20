//! Device-callback side of the playback queue: copies queued samples out with no allocation, lock, log or wait.
//!
//! Real-time file: see the convention in the crate docs. Everything here runs
//! on the audio thread.

use std::sync::atomic::Ordering;

use crate::playback_buffer::PlaybackBuffer;

impl PlaybackBuffer {
    /// Fill `out` from the queue, padding with silence. Called from the
    /// device callback; only one thread may pull.
    ///
    /// A pull that empties the queue before `out` is full is an underrun,
    /// unless the writer announced the end of the stream.
    pub fn pull(&self, out: &mut [f32]) {
        let cap = self.slots.len() as u64;
        // `discard` is read before `head`, so it is never ahead of `head`.
        let skip_to = self.discard.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed).max(skip_to);
        let available = head.saturating_sub(tail);
        let n = (out.len() as u64).min(available) as usize;
        for (i, slot) in out[..n].iter_mut().enumerate() {
            let index = ((tail + i as u64) % cap) as usize;
            *slot = f32::from_bits(self.slots[index].load(Ordering::Relaxed));
        }
        out[n..].fill(0.0);
        let new_tail = tail + n as u64;
        self.tail.store(new_tail, Ordering::Release);
        let emptied = n > 0 && new_tail == head;
        if emptied && n < out.len() && self.end_at.load(Ordering::Acquire) < head {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}
