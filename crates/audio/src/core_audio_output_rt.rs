//! IO-proc body of the Core Audio output stream: hands the device buffer to the playback ring drain.
//!
//! Real-time file: see the convention in the crate docs. The C shim calls
//! [`render_trampoline`] on the device's IO thread; it does nothing but turn
//! the raw buffer into a slice and call `fill_output`.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::cpal_output_rt::fill_output;
use crate::playback_buffer::PlaybackBuffer;

/// What the IO proc needs: the run flag and the ring it drains.
pub(crate) struct RenderState {
    running: Arc<AtomicBool>,
    buffer: Arc<PlaybackBuffer>,
}

impl RenderState {
    /// State for one stream (built off the audio thread).
    pub(crate) fn new(running: Arc<AtomicBool>, buffer: Arc<PlaybackBuffer>) -> Self {
        Self { running, buffer }
    }
}

/// Called by `native/prxvc_output.m` on the IO thread with `frames` frames of
/// `channels` interleaved float32 samples to fill.
///
/// # Safety
/// `user` must point to a live [`RenderState`] and `out` to
/// `frames * channels` writable floats, as the shim guarantees.
pub(crate) unsafe extern "C" fn render_trampoline(
    user: *mut c_void,
    out: *mut f32,
    frames: u32,
    channels: u32,
) {
    // SAFETY: see the function's contract.
    let (state, data) = unsafe {
        (
            &*user.cast::<RenderState>(),
            std::slice::from_raw_parts_mut(out, frames as usize * channels as usize),
        )
    };
    fill_output(&state.running, &state.buffer, data);
}
