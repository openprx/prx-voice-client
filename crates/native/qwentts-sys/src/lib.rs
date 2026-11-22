//! Raw bindings to the C API of the vendored qwentts.cpp (`qwen.h`, ABI version 6).
//!
//! Only the surface the call-translation pipeline needs is declared: load a model and codec,
//! prepare a voice from a reference recording, synthesize with a streaming chunk callback and
//! a cancel callback, and free. Everything here is `unsafe` to call; the safe wrapper lives in
//! the adapters crate (CS-2.10.1). The library is the dynamic library `libprxvc_qwentts.dylib`,
//! built by `build.rs` with ggml linked statically inside; it exports only the `qt_*` symbols.
//!
//! Compiled to nothing unless the `qwentts` feature is on.
//!
//! # Ownership and threading rules
//!
//! - `qt_init` returns an owned `qt_context`; `qt_free` releases it and is mandatory before the
//!   process exits (the Metal backend aborts at exit while a device is still in use). `qt_free`
//!   on null is a no-op. The context must not be used after `qt_free`, and `qt_free` must not
//!   run while a `qt_synthesize` call on the same context is in flight.
//! - `qt_extract_voice_ref` fills a `qt_voice_ref` with malloc-owned buffers; release them with
//!   `qt_voice_ref_free`. The struct must be zero-initialised first and not reassigned without
//!   freeing. The latents are plain buffers: they may be reused for any number of
//!   `qt_synthesize` calls and across contexts, and must stay alive during each call that
//!   points at them.
//! - `qt_synthesize` is blocking and thread safe on one context. Every pointer inside
//!   `qt_tts_params` is only read during the call. In streaming mode (`on_chunk` set) the audio
//!   goes to the callback and `out` stays empty; `out` must still be released with
//!   `qt_audio_free` (safe on a zero-initialised struct).
//! - `on_chunk` and `cancel` run on the library's internal compute worker thread, never on the
//!   caller's thread. They must not unwind (wrap the body in `catch_unwind`), must not call back
//!   into the `qt_*` API, and must not block: a blocking callback stalls every batched request.
//!   The sample pointer given to `on_chunk` is valid only during the call.
//! - `qt_last_error` returns a thread-local message pointer valid until the next failing call on
//!   the same thread; copy it out at once. Strings passed in are NUL-terminated UTF-8 and only
//!   read during the call.
//! - `qt_version` and `qt_model_type` return strings owned by the library (`qt_model_type` until
//!   `qt_free`).
#![cfg(feature = "qwentts")]
#![allow(non_camel_case_types, non_snake_case)]
#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_float, c_int, c_void};

/// Struct layout version this crate was written against (`QT_ABI_VERSION` in `qwen.h`).
pub const QT_ABI_VERSION: c_int = 6;

/// Status codes (`enum qt_status`). Zero is success.
pub const QT_STATUS_OK: c_int = 0;
/// A parameter was invalid.
pub const QT_STATUS_INVALID_PARAMS: c_int = -1;
/// The parameters do not fit the loaded model type.
pub const QT_STATUS_MODE_INVALID: c_int = -2;
/// Generation failed.
pub const QT_STATUS_GENERATE_FAILED: c_int = -3;
/// Out of memory.
pub const QT_STATUS_OOM: c_int = -4;
/// Cancelled through the cancel or chunk callback.
pub const QT_STATUS_CANCELLED: c_int = -5;

/// Opaque model handle.
#[repr(C)]
pub struct qt_context {
    _private: [u8; 0],
}

/// Output buffer of a buffered synthesis (`struct qt_audio`).
#[repr(C)]
#[derive(Debug)]
pub struct qt_audio {
    /// Mono PCM, malloc-allocated by the library.
    pub samples: *mut c_float,
    /// Length in samples.
    pub n_samples: c_int,
    /// 24000.
    pub sample_rate: c_int,
    /// 1.
    pub channels: c_int,
}

/// Initialisation parameters (`struct qt_init_params`).
#[repr(C)]
#[derive(Debug)]
pub struct qt_init_params {
    /// Set by `qt_init_default_params`.
    pub abi_version: c_int,
    /// Talker GGUF path (required).
    pub talker_path: *const c_char,
    /// Codec GGUF path (required).
    pub codec_path: *const c_char,
    /// Fused flash attention.
    pub use_fa: bool,
    /// FP16 clamp for old CUDA targets.
    pub clamp_fp16: bool,
    /// Number of requests batched on the GPU; 0 and 1 mean a single sequence.
    pub max_batch: c_int,
    /// Chunk width of the buffered codec decode, in seconds; 0 selects the default.
    pub codec_chunk_sec: c_float,
    /// ggml device name (`MTL0`, `CPU`, ...), null for the best available.
    pub device: *const c_char,
}

/// Voice reference latents (`struct qt_voice_ref`), malloc-owned by the library.
#[repr(C)]
#[derive(Debug)]
pub struct qt_voice_ref {
    /// Speaker embedding, `ref_spk_dim` floats.
    pub ref_spk_emb: *mut c_float,
    /// Length of the speaker embedding.
    pub ref_spk_dim: c_int,
    /// RVQ codes, `[num_codebooks, ref_T]` row-major.
    pub ref_codes: *mut i32,
    /// Frames in `ref_codes`.
    pub ref_T: c_int,
    /// Codebooks in `ref_codes`.
    pub num_codebooks: c_int,
}

impl qt_voice_ref {
    /// The zero-initialised value the library expects before `qt_extract_voice_ref`.
    pub const fn zeroed() -> Self {
        Self {
            ref_spk_emb: std::ptr::null_mut(),
            ref_spk_dim: 0,
            ref_codes: std::ptr::null_mut(),
            ref_T: 0,
            num_codebooks: 0,
        }
    }
}

