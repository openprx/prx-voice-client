//! Raw bindings to the C API of the vendored transcribe.cpp (`transcribe.h`).
//!
//! Only what offline (segment) and streaming recognition and voice activity detection need is
//! declared: backend selection by the load parameters, model load, session init, a one-shot run
//! with its results, the streaming begin/feed/finalize cycle, an abort callback, a log callback,
//! the VAD session (info, streaming feed, probabilities, reset) and the ABI self-check. Every
//! function is `unsafe` to call; the safe wrapper lives in the adapters crate (CS-2.10.1). The
//! library is the dynamic library `libprxvc_transcribe.dylib`, built by `build.rs` with ggml
//! linked statically inside; it exports only the `transcribe_*` symbols.
//!
//! Compiled to nothing unless the `transcribe` feature is on.
//!
//! # Ownership and threading rules
//!
//! - Every params struct is created by its `*_init` function (which stamps `struct_size`) and
//!   then adjusted; a zeroed struct is rejected with `TRANSCRIBE_ERR_BAD_STRUCT_SIZE`. Null
//!   params mean library defaults.
//! - `transcribe_model_load_file` returns an owned model (`transcribe_model_free`, null is a
//!   no-op). A model must outlive every session made from it and must not be freed while any
//!   thread still uses it. Several sessions may share one model, each used by one thread at a
//!   time. A memory-mapped model file must not be truncated or rewritten in place while loaded.
//! - `transcribe_session_init` returns a session that borrows the model
//!   (`transcribe_session_free`, null is a no-op). `transcribe_open` is the convenience form whose
//!   session owns its model; it is not declared here.
//! - Audio is 16 kHz mono float32 in [-1, 1]; every sample must be finite. There is no resampler.
//! - Result strings (`transcribe_full_text`, segment text, detected language) are owned by the
//!   session and valid until the next run, stream call or free on that session: copy them out.
//! - The abort callback runs on the thread that is inside `transcribe_run` / `transcribe_stream_*`.
//!   It must not unwind (wrap the body in `catch_unwind`) and must not call into this API on the
//!   same session. The log callback may run on any library thread and must be reentrant.
//! - Metal compute on a model is serialised by ggml; two models or sessions in one process are
//!   fine, and so is a second runtime with its own ggml in the same process because this library
//!   exports no ggml symbols.
#![cfg(feature = "transcribe")]
#![allow(non_camel_case_types, non_snake_case)]
#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_float, c_int, c_void};

/// `transcribe_status` is an `int`; zero is success.
pub type transcribe_status = c_int;
/// Success.
pub const TRANSCRIBE_OK: transcribe_status = 0;
/// A parameter was invalid.
pub const TRANSCRIBE_ERR_INVALID_ARG: transcribe_status = 1;
/// The model file does not exist.
pub const TRANSCRIBE_ERR_FILE_NOT_FOUND: transcribe_status = 3;
/// Backend failure (recoverable by reloading on the CPU).
pub const TRANSCRIBE_ERR_BACKEND: transcribe_status = 8;
/// The abort callback stopped the run; partial results are readable.
pub const TRANSCRIBE_ERR_ABORTED: transcribe_status = 13;
/// A params struct had a zero or too small `struct_size`.
pub const TRANSCRIBE_ERR_BAD_STRUCT_SIZE: transcribe_status = 14;

/// `transcribe_backend_request`: pick the best device.
pub const TRANSCRIBE_BACKEND_AUTO: c_int = 0;
/// `transcribe_backend_request`: CPU only.
pub const TRANSCRIBE_BACKEND_CPU: c_int = 1;
/// `transcribe_backend_request`: Metal.
pub const TRANSCRIBE_BACKEND_METAL: c_int = 2;

