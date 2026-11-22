//! Core Audio process-tap IO callback: validates one cycle's buffers, stamps them with the capture time and copies them into the preallocated ring.
//!
//! Real-time file: see the convention in the crate docs. [`on_buffers`] runs on
//! the Core Audio HAL IO thread, so nothing here allocates, locks, logs or
//! sends: it copies samples, bumps atomic counters and returns. A full ring
//! drops the new data and counts the overrun. The context type and the raw FFI
//! types are in [`crate::core_audio_tap`].

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use crate::core_audio_tap::{RawTapBuffer, TapCtx};
use crate::tap_format::{InputBuffer, MAX_BUFFERS, StreamLayout};
use crate::tap_format_rt::any_nonzero;
use crate::tap_ring::{RingProducer, SLOT_SAMPLES};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Capture time of the slot that starts `start_frames` frames into a callback
/// whose first frame was captured at `host_ns` (0 when the system gave none,
/// which stays 0 so the reader falls back to its own clock).
pub(crate) fn slot_time_ns(host_ns: u64, start_frames: usize, sample_rate: u32) -> u64 {
    if host_ns == 0 {
        return 0;
    }
    let offset =
        (start_frames as u64).saturating_mul(NANOS_PER_SECOND) / u64::from(sample_rate.max(1));
    host_ns.saturating_add(offset)
}

/// IO callback registered with the native shim. Runs on the Core Audio HAL
/// thread (CS-4.1.1); `host_time_ns` is the capture time of the cycle's first
/// frame on the Core Audio host clock, 0 if unknown.
pub(crate) extern "C" fn on_buffers(
    ctx: *mut c_void,
    count: u32,
    buffers: *const RawTapBuffer,
    host_time_ns: u64,
) {
    if ctx.is_null() || buffers.is_null() || count == 0 {
        return;
    }
    // A panic must never unwind into C; `catch_unwind` does not allocate unless
    // a panic occurs.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `ctx` is `Arc::as_ptr` of a `TapCtx` that outlives the native
        // tap (the supervisor drops the tap before the last `Arc`).
        let ctx = unsafe { &*(ctx as *const TapCtx) };
        // SAFETY: the shim passes `count` valid `RawTapBuffer`s for the call's duration.
        let raw = unsafe { std::slice::from_raw_parts(buffers, (count as usize).min(MAX_BUFFERS)) };
        // SAFETY: the HAL calls the callback serially and never after
        // `prxvc_tap_stop` returned, so this is the only access to the producer.
        let producer = unsafe { &mut *ctx.producer.get() };
        deliver(ctx, producer, raw, host_time_ns);
    }));
}

/// Validate one callback's buffers against the installed layout and queue them.
fn deliver(ctx: &TapCtx, producer: &mut RingProducer, raw: &[RawTapBuffer], host_time_ns: u64) {
    let counters = &ctx.counters;
    // Counted first so a stream of unreadable buffers is not mistaken for a stall.
    counters.callbacks.fetch_add(1, Ordering::Relaxed);
    let Some(layout) = StreamLayout::unpack(ctx.layout.load(Ordering::Acquire)) else {
        return;
    };
    let mut views = [InputBuffer::EMPTY; MAX_BUFFERS];
    for (view, b) in views.iter_mut().zip(raw) {
        if b.data.is_null()
            || !(b.data as usize).is_multiple_of(std::mem::align_of::<f32>())
            || !b.byte_size.is_multiple_of(4)
        {
            counters.bad_buffers.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // SAFETY: non-null, 4-byte aligned, `byte_size` bytes valid for this call.
        let samples =
            unsafe { std::slice::from_raw_parts(b.data as *const f32, (b.byte_size / 4) as usize) };
        *view = InputBuffer {
            channels: b.channels,
            samples,
        };
    }
    let views = &views[..raw.len()];
    let Some(frames) = layout.frames_in(views) else {
        counters.bad_buffers.fetch_add(1, Ordering::Relaxed);
        return;
    };
    counters.frames.fetch_add(frames as u64, Ordering::Relaxed);
    if any_nonzero(views) {
        counters.nonzero_callbacks.fetch_add(1, Ordering::Relaxed);
    }

    let channels = layout.channels as usize;
    let per_slot = SLOT_SAMPLES / channels;
    let mut start = 0;
    while start < frames {
        let take = per_slot.min(frames - start);
        let slot_ns = slot_time_ns(host_time_ns, start, layout.sample_rate);
        let queued = producer.push_with(layout.channels, layout.sample_rate, slot_ns, |out| {
            layout.interleave_into(views, start, take, out);
            take * channels
        });
        if !queued {
            // Ring full: the consumer fell behind. Drop the rest and count it.
            counters.overrun_slots.fetch_add(1, Ordering::Relaxed);
            counters
                .overrun_frames
                .fetch_add((frames - start) as u64, Ordering::Relaxed);
            return;
        }
        start += take;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_time_adds_the_frame_offset_at_the_sample_rate() {
        assert_eq!(slot_time_ns(1_000_000, 0, 48_000), 1_000_000);
        // 480 frames at 48 kHz are 10 ms.
        assert_eq!(slot_time_ns(1_000_000, 480, 48_000), 11_000_000);
        assert_eq!(slot_time_ns(5, 1024, 16_000), 5 + 64_000_000);
    }

    #[test]
    fn unknown_host_time_stays_unknown() {
        assert_eq!(slot_time_ns(0, 960, 48_000), 0);
    }

    #[test]
    fn extreme_values_saturate_instead_of_wrapping() {
        assert_eq!(slot_time_ns(u64::MAX, 480, 48_000), u64::MAX);
        assert_eq!(slot_time_ns(1, 10, 0), 1 + 10 * NANOS_PER_SECOND);
    }
}