impl qt_audio {
    /// The zero-initialised value the library expects before `qt_synthesize`.
    pub const fn zeroed() -> Self {
        Self {
            samples: std::ptr::null_mut(),
            n_samples: 0,
            sample_rate: 0,
            channels: 0,
        }
    }
}

/// Cancellation poll, called on the worker thread before every decode step; true aborts.
pub type qt_cancel_cb = Option<unsafe extern "C" fn(user_data: *mut c_void) -> bool>;
/// Streaming output callback, called on the worker thread; returning false aborts.
pub type qt_audio_chunk_cb = Option<
    unsafe extern "C" fn(samples: *const c_float, n_samples: c_int, user_data: *mut c_void) -> bool,
>;

/// Synthesis parameters (`struct qt_tts_params`).
#[repr(C)]
#[derive(Debug)]
pub struct qt_tts_params {
    /// Set by `qt_tts_default_params`.
    pub abi_version: c_int,
    /// Input text (required).
    pub text: *const c_char,
    /// Language name (`"english"`, `"chinese"`, `"auto"`), null means auto.
    pub lang: *const c_char,
    /// Style instruction (voice-design models only).
    pub instruct: *const c_char,
    /// Named speaker (custom-voice models only).
    pub speaker: *const c_char,
    /// Raw reference audio, 24 kHz mono float (alternative to the latents below).
    pub ref_audio_24k: *const c_float,
    /// Samples in `ref_audio_24k`.
    pub ref_n_samples: c_int,
    /// Reference transcript (ICL mode).
    pub ref_text: *const c_char,
    /// RNG seed, -1 for a random one.
    pub seed: i64,
    /// Generation cap in codec frames (12.5 per second of audio).
    pub max_new_tokens: c_int,
    /// Talker temperature.
    pub temperature: c_float,
    /// Talker top-k.
    pub top_k: c_int,
    /// Talker top-p.
    pub top_p: c_float,
    /// Talker repetition penalty.
    pub repetition_penalty: c_float,
    /// Sub-talker temperature.
    pub subtalker_temperature: c_float,
    /// Sub-talker top-k.
    pub subtalker_top_k: c_int,
    /// Sub-talker top-p.
    pub subtalker_top_p: c_float,
    /// Debug tensor dump directory, null disables.
    pub dump_dir: *const c_char,
    /// Cancel poll, null disables.
    pub cancel: qt_cancel_cb,
    /// Passed to `cancel`.
    pub cancel_user_data: *mut c_void,
    /// Streaming chunk callback; null selects the buffered path.
    pub on_chunk: qt_audio_chunk_cb,
    /// Passed to `on_chunk`.
    pub on_chunk_user_data: *mut c_void,
    /// Pre-encoded speaker embedding (`qt_voice_ref::ref_spk_emb`).
    pub ref_spk_emb: *const c_float,
    /// Length of `ref_spk_emb`; must equal the talker hidden size.
    pub ref_spk_dim: c_int,
    /// Pre-encoded RVQ codes (`qt_voice_ref::ref_codes`), ICL mode with `ref_text`.
    pub ref_codes: *const i32,
    /// Frames in `ref_codes`.
    pub ref_T: c_int,
}

/// Log severity (`enum qt_log_level`).
pub type qt_log_level = c_int;
/// Log callback; may be called from any library thread and must be reentrant.
pub type qt_log_cb =
    Option<unsafe extern "C" fn(level: qt_log_level, msg: *const c_char, user_data: *mut c_void)>;

#[link(name = "prxvc_qwentts")]
unsafe extern "C" {
    /// Build identity string, `"<short hash> (<date>)"`; static.
    pub fn qt_version() -> *const c_char;
    /// Last error message of the calling thread.
    pub fn qt_last_error() -> *const c_char;
    /// Release a `qt_audio` buffer; safe on a zero-initialised struct.
    pub fn qt_audio_free(a: *mut qt_audio);
    /// Fill `p` with the default parameters (sets `abi_version`).
    pub fn qt_init_default_params(p: *mut qt_init_params);
    /// Load talker and codec; null on failure (see `qt_last_error`). Release with `qt_free`.
    pub fn qt_init(params: *const qt_init_params) -> *mut qt_context;
    /// Release a context; null is a no-op.
    pub fn qt_free(q: *mut qt_context);
    /// Prepare voice latents from 24 kHz mono float audio.
    pub fn qt_extract_voice_ref(
        q: *mut qt_context,
        ref_audio_24k: *const c_float,
        ref_n_samples: c_int,
        out: *mut qt_voice_ref,
    ) -> c_int;
    /// Release voice latents; safe on a zero-initialised struct.
    pub fn qt_voice_ref_free(r: *mut qt_voice_ref);
    /// Install a process-wide log callback; null restores stderr.
    pub fn qt_log_set(cb: qt_log_cb, user_data: *mut c_void);
    /// Fill `p` with the default synthesis parameters (sets `abi_version`).
    pub fn qt_tts_default_params(p: *mut qt_tts_params);
    /// Number of RVQ codebooks of the loaded codec.
    pub fn qt_num_codebooks(q: *const qt_context) -> c_int;
    /// Run a synthesis; blocking; see the module notes for the callbacks.
    pub fn qt_synthesize(
        q: *mut qt_context,
        params: *const qt_tts_params,
        out: *mut qt_audio,
    ) -> c_int;
    /// `"base"`, `"custom_voice"` or `"voice_design"`.
    pub fn qt_model_type(q: *const qt_context) -> *const c_char;
}