/// `transcribe_abi_struct` id of the model load parameters (layout self-check).
pub const TRANSCRIBE_ABI_MODEL_LOAD_PARAMS: c_int = 0;
/// `transcribe_abi_struct` id of the session parameters.
pub const TRANSCRIBE_ABI_SESSION_PARAMS: c_int = 1;
/// `transcribe_abi_struct` id of the run parameters.
pub const TRANSCRIBE_ABI_RUN_PARAMS: c_int = 2;
/// `transcribe_abi_struct` id of the stream parameters.
pub const TRANSCRIBE_ABI_STREAM_PARAMS: c_int = 3;
/// `transcribe_abi_struct` id of the segment record.
pub const TRANSCRIBE_ABI_SEGMENT: c_int = 6;
/// `transcribe_abi_struct` id of the stream update record.
pub const TRANSCRIBE_ABI_STREAM_UPDATE: c_int = 9;
/// `transcribe_abi_struct` id of the stream text record.
pub const TRANSCRIBE_ABI_STREAM_TEXT: c_int = 10;

/// `transcribe_abi_struct` id of the VAD info record.
pub const TRANSCRIBE_ABI_VAD_INFO: c_int = 24;
/// `transcribe_abi_struct` id of the VAD session parameters.
pub const TRANSCRIBE_ABI_VAD_SESSION_PARAMS: c_int = 25;
/// `transcribe_abi_struct` id of the VAD run parameters.
pub const TRANSCRIBE_ABI_VAD_PARAMS: c_int = 26;
/// `transcribe_abi_struct` id of the VAD result record.
pub const TRANSCRIBE_ABI_VAD_RESULT: c_int = 27;

/// `transcribe_role` bit: the model can recognise speech.
pub const TRANSCRIBE_ROLE_ASR: u32 = 1 << 0;
/// `transcribe_role` bit: the model can detect speech activity.
pub const TRANSCRIBE_ROLE_VAD: u32 = 1 << 3;

/// `transcribe_stream_state`: no stream.
pub const TRANSCRIBE_STREAM_IDLE: c_int = 0;
/// `transcribe_stream_state`: a stream is running.
pub const TRANSCRIBE_STREAM_ACTIVE: c_int = 1;
/// `transcribe_stream_state`: the stream was finalized.
pub const TRANSCRIBE_STREAM_FINISHED: c_int = 2;
/// `transcribe_stream_state`: the stream failed.
pub const TRANSCRIBE_STREAM_FAILED: c_int = 3;

/// Opaque loaded model.
#[repr(C)]
pub struct transcribe_model {
    _private: [u8; 0],
}
/// Opaque recognition session.
#[repr(C)]
pub struct transcribe_session {
    _private: [u8; 0],
}
/// Opaque compute device.
#[repr(C)]
pub struct transcribe_device {
    _private: [u8; 0],
}
/// Opaque VAD session.
#[repr(C)]
pub struct transcribe_vad_session {
    _private: [u8; 0],
}
/// Opaque family extension block (not used here; always null).
#[repr(C)]
pub struct transcribe_ext {
    _private: [u8; 0],
}

/// `struct transcribe_model_load_params`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_model_load_params {
    /// Stamped by `transcribe_model_load_params_init`.
    pub struct_size: u64,
    /// `TRANSCRIBE_BACKEND_*`.
    pub backend: c_int,
    /// Explicit device, null for the backend default.
    pub device: *mut transcribe_device,
    /// Copy the weights instead of memory-mapping the file.
    pub no_mmap: bool,
}

/// `struct transcribe_session_params`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_session_params {
    /// Stamped by `transcribe_session_params_init`.
    pub struct_size: u64,
    /// CPU threads for ops that run on the CPU; 0 picks a default.
    pub n_threads: c_int,
    /// `transcribe_kv_type`.
    pub kv_type: c_int,
    /// Decoder context cap in tokens; 0 uses the model maximum.
    pub n_ctx: i32,
}

