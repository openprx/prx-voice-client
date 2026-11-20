//! Ownership wrapper that lets an open cpal stream live inside a `Send` endpoint.

use std::any::Any;

/// A running device stream, type-erased so that endpoints can be built over a
/// fake in tests. Dropping it stops the device.
pub type StreamHandle = Box<dyn Any + Send>;

/// Wrapper to make `cpal::Stream` usable in `Send` contexts.
///
/// `cpal::Stream` on macOS (Core Audio) is not `Send` because of internal raw
/// pointers. The endpoint that owns it only drops it (which stops the device);
/// the audio callbacks run on a cpal-managed thread and reach the endpoint only
/// through `Arc`-shared atomics and rings, never through the stream.
pub struct SendStream(#[allow(dead_code)] pub cpal::Stream);

// SAFETY: the stream is created on one thread and only moved to be dropped;
// no method of it is called from another thread.
unsafe impl Send for SendStream {}

/// A stream the endpoint opened, with the name of the device behind it.
pub struct OpenedStream {
    /// Keeps the stream alive.
    pub handle: StreamHandle,
    /// The device's display name.
    pub device: String,
}
