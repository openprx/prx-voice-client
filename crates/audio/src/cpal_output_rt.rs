//! Output device callback body: drains the playback ring into the device buffer, or plays silence when the sink is stopped.
//!
//! Real-time file: see the convention in the crate docs.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::playback_buffer::PlaybackBuffer;

/// Fill one device buffer. Called by the cpal output callback.
pub(crate) fn fill_output(running: &AtomicBool, buffer: &PlaybackBuffer, data: &mut [f32]) {
    if !running.load(Ordering::Acquire) {
        data.fill(0.0);
        return;
    }
    buffer.pull(data);
}