/// `struct transcribe_run_params`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_run_params {
    /// Stamped by `transcribe_run_params_init`.
    pub struct_size: u64,
    /// `transcribe_task` (0 transcribe, 1 translate, 2 instruct).
    pub task: c_int,
    /// `transcribe_timestamp_kind`.
    pub timestamps: c_int,
    /// `transcribe_pnc_mode`.
    pub pnc: c_int,
    /// `transcribe_itn_mode`.
    pub itn: c_int,
    /// `transcribe_diarize_mode`.
    pub diarize: c_int,
    /// Language code, null for auto.
    pub language: *const c_char,
    /// Target language for translation, null otherwise.
    pub target_language: *const c_char,
    /// Keep special tags in the text.
    pub keep_special_tags: bool,
    /// Family extension, null.
    pub family: *const transcribe_ext,
    /// Speculative decoding drafts.
    pub spec_k_drafts: i32,
    /// Context vocabulary list.
    pub vocabulary: *const *const c_char,
    /// Entries in `vocabulary`.
    pub n_vocabulary: i32,
    /// Context prompt.
    pub prompt: *const c_char,
    /// Transcript prefix to continue from.
    pub prefix: *const c_char,
}

/// `struct transcribe_segment`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_segment {
    /// Stamped by `transcribe_segment_init`.
    pub struct_size: u64,
    /// Start in milliseconds.
    pub t0_ms: i64,
    /// End in milliseconds.
    pub t1_ms: i64,
    /// Index of the first word.
    pub first_word: c_int,
    /// Number of words.
    pub n_words: c_int,
    /// Index of the first token.
    pub first_token: c_int,
    /// Number of tokens.
    pub n_tokens: c_int,
    /// Session-owned text.
    pub text: *const c_char,
    /// 1-based speaker id, 0 for none.
    pub speaker_id: i32,
}

/// `struct transcribe_stream_params`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_stream_params {
    /// Stamped by `transcribe_stream_params_init`.
    pub struct_size: u64,
    /// Family extension, null.
    pub family: *const transcribe_ext,
    /// `transcribe_stream_commit_policy`.
    pub commit_policy: c_int,
    /// Agreement count for the stable-prefix policy.
    pub stable_prefix_agreement_n: u32,
}

/// `struct transcribe_stream_update`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_stream_update {
    /// Stamped by `transcribe_stream_update_init`.
    pub struct_size: u64,
    /// The result text changed.
    pub result_changed: bool,
    /// This update is the final one.
    pub is_final: bool,
    /// Revision counter.
    pub revision: i32,
    /// Audio received so far, ms.
    pub input_received_ms: i64,
    /// Audio committed so far, ms.
    pub audio_committed_ms: i64,
    /// Audio buffered and not yet committed, ms.
    pub buffered_ms: i64,
    /// The committed text changed.
    pub committed_changed: bool,
    /// The tentative text changed.
    pub tentative_changed: bool,
}

/// `struct transcribe_stream_text`; the pointers are session-owned.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_stream_text {
    /// Stamped by `transcribe_stream_text_init`.
    pub struct_size: u64,
    /// Committed plus tentative text.
    pub full_text: *const c_char,
    /// Length of `full_text`.
    pub full_text_bytes: u64,
    /// Committed text.
    pub committed_text: *const c_char,
    /// Length of `committed_text`.
    pub committed_text_bytes: u64,
    /// Tentative text.
    pub tentative_text: *const c_char,
    /// Length of `tentative_text`.
    pub tentative_text_bytes: u64,
    /// Byte offset of the tentative part in the raw text.
    pub raw_tentative_start_bytes: u64,
}

/// `struct transcribe_vad_info`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_vad_info {
    /// Stamped by `transcribe_vad_info_init`.
    pub struct_size: u64,
    /// Required sample rate (16000).
    pub sample_rate: i32,
    /// Samples per frame (512).
    pub frame_samples: i32,
}

