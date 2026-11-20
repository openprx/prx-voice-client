//! Owning wrapper over the `transcribe_*` recognition API, just enough for the link spike.
//!
//! `AsrModel` owns a loaded model and `AsrSession` a session that keeps the model alive through an
//! `Arc`, so the C rule "the model outlives every session" holds by construction. Audio is 16 kHz
//! mono float.
#![warn(clippy::undocumented_unsafe_blocks)]

use crate::NativeError;
use prxvc_transcribe_sys as sys;
use std::ffi::{CStr, CString, c_int};
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Input sample rate of the recognizer.
pub const SAMPLE_RATE: u32 = 16_000;

fn status_error(what: &str, status: c_int) -> NativeError {
    // SAFETY: `transcribe_status_string` returns a static NUL-terminated string for any int.
    let text = unsafe {
        CStr::from_ptr(sys::transcribe_status_string(status))
            .to_string_lossy()
            .into_owned()
    };
    NativeError(format!("{what}: {text} ({status})"))
}

fn cstr_or_empty(p: *const std::ffi::c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: the library returns NUL-terminated strings; the caller copies before the next call.
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}

/// Library version and commit as `"<version> (<commit>)"`.
pub fn version() -> String {
    // SAFETY: both functions return static NUL-terminated strings.
    unsafe {
        format!(
            "{} ({})",
            cstr_or_empty(sys::transcribe_version()),
            cstr_or_empty(sys::transcribe_version_commit())
        )
    }
}

/// A loaded recognition model.
#[derive(Debug)]
pub struct AsrModel {
    raw: *mut sys::transcribe_model,
}

// SAFETY: a model may be shared by several sessions on different threads (header contract); it is
// freed only in `Drop`, after every session (each holds an `Arc`) is gone.
unsafe impl Send for AsrModel {}
// SAFETY: see the `Send` impl; the model is read-only after load.
unsafe impl Sync for AsrModel {}

impl AsrModel {
    /// Load a GGUF model on the best backend (Metal on Apple Silicon).
    pub fn load(path: &Path) -> Result<Arc<Self>, NativeError> {
        let c = CString::new(path.to_string_lossy().into_owned())
            .map_err(|_| NativeError("path contains NUL".into()))?;
        let mut params = std::mem::MaybeUninit::<sys::transcribe_model_load_params>::uninit();
        // SAFETY: `transcribe_model_load_params_init` fully initialises the struct.
        let params = unsafe {
            sys::transcribe_model_load_params_init(params.as_mut_ptr());
            params.assume_init()
        };
        let mut raw = ptr::null_mut();
        // SAFETY: `c` and `params` are alive for the call; `raw` is a valid out pointer.
        let st = unsafe { sys::transcribe_model_load_file(c.as_ptr(), &params, &mut raw) };
        if st == sys::TRANSCRIBE_OK && !raw.is_null() {
            Ok(Arc::new(Self { raw }))
        } else {
            Err(status_error("model load", st))
        }
    }

    /// Name of the backend the model runs on.
    pub fn backend(&self) -> String {
        // SAFETY: the model is valid; the returned string is library-owned and copied at once.
        cstr_or_empty(unsafe { sys::transcribe_model_backend(self.raw) })
    }
}

impl Drop for AsrModel {
    fn drop(&mut self) {
        // SAFETY: loaded by `transcribe_model_load_file`, freed once, after all sessions (they hold an `Arc`).
        unsafe { sys::transcribe_model_free(self.raw) };
    }
}

/// A recognition session. One thread uses it at a time; it can move between threads.
#[derive(Debug)]
pub struct AsrSession {
    raw: *mut sys::transcribe_session,
    _model: Arc<AsrModel>,
}

// SAFETY: a session is single-threaded-at-a-time, not thread-affine; `&mut self` on every call enforces that.
unsafe impl Send for AsrSession {}

/// Result of a streaming recognition.
#[derive(Debug)]
pub struct StreamResult {
    /// Final text after `finalize`.
    pub text: String,
    /// Index of the first fed chunk after which any text (committed or tentative) existed.
    pub first_text_chunk: Option<usize>,
    /// Wall time of the whole begin/feed/finalize cycle.
    pub elapsed: Duration,
}

impl AsrSession {
    /// Create a session on a loaded model.
    pub fn new(model: &Arc<AsrModel>) -> Result<Self, NativeError> {
        let mut raw = ptr::null_mut();
        // SAFETY: the model is valid (kept alive by the `Arc` stored in the session), null params mean
        // defaults, and `raw` is a valid out pointer.
        let st = unsafe { sys::transcribe_session_init(model.raw, ptr::null(), &mut raw) };
        if st == sys::TRANSCRIBE_OK && !raw.is_null() {
            Ok(Self {
                raw,
                _model: Arc::clone(model),
            })
        } else {
            Err(status_error("session init", st))
        }
    }

