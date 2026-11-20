//! Output stream on a Core Audio device chosen by UID, hidden devices included: safe wrapper over `native/prxvc_output.m`.
//!
//! cpal cannot open the virtual microphone's sink: Core Audio keeps hidden
//! devices out of the device list that cpal enumerates. This module resolves
//! the UID itself and starts an IO proc that drains the playback ring through
//! the trampoline of `core_audio_output_rt.rs`.

use std::ffi::{CString, c_void};
use std::os::raw::c_char;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use prxvc_ports::EndpointError;

use crate::core_audio_output_rt::{RenderState, render_trampoline};
use crate::cpal_stream::OpenedStream;
use crate::playback_buffer::PlaybackBuffer;

type RenderFn = unsafe extern "C" fn(*mut c_void, *mut f32, u32, u32);

#[repr(C)]
struct RawOutput {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn prxvc_output_format(uid: *const c_char, rate: *mut f64, channels: *mut u32) -> i32;
    fn prxvc_output_open(
        uid: *const c_char,
        render: RenderFn,
        user: *mut c_void,
        status: *mut i32,
    ) -> *mut RawOutput;
    fn prxvc_output_close(output: *mut RawOutput);
}

/// The sample rate and channel count of the first output stream of the device.
///
/// # Errors
/// [`EndpointError::Unavailable`] when the device is not found, has no output
/// stream, or does not use 32-bit float interleaved PCM.
pub fn output_format(uid: &str) -> Result<(u32, u16), EndpointError> {
    let c_uid = c_string(uid)?;
    let (mut rate, mut channels) = (0.0f64, 0u32);
    // SAFETY: `c_uid` is NUL-terminated; both out-pointers are valid for the call.
    let status = unsafe { prxvc_output_format(c_uid.as_ptr(), &mut rate, &mut channels) };
    if status != 0 {
        return Err(unavailable(uid, status));
    }
    Ok((
        rate.round() as u32,
        channels.min(u32::from(u16::MAX)) as u16,
    ))
}

/// A running output stream; dropping it stops the device.
pub struct CoreAudioOutput {
    raw: *mut RawOutput,
    /// Handed to the IO proc as its user pointer; freed after the device stopped.
    state: *mut RenderState,
}

// SAFETY: the stream is created on one thread and then only dropped. The IO
// proc reaches `state` through shared atomics and the ring, never through this
// handle; `Drop` stops the device before freeing the state.
unsafe impl Send for CoreAudioOutput {}

impl Drop for CoreAudioOutput {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `prxvc_output_open` and is closed once; after
        // the close returns the IO proc no longer runs, so `state` can be freed.
        unsafe {
            prxvc_output_close(self.raw);
            drop(Box::from_raw(self.state));
        }
    }
}

/// Start playing `buffer` on the device with this UID.
///
/// The device is looked up again on every call, so a restart picks up a
/// device that was reinstalled.
///
/// # Errors
/// [`EndpointError::Unavailable`] with the reason.
pub fn open(
    uid: &str,
    device_name: &str,
    buffer: &Arc<PlaybackBuffer>,
    running: &Arc<AtomicBool>,
) -> Result<OpenedStream, EndpointError> {
    let c_uid = c_string(uid)?;
    let state = Box::into_raw(Box::new(RenderState::new(
        Arc::clone(running),
        Arc::clone(buffer),
    )));
    let mut status = 0i32;
    // SAFETY: `c_uid` is NUL-terminated; `state` stays valid until `CoreAudioOutput`
    // is dropped, which closes the stream first; `status` is a valid out-pointer.
    let raw =
        unsafe { prxvc_output_open(c_uid.as_ptr(), render_trampoline, state.cast(), &mut status) };
    if raw.is_null() {
        // SAFETY: the stream never started, so nothing else holds `state`.
        drop(unsafe { Box::from_raw(state) });
        return Err(unavailable(uid, status));
    }
    Ok(OpenedStream {
        handle: Box::new(CoreAudioOutput { raw, state }),
        device: device_name.to_owned(),
    })
}

fn c_string(uid: &str) -> Result<CString, EndpointError> {
    CString::new(uid).map_err(|_| EndpointError::Unavailable {
        reason: "device UID contains a NUL byte".to_owned(),
    })
}

fn unavailable(uid: &str, status: i32) -> EndpointError {
    let why = match status {
        -1 => "no such device".to_owned(),
        -2 => "the device has no output stream".to_owned(),
        -3 => "the device does not use 32-bit float interleaved PCM".to_owned(),
        other => format!("Core Audio status {other}"),
    };
    EndpointError::Unavailable {
        reason: format!("output device `{uid}`: {why}"),
    }
}