/// `struct transcribe_vad_session_params`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_vad_session_params {
    /// Stamped by `transcribe_vad_session_params_init`.
    pub struct_size: u64,
    /// Processing threads; 0 picks a default.
    pub n_threads: i32,
}

/// `struct transcribe_vad_result`.
#[repr(C)]
#[derive(Debug)]
pub struct transcribe_vad_result {
    /// Stamped by `transcribe_vad_result_init`.
    pub struct_size: u64,
    /// Probabilities returned by the last call.
    pub n_probs: i32,
    /// Index of the first returned frame, counted from stream start.
    pub first_frame: i64,
    /// Speech segments (always 0 after feed or flush).
    pub n_segments: i32,
}

/// Abort poll; true stops the run.
pub type transcribe_abort_callback = Option<unsafe extern "C" fn(user_data: *mut c_void) -> bool>;
/// Log callback; `level` is a `transcribe_log_level` (1 info, 2 warn, 3 error, 4 debug, 5 continuation).
pub type transcribe_log_callback =
    Option<unsafe extern "C" fn(level: c_int, msg: *const c_char, user_data: *mut c_void)>;

#[link(name = "prxvc_transcribe")]
unsafe extern "C" {
    /// Library version string, static.
    pub fn transcribe_version() -> *const c_char;
    /// Commit the library was built from, static.
    pub fn transcribe_version_commit() -> *const c_char;
    /// Static description of a status code.
    pub fn transcribe_status_string(status: c_int) -> *const c_char;
    /// `sizeof` of a public struct by `TRANSCRIBE_ABI_*` id, 0 if unknown.
    pub fn transcribe_abi_struct_size(which: c_int) -> usize;
    /// `alignof` of a public struct by `TRANSCRIBE_ABI_*` id, 0 if unknown.
    pub fn transcribe_abi_struct_align(which: c_int) -> usize;
    /// Install a process-wide log callback; null restores the default.
    pub fn transcribe_log_set(cb: transcribe_log_callback, userdata: *mut c_void);

    /// Fill load parameters with defaults.
    pub fn transcribe_model_load_params_init(params: *mut transcribe_model_load_params);
    /// Load a GGUF model; on success the caller owns `*out_model`.
    pub fn transcribe_model_load_file(
        path: *const c_char,
        params: *const transcribe_model_load_params,
        out_model: *mut *mut transcribe_model,
    ) -> transcribe_status;
    /// Free a model; null is a no-op.
    pub fn transcribe_model_free(model: *mut transcribe_model);
    /// Name of the backend a loaded model runs on (`"Metal"`, `"CPU"`, ...), library-owned.
    pub fn transcribe_model_backend(model: *const transcribe_model) -> *const c_char;

    /// Fill session parameters with defaults.
    pub fn transcribe_session_params_init(params: *mut transcribe_session_params);
    /// Create a session bound to `model`.
    pub fn transcribe_session_init(
        model: *mut transcribe_model,
        params: *const transcribe_session_params,
        out_session: *mut *mut transcribe_session,
    ) -> transcribe_status;
    /// Free a session; null is a no-op.
    pub fn transcribe_session_free(session: *mut transcribe_session);
    /// Install or clear the abort callback.
    pub fn transcribe_set_abort_callback(
        session: *mut transcribe_session,
        cb: transcribe_abort_callback,
        user_data: *mut c_void,
    );

    /// Fill run parameters with defaults.
    pub fn transcribe_run_params_init(params: *mut transcribe_run_params);
    /// Transcribe one buffer of 16 kHz mono float PCM.
    pub fn transcribe_run(
        session: *mut transcribe_session,
        pcm: *const c_float,
        n_samples: c_int,
        params: *const transcribe_run_params,
    ) -> transcribe_status;
    /// Full text of the last run, session-owned.
    pub fn transcribe_full_text(session: *const transcribe_session) -> *const c_char;
    /// Detected language code of the last run, session-owned, may be empty.
    pub fn transcribe_detected_language(session: *const transcribe_session) -> *const c_char;
    /// Number of segments of the last run.
    pub fn transcribe_n_segments(session: *const transcribe_session) -> c_int;
    /// Fill a segment record with defaults.
    pub fn transcribe_segment_init(out: *mut transcribe_segment);
    /// Read segment `i` of the last run.
    pub fn transcribe_get_segment(
        session: *const transcribe_session,
        i: c_int,
        out: *mut transcribe_segment,
    ) -> transcribe_status;

    /// Fill stream parameters with defaults.
    pub fn transcribe_stream_params_init(params: *mut transcribe_stream_params);
    /// Fill an update record with defaults.
    pub fn transcribe_stream_update_init(out: *mut transcribe_stream_update);
    /// Fill a text record with defaults.
    pub fn transcribe_stream_text_init(out: *mut transcribe_stream_text);
    /// Start a streaming run.
    pub fn transcribe_stream_begin(
        session: *mut transcribe_session,
        run_params: *const transcribe_run_params,
        stream_params: *const transcribe_stream_params,
    ) -> transcribe_status;
    /// Feed 16 kHz mono float PCM; `update` may be null.
    pub fn transcribe_stream_feed(
        session: *mut transcribe_session,
        pcm: *const c_float,
        n_samples: c_int,
        update: *mut transcribe_stream_update,
    ) -> transcribe_status;
    /// End the stream and commit the remaining text.
    pub fn transcribe_stream_finalize(
        session: *mut transcribe_session,
        update: *mut transcribe_stream_update,
    ) -> transcribe_status;
    /// Abandon a stream and return the session to idle.
    pub fn transcribe_stream_reset(session: *mut transcribe_session);
    /// Current stream text, session-owned.
    pub fn transcribe_stream_get_text(
        session: *const transcribe_session,
        out: *mut transcribe_stream_text,
    ) -> transcribe_status;
    /// `TRANSCRIBE_STREAM_*` state of the session.
    pub fn transcribe_stream_get_state(session: *const transcribe_session) -> c_int;

    /// Bit set of `TRANSCRIBE_ROLE_*` the model serves.
    pub fn transcribe_model_roles(model: *const transcribe_model) -> u32;

    /// Fill a VAD info record with defaults.
    pub fn transcribe_vad_info_init(out: *mut transcribe_vad_info);
    /// Fill VAD session parameters with defaults.
    pub fn transcribe_vad_session_params_init(params: *mut transcribe_vad_session_params);
    /// Fill a VAD result record with defaults.
    pub fn transcribe_vad_result_init(out: *mut transcribe_vad_result);
    /// Audio requirements of a VAD model.
    pub fn transcribe_vad_get_info(
        model: *const transcribe_model,
        out: *mut transcribe_vad_info,
    ) -> transcribe_status;
    /// Create a VAD session on `model`.
    pub fn transcribe_vad_session_init(
        model: *mut transcribe_model,
        params: *const transcribe_vad_session_params,
        out_session: *mut *mut transcribe_vad_session,
    ) -> transcribe_status;
    /// Free a VAD session; null is a no-op.
    pub fn transcribe_vad_session_free(session: *mut transcribe_vad_session);
    /// Process incoming audio; incomplete frames are kept for the next call.
    pub fn transcribe_vad_stream_feed(
        session: *mut transcribe_vad_session,
        pcm: *const c_float,
        n_samples: c_int,
    ) -> transcribe_status;
    /// Discard the stream and its state; null is a no-op.
    pub fn transcribe_vad_stream_reset(session: *mut transcribe_vad_session);
    /// Summary of the last feed.
    pub fn transcribe_vad_get_result(
        session: *const transcribe_vad_session,
        out: *mut transcribe_vad_result,
    ) -> transcribe_status;
    /// Probabilities of the last feed, session-owned, null when there are none.
    pub fn transcribe_vad_probs(session: *const transcribe_vad_session) -> *const c_float;
}