    fn run_params(language: Option<&CString>) -> sys::transcribe_run_params {
        let mut p = std::mem::MaybeUninit::<sys::transcribe_run_params>::uninit();
        // SAFETY: `transcribe_run_params_init` fully initialises the struct.
        let mut p = unsafe {
            sys::transcribe_run_params_init(p.as_mut_ptr());
            p.assume_init()
        };
        if let Some(l) = language {
            p.language = l.as_ptr();
        }
        p
    }

    /// Recognise one buffer of 16 kHz mono float samples. `language` is a code such as `"en"`, or `None` for auto.
    pub fn run(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String, NativeError> {
        let lang = language
            .map(CString::new)
            .transpose()
            .map_err(|_| NativeError("language contains NUL".into()))?;
        let params = Self::run_params(lang.as_ref());
        let n = c_int::try_from(pcm.len()).map_err(|_| NativeError("audio too long".into()))?;
        // SAFETY: the session is valid and used by this thread only; `pcm` is valid for `n` floats and
        // `params` (with `lang` behind its pointer) outlives the call.
        let st = unsafe { sys::transcribe_run(self.raw, pcm.as_ptr(), n, &params) };
        if st != sys::TRANSCRIBE_OK {
            return Err(status_error("run", st));
        }
        // SAFETY: the session is valid; the text is session-owned and copied at once.
        Ok(
            cstr_or_empty(unsafe { sys::transcribe_full_text(self.raw) })
                .trim()
                .to_string(),
        )
    }

    /// Recognise `pcm` through the streaming API, feeding `chunk` samples at a time.
    pub fn run_streaming(
        &mut self,
        pcm: &[f32],
        chunk: usize,
        language: Option<&str>,
    ) -> Result<StreamResult, NativeError> {
        let lang = language
            .map(CString::new)
            .transpose()
            .map_err(|_| NativeError("language contains NUL".into()))?;
        let params = Self::run_params(lang.as_ref());
        let started = Instant::now();
        // SAFETY: the session is valid; `params` is alive for the call; null stream params mean defaults.
        let st = unsafe { sys::transcribe_stream_begin(self.raw, &params, ptr::null()) };
        if st != sys::TRANSCRIBE_OK {
            return Err(status_error("stream begin", st));
        }
        let mut first_text_chunk = None;
        for (i, part) in pcm.chunks(chunk.max(1)).enumerate() {
            let n =
                c_int::try_from(part.len()).map_err(|_| NativeError("chunk too long".into()))?;
            // SAFETY: the session is valid and in an active stream; `part` is valid for `n` floats; a null
            // update pointer is allowed.
            let st =
                unsafe { sys::transcribe_stream_feed(self.raw, part.as_ptr(), n, ptr::null_mut()) };
            if st != sys::TRANSCRIBE_OK {
                // SAFETY: abandoning the stream on an error leaves the session idle.
                unsafe { sys::transcribe_stream_reset(self.raw) };
                return Err(status_error("stream feed", st));
            }
            if first_text_chunk.is_none() && !self.stream_text()?.is_empty() {
                first_text_chunk = Some(i);
            }
        }
        // SAFETY: the session is valid and in an active stream; a null update pointer is allowed.
        let st = unsafe { sys::transcribe_stream_finalize(self.raw, ptr::null_mut()) };
        if st != sys::TRANSCRIBE_OK {
            return Err(status_error("stream finalize", st));
        }
        let text = self.stream_text()?;
        Ok(StreamResult {
            text,
            first_text_chunk,
            elapsed: started.elapsed(),
        })
    }

    fn stream_text(&self) -> Result<String, NativeError> {
        let mut out = std::mem::MaybeUninit::<sys::transcribe_stream_text>::uninit();
        // SAFETY: `transcribe_stream_text_init` fully initialises the struct.
        let mut out = unsafe {
            sys::transcribe_stream_text_init(out.as_mut_ptr());
            out.assume_init()
        };
        // SAFETY: the session is valid; `out` is initialised; the pointers it receives are session-owned
        // and copied before the next call.
        let st = unsafe { sys::transcribe_stream_get_text(self.raw, &mut out) };
        if st != sys::TRANSCRIBE_OK {
            return Err(status_error("stream text", st));
        }
        Ok(cstr_or_empty(out.full_text).trim().to_string())
    }
}

impl Drop for AsrSession {
    fn drop(&mut self) {
        // SAFETY: created by `transcribe_session_init`, freed once; the model is still alive (`_model` drops after this).
        unsafe { sys::transcribe_session_free(self.raw) };
    }
}
