//! A loaded model, shared by the sessions made from it.

use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_transcribe_sys as sys;

use super::{NativeError, cstr_or_empty, status_error};

/// Where a model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The best device (Metal on Apple Silicon).
    Auto,
    /// The CPU only: for a model so small that GPU dispatch costs more than the compute, and to
    /// keep it off the GPU the other stages use.
    Cpu,
}

/// A loaded recognition or VAD model. Cheap to share: sessions keep an `Arc` of it.
#[derive(Debug)]
pub struct TranscribeModel {
    raw: *mut sys::transcribe_model,
    /// Held across every call that computes on this model (at most one in flight per model).
    pub(super) compute: Mutex<()>,
}

// SAFETY: the header lets several sessions on different threads share one model; the compute lock
// serialises the calls that compute, and the model is freed only in `Drop`, which needs exclusive
// ownership and so runs after every session (each holds an `Arc`) is gone.
unsafe impl Send for TranscribeModel {}
// SAFETY: see the `Send` impl; the model is read-only after load.
unsafe impl Sync for TranscribeModel {}

impl TranscribeModel {
    /// Load a GGUF model on the best backend (Metal on Apple Silicon).
    pub fn load(path: &Path) -> Result<Arc<Self>, NativeError> {
        Self::load_on(path, Backend::Auto)
    }

    /// Load a GGUF model on the given backend.
    pub fn load_on(path: &Path, backend: Backend) -> Result<Arc<Self>, NativeError> {
        let text = path
            .to_str()
            .ok_or_else(|| NativeError::InvalidInput(format!("{} is not UTF-8", path.display())))?;
        let c = CString::new(text)
            .map_err(|_| NativeError::InvalidInput("path contains a NUL byte".to_owned()))?;
        let mut params = std::mem::MaybeUninit::<sys::transcribe_model_load_params>::uninit();
        // SAFETY: `transcribe_model_load_params_init` fully initialises the struct.
        let mut params = unsafe {
            sys::transcribe_model_load_params_init(params.as_mut_ptr());
            params.assume_init()
        };
        params.backend = match backend {
            Backend::Auto => sys::TRANSCRIBE_BACKEND_AUTO,
            Backend::Cpu => sys::TRANSCRIBE_BACKEND_CPU,
        };
        let mut raw = std::ptr::null_mut();
        // SAFETY: `c` and `params` are alive for the call and `raw` is a valid out pointer.
        let status = unsafe { sys::transcribe_model_load_file(c.as_ptr(), &params, &mut raw) };
        if status == sys::TRANSCRIBE_OK && !raw.is_null() {
            Ok(Arc::new(Self {
                raw,
                compute: Mutex::new(()),
            }))
        } else {
            Err(status_error("model load", status))
        }
    }

    /// Name of the backend the model runs on (`"Metal"`, `"CPU"`).
    pub fn backend(&self) -> String {
        // SAFETY: the model is valid; the returned string is library-owned and copied at once.
        cstr_or_empty(unsafe { sys::transcribe_model_backend(self.raw) })
    }

    /// Whether the model can recognise speech.
    pub fn is_asr(&self) -> bool {
        self.roles() & sys::TRANSCRIBE_ROLE_ASR != 0
    }

    /// Whether the model can detect speech activity.
    pub fn is_vad(&self) -> bool {
        self.roles() & sys::TRANSCRIBE_ROLE_VAD != 0
    }

    fn roles(&self) -> u32 {
        // SAFETY: the model is valid for the call.
        unsafe { sys::transcribe_model_roles(self.raw) }
    }

    pub(super) fn raw(&self) -> *mut sys::transcribe_model {
        self.raw
    }
}

impl Drop for TranscribeModel {
    fn drop(&mut self) {
        // SAFETY: loaded by `transcribe_model_load_file`, freed once, after every session (each
        // holds an `Arc` of the model).
        unsafe { sys::transcribe_model_free(self.raw) };
    }
}
