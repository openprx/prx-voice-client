//! Core Audio process-tap capture (macOS 14.4+).
//!
//! Captures the audio *rendered by a specific process* (e.g. a meeting app) and
//! delivers it as [`AudioFrame`]s over a channel. This is the macOS backend for
//! capturing the remote party's audio in the interpreter pipeline.
//!
//! This module is only compiled on macOS. The heavy lifting lives in the
//! Objective-C shim (`native/prxvc_tap.m`); this file is the safe Rust wrapper.
//!
//! Frames are emitted as raw interleaved **float32 at the tap's native rate**
//! (typically 48 kHz stereo); downmix-to-mono and resampling to 16 kHz for ASR
//! happen downstream, off the hot callback.

use std::os::raw::{c_int, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::{AudioError, AudioFormat, AudioFrame};

// ---------------------------------------------------------------------------
// Raw FFI surface (see native/prxvc_tap.h)
// ---------------------------------------------------------------------------

/// Opaque handle owned by the Objective-C shim.
#[repr(C)]
struct PrxvcTap {
    _private: [u8; 0],
}

/// Audio delivery callback type, matching `PrxvcTapAudioCb` in the header.
type TapAudioCb =
    extern "C" fn(ctx: *mut c_void, samples: *const f32, frame_count: u32, channels: u32, sample_rate: f64);

unsafe extern "C" {
    fn prxvc_tap_start(pid: c_int, cb: TapAudioCb, ctx: *mut c_void) -> *mut PrxvcTap;
    fn prxvc_tap_stop(tap: *mut PrxvcTap);
}

// ---------------------------------------------------------------------------
// Callback context
// ---------------------------------------------------------------------------

/// Heap state handed to the C callback as its opaque `ctx`. Owned by
/// [`CoreAudioTap`] via a raw pointer and freed on drop (after the tap stops,
/// so the callback can never observe freed memory).
struct TapCtx {
    tx: Sender<AudioFrame>,
    seq: AtomicU64,
}

/// Invoked from a Core Audio IO thread. Must not block. Converts interleaved
/// float32 into an [`AudioFrame`] and forwards it; drops the frame silently if
/// the receiver is gone.
extern "C" fn on_audio(
    ctx: *mut c_void,
    samples: *const f32,
    frame_count: u32,
    channels: u32,
    sample_rate: f64,
) {
    if ctx.is_null() || samples.is_null() || channels == 0 || frame_count == 0 {
        return;
    }
    // SAFETY: `ctx` was produced by `Box::into_raw(Box::new(TapCtx))` in
    // `CoreAudioTap::start` and stays valid until the tap is stopped and the box
    // is reclaimed in `Drop`. The C side guarantees no callbacks after stop.
    let ctx = unsafe { &*(ctx as *const TapCtx) };
    let count = (frame_count as usize) * (channels as usize);
    // SAFETY: the shim passes `frame_count * channels` valid f32 samples.
    let slice = unsafe { std::slice::from_raw_parts(samples, count) };

    let mut data = Vec::with_capacity(count * std::mem::size_of::<f32>());
    for sample in slice {
        data.extend_from_slice(&sample.to_le_bytes());
    }

    let frame = AudioFrame {
        sequence_no: ctx.seq.fetch_add(1, Ordering::Relaxed),
        sample_rate: sample_rate as u32,
        channels: channels as u16,
        format: AudioFormat::F32,
        timestamp_ms: 0,
        data,
    };
    // Receiver dropped => translation stopped; nothing to do.
    let _ = ctx.tx.send(frame);
}

// ---------------------------------------------------------------------------
// Safe wrapper
// ---------------------------------------------------------------------------

/// A running Core Audio process tap. Stops and releases all native resources on
/// drop. Construct with [`CoreAudioTap::start`].
pub struct CoreAudioTap {
    handle: *mut PrxvcTap,
    ctx: *mut TapCtx,
}

// SAFETY: `handle`/`ctx` are only dereferenced by the C side (on its own IO
// thread) and by `Drop` (which runs once, after `start`). The wrapper exposes no
// method that races the native thread, so moving it between threads is sound.
unsafe impl Send for CoreAudioTap {}

impl CoreAudioTap {
    /// Start capturing audio rendered by process `pid`.
    ///
    /// Returns the tap handle plus a [`Receiver`] of captured frames
    /// (interleaved float32 at the tap's native rate). Keep the [`CoreAudioTap`]
    /// alive for as long as you want frames; dropping it stops capture.
    ///
    /// # Errors
    /// Returns [`AudioError::InitializationFailed`] if the tap cannot be created
    /// — e.g. macOS < 14.4, missing `NSAudioCaptureUsageDescription` permission,
    /// or an invalid / non-audio `pid`.
    pub fn start(pid: i32) -> Result<(Self, Receiver<AudioFrame>), AudioError> {
        let (tx, rx) = channel();
        let ctx = Box::into_raw(Box::new(TapCtx {
            tx,
            seq: AtomicU64::new(0),
        }));

        // SAFETY: `on_audio` matches the C callback ABI; `ctx` is a valid,
        // owned pointer that outlives the native tap (freed below / in Drop).
        let handle = unsafe { prxvc_tap_start(pid as c_int, on_audio, ctx as *mut c_void) };

        if handle.is_null() {
            // Reclaim the context box the C side never took ownership of.
            // SAFETY: `ctx` came from `Box::into_raw` just above and was not
            // retained by the (failed) native call.
            unsafe { drop(Box::from_raw(ctx)) };
            return Err(AudioError::InitializationFailed(
                "Core Audio process tap failed to start (need macOS 14.4+, \
                 NSAudioCaptureUsageDescription granted, and a valid audio pid)"
                    .to_owned(),
            ));
        }

        Ok((Self { handle, ctx }, rx))
    }
}

impl Drop for CoreAudioTap {
    fn drop(&mut self) {
        // SAFETY: stop first so no further callbacks reference `ctx`, then
        // reclaim the context box. Both pointers are non-null (set in `start`).
        unsafe {
            prxvc_tap_stop(self.handle);
            drop(Box::from_raw(self.ctx));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Peak absolute amplitude across a float32 [`AudioFrame`]'s samples, in `0.0..`.
///
/// Handy for the "is the captured stream actually non-silent?" check
/// (some apps can deliver a silent tap stream).
pub fn frame_peak_amplitude(frame: &AudioFrame) -> f32 {
    if frame.format != AudioFormat::F32 {
        return 0.0;
    }
    let mut peak = 0.0f32;
    for chunk in frame.data.chunks_exact(std::mem::size_of::<f32>()) {
        // chunks_exact yields exactly 4 bytes; build a fixed array for from_le_bytes.
        let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let sample = f32::from_le_bytes(bytes).abs();
        if sample > peak {
            peak = sample;
        }
    }
    peak
}

/// Convenience alias for sharing a tap handle across owners during validation.
pub type SharedTap = Arc<CoreAudioTap>;

// ---------------------------------------------------------------------------
// AppAudioSource implementation
// ---------------------------------------------------------------------------

use crate::AudioState;
use crate::routing::{AppAudioSource, ProcessFilter};

/// macOS [`AppAudioSource`] backed by a Core Audio process tap.
///
/// Lazily starts the tap on [`start`](AppAudioSource::start) using the configured
/// [`ProcessFilter`], delivers native-format frames via
/// [`try_recv`](AppAudioSource::try_recv), and tears the tap down on
/// [`stop`](AppAudioSource::stop) or drop.
pub struct CoreAudioTapSource {
    filter: ProcessFilter,
    inner: Option<(CoreAudioTap, Receiver<AudioFrame>)>,
    state: AudioState,
}

impl CoreAudioTapSource {
    /// Create a source that will capture the given process filter when started.
    pub fn new(filter: ProcessFilter) -> Self {
        Self {
            filter,
            inner: None,
            state: AudioState::InputClosed,
        }
    }
}

impl AppAudioSource for CoreAudioTapSource {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.inner.is_some() {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::InputStreaming.to_string(),
            });
        }
        let pair = match self.filter {
            ProcessFilter::IncludeApp(pid) => CoreAudioTap::start(pid)?,
        };
        self.inner = Some(pair);
        self.state = AudioState::InputStreaming;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        // Dropping the CoreAudioTap stops the native tap.
        self.inner = None;
        self.state = AudioState::InputClosed;
        Ok(())
    }

    fn try_recv(&mut self) -> Option<AudioFrame> {
        self.inner.as_ref().and_then(|(_, rx)| rx.try_recv().ok())
    }

    fn state(&self) -> AudioState {
        self.state
    }
}
