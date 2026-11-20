//! The safe wrapper over `prxvc-transcribe-sys`: the only module of the crate that uses `unsafe`
//! for the `transcribe.cpp` runtime (CS-2.10.1, pipeline-design PD-2.7.2). Compiled with the cargo
//! feature `transcribe`.
//!
//! # Ownership
//!
//! [`TranscribeModel`] owns a loaded `transcribe_model` behind an `Arc`; every session
//! ([`NativeRecognizer`], [`NativeVad`]) holds a clone, so "the model outlives its sessions" holds by
//! construction. `Drop` of a session frees it before the model can be freed, and `Drop` of the last
//! `Arc` frees the model, which is mandatory because the Metal backend aborts at exit while a model
//! is alive. No raw pointer leaves the module. (`std::mem::forget` would still leak a value: safe Rust
//! cannot prevent that and nothing here does it.)
//!
//! # Threads and callbacks
//!
//! The header allows at most one compute in flight per loaded model across all of its sessions, so
//! every call that computes takes the model's `compute` lock; a session is used through `&mut self`
//! and never by two threads at once. The abort callback runs on the thread that is inside the
//! call, under `catch_unwind`, and only reads a stack-local state that lives for the call; a panic is
//! counted ([`callback_panics`]) and ends the call with [`NativeError::CallbackPanicked`].

#![warn(clippy::undocumented_unsafe_blocks)]

mod model;
mod recognizer;
mod vad;

use std::ffi::{CStr, c_char, c_int};
use std::sync::atomic::{AtomicU64, Ordering};

use prxvc_transcribe_sys as sys;

pub use model::{Backend, TranscribeModel};
pub use recognizer::NativeRecognizer;
pub use vad::{NativeVad, VadInfo};

/// A failure of the native runtime.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NativeError {
    /// A call returned a non-zero status.
    #[error("{what}: {text} (status {status})")]
    Status {
        /// The call.
        what: &'static str,
        /// The runtime's status code.
        status: i32,
        /// The runtime's description of the code.
        text: String,
    },
    /// The call stopped because the abort callback asked it to.
    #[error("call aborted")]
    Aborted,
    /// The input cannot be passed to the runtime (an embedded NUL byte, a size over `i32`, a
    /// non-finite sample).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The model does not serve the role it was loaded for.
    #[error("{0}")]
    WrongRole(String),
    /// A native callback panicked; the panic was caught at the boundary.
    #[error("native callback panicked")]
    CallbackPanicked,
}

/// Native callbacks that panicked since the process started.
static CALLBACK_PANICS: AtomicU64 = AtomicU64::new(0);

/// How many native callbacks panicked and were caught since the process started.
pub fn callback_panics() -> u64 {
    CALLBACK_PANICS.load(Ordering::Relaxed)
}

pub(crate) fn count_panic() {
    CALLBACK_PANICS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn cstr_or_empty(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: the library returns NUL-terminated strings that stay valid until its next call on
        // the same object; the text is copied before any other call.
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}

pub(crate) fn status_error(what: &'static str, status: c_int) -> NativeError {
    if status == sys::TRANSCRIBE_ERR_ABORTED {
        return NativeError::Aborted;
    }
    NativeError::Status {
        what,
        status,
        // SAFETY: `transcribe_status_string` returns a static NUL-terminated string for any int.
        text: cstr_or_empty(unsafe { sys::transcribe_status_string(status) }),
    }
}

/// Build identity of the linked library, `"<version> (<commit>)"`.
pub fn runtime_version() -> String {
    // SAFETY: both functions return static NUL-terminated strings and are callable from any thread.
    unsafe {
        format!(
            "{} ({})",
            cstr_or_empty(sys::transcribe_version()),
            cstr_or_empty(sys::transcribe_version_commit())
        )
    }
}

/// Check that every sample is finite: the runtime rejects NaN and infinity.
pub(crate) fn check_finite(samples: &[f32]) -> Result<(), NativeError> {
    if samples.iter().all(|s| s.is_finite()) {
        Ok(())
    } else {
        Err(NativeError::InvalidInput(
            "audio holds a non-finite sample".to_owned(),
        ))
    }
}
