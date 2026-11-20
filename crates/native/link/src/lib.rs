//! Link spike for the native runtimes (work package CT-9).
//!
//! This crate exists to prove, in a test binary, that `qwentts.cpp` and `transcribe.cpp`
//! (each a dynamic library with its own statically linked ggml) can be loaded into one process,
//! run concurrently on Metal and exit cleanly. The adapters of later packages replace the thin
//! wrappers here.
//!
//! Modules:
//! - [`wav`]: minimal WAV reader for the reference recordings.
//! - [`resample`]: sample-rate conversion (24 kHz synthesis output to 16 kHz recognition input).
//! - [`text`]: transcript normalisation and word error rate for the round-trip checks.
//! - `tts` (feature `qwentts`): owning wrapper over the `qt_*` API.
//! - `asr` (feature `transcribe`): owning wrapper over the `transcribe_*` API.
//! - [`models`]: locations of the benchmark models and references used by the ignored tests.

#[cfg(feature = "transcribe")]
pub mod asr;
pub mod models;
pub mod resample;
pub mod text;
#[cfg(feature = "qwentts")]
pub mod tts;
pub mod wav;

/// Error of the native wrappers: the library's own message, copied out at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError(pub String);

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NativeError {}
