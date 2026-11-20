//! A VAD session: speech probabilities for a stream of 16 kHz mono float frames.

use std::ffi::c_int;
use std::sync::Arc;

use prxvc_transcribe_sys as sys;

use super::{NativeError, TranscribeModel, check_finite, status_error};

/// Audio requirements of a VAD model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VadInfo {
    /// Required sample rate in hertz.
    pub sample_rate: u32,
    /// Samples per frame (one probability per frame).
    pub frame_samples: usize,
}

/// A VAD session on a model. One thread uses it at a time; it can move between threads.
#[derive(Debug)]
pub struct NativeVad {
    raw: *mut sys::transcribe_vad_session,
    model: Arc<TranscribeModel>,
    info: VadInfo,
}

// SAFETY: a session is single-threaded-at-a-time, not thread-affine; every call takes `&mut self`.
unsafe impl Send for NativeVad {}

impl NativeVad {
    /// Create a session on a model that serves the VAD role.
    pub fn new(model: &Arc<TranscribeModel>) -> Result<Self, NativeError> {
        if !model.is_vad() {
            return Err(NativeError::WrongRole(
                "the model does not serve the VAD role".to_owned(),
            ));
        }
        let mut info = std::mem::MaybeUninit::<sys::transcribe_vad_info>::uninit();
        // SAFETY: `transcribe_vad_info_init` fully initialises the struct.
        let mut info = unsafe {
            sys::transcribe_vad_info_init(info.as_mut_ptr());
            info.assume_init()
        };
        // SAFETY: the model is valid and `info` is initialised.
        let status = unsafe { sys::transcribe_vad_get_info(model.raw(), &mut info) };
        if status != sys::TRANSCRIBE_OK {
            return Err(status_error("vad info", status));
        }
        let info = VadInfo {
            sample_rate: u32::try_from(info.sample_rate).unwrap_or(0),
            frame_samples: usize::try_from(info.frame_samples).unwrap_or(0),
        };
        if info.sample_rate == 0 || info.frame_samples == 0 {
            return Err(NativeError::WrongRole(
                "the VAD model reports no frame size".to_owned(),
            ));
        }
        let mut raw = std::ptr::null_mut();
        // SAFETY: the model is valid (the session keeps an `Arc` of it), null parameters mean
        // defaults and `raw` is a valid out pointer.
        let status =
            unsafe { sys::transcribe_vad_session_init(model.raw(), std::ptr::null(), &mut raw) };
        if status != sys::TRANSCRIBE_OK || raw.is_null() {
            return Err(status_error("vad session init", status));
        }
        Ok(Self {
            raw,
            model: Arc::clone(model),
            info,
        })
    }

    /// Sample rate and frame size the model needs.
    pub fn info(&self) -> VadInfo {
        self.info
    }

    /// Feed audio and return the probability of the first completed frame, or `None` when the
    /// samples did not complete a frame. Callers feed exactly one frame per call, so at most one
    /// probability comes back; more than one is an error.
    pub fn feed_frame(&mut self, frame: &[f32]) -> Result<Option<f32>, NativeError> {
        check_finite(frame)?;
        let n = c_int::try_from(frame.len())
            .map_err(|_| NativeError::InvalidInput("frame is too long".to_owned()))?;
        let _compute = self.model.compute.lock();
        // SAFETY: the session is valid and used by this thread only; `frame` is valid for `n` floats.
        let status = unsafe { sys::transcribe_vad_stream_feed(self.raw, frame.as_ptr(), n) };
        if status != sys::TRANSCRIBE_OK {
            return Err(status_error("vad feed", status));
        }
        let mut result = std::mem::MaybeUninit::<sys::transcribe_vad_result>::uninit();
        // SAFETY: `transcribe_vad_result_init` fully initialises the struct.
        let mut result = unsafe {
            sys::transcribe_vad_result_init(result.as_mut_ptr());
            result.assume_init()
        };
        // SAFETY: the session is valid and `result` is initialised.
        let status = unsafe { sys::transcribe_vad_get_result(self.raw, &mut result) };
        if status != sys::TRANSCRIBE_OK {
            return Err(status_error("vad result", status));
        }
        if result.n_probs <= 0 {
            return Ok(None);
        }
        if result.n_probs > 1 {
            return Err(NativeError::InvalidInput(format!(
                "{} frames in one call",
                result.n_probs
            )));
        }
        // SAFETY: the session is valid; with `n_probs` above zero the pointer is non-null and
        // valid for `n_probs` floats until the next feed, and the value is copied at once.
        let probs = unsafe { sys::transcribe_vad_probs(self.raw) };
        if probs.is_null() {
            return Ok(None);
        }
        // SAFETY: `probs` is non-null and holds `n_probs` (one) float.
        Ok(Some(unsafe { *probs }))
    }

    /// Forget all history: the model state and the frame count.
    pub fn reset(&mut self) {
        let _compute = self.model.compute.lock();
        // SAFETY: the session is valid and used by this thread only.
        unsafe { sys::transcribe_vad_stream_reset(self.raw) };
    }
}

impl Drop for NativeVad {
    fn drop(&mut self) {
        // SAFETY: created by `transcribe_vad_session_init`, freed once; the model is still alive.
        unsafe { sys::transcribe_vad_session_free(self.raw) };
    }
}
