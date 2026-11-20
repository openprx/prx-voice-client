//! The safe wrapper over `prxvc-qwentts-sys`: the only module of the crate that uses `unsafe`
//! (CS-2.10.1). Compiled with the cargo feature `qwentts`.
//!
//! # Ownership
//!
//! [`NativeEngine`] owns the `qt_context`; its `Drop` calls `qt_free`, which is mandatory because
//! the Metal backend aborts the process at exit while a context is alive. No raw pointer leaves the
//! module and there is no way to obtain the context except through `NativeEngine::load`. Prepared
//! voices are plain `Vec<f32>` copies of the library's buffers (the library's own buffers are freed
//! by a guard before `extract_voice` returns), so a voice cannot outlive or dangle from an engine,
//! and synthesis calls borrow nothing from the engine beyond `&self`. `QwenTts` holds the engine in
//! an `Arc` and drops its last reference only after its worker threads have ended, so `qt_free`
//! never runs while a call is in flight. (`std::mem::forget` on the engine would still leak it:
//! safe Rust cannot prevent that, and nothing here does it.)
//!
//! # Threads and callbacks
//!
//! `qt_synthesize` is blocking and thread safe on one context; with `max_batch` above one,
//! concurrent calls share decode steps. The chunk callback and the cancel callback are `extern "C"`
//! trampolines that run on the library's internal compute worker thread, never on the thread that
//! called `synthesize`. Both run under `catch_unwind` so a panic never unwinds into C; a panic is
//! counted ([`callback_panics`]) and ends the call with [`EngineError::CallbackPanicked`]. The
//! callbacks only call [`CallControl`], which is non-blocking by contract. Cancellation goes through
//! the cancel callback (measured 4.3 ms median from flag to return); the chunk callback always
//! returns true because aborting through it is about 100 ms slower.

#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use prxvc_media::Lang;
use prxvc_qwentts_sys as sys;

use super::adapter::{LoadError, QwenTts, QwenTtsConfig};
use super::seam::{CallControl, CallEnd, EngineError, SynthCall, SynthEngine};

/// Native callbacks that panicked since the process started (reported by health checks).
static CALLBACK_PANICS: AtomicU64 = AtomicU64::new(0);

/// How many native callbacks panicked and were caught since the process started.
pub fn callback_panics() -> u64 {
    CALLBACK_PANICS.load(Ordering::Relaxed)
}

/// Build identity of the linked library, `"<short hash> (<date>)"`.
pub fn runtime_version() -> String {
    // SAFETY: `qt_version` returns a static NUL-terminated string and is callable from any thread.
    unsafe { CStr::from_ptr(sys::qt_version()) }
        .to_string_lossy()
        .into_owned()
}

/// Message of the last failing call on this thread, copied out at once.
fn last_error() -> String {
    // SAFETY: `qt_last_error` returns null or a NUL-terminated string that stays valid until the
    // next failing call on this thread; it is copied before any other call.
    unsafe {
        let p = sys::qt_last_error();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

fn c_string(what: &str, value: &str) -> Result<CString, EngineError> {
    CString::new(value)
        .map_err(|_| EngineError::InvalidInput(format!("{what} contains a NUL byte")))
}

fn c_path(path: &Path) -> Result<CString, EngineError> {
    let text = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidInput(format!("{} is not UTF-8", path.display())))?;
    c_string("path", text)
}

fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "chinese",
        Lang::En => "english",
    }
}

/// A loaded talker and codec.
#[derive(Debug)]
pub struct NativeEngine {
    ctx: *mut sys::qt_context,
}

// SAFETY: the header contract makes `qt_synthesize` and `qt_extract_voice_ref` thread safe on one
// context, and `qt_free` runs only in `Drop`, which needs exclusive ownership of the value.
unsafe impl Send for NativeEngine {}
// SAFETY: see the `Send` impl; every method takes `&self` and calls only thread-safe entries.
unsafe impl Sync for NativeEngine {}

impl NativeEngine {
    /// Load the talker and codec GGUF files. `max_batch` is the number of requests batched on the
    /// GPU (two: one per call direction).
    pub fn load(talker: &Path, codec: &Path, max_batch: usize) -> Result<Self, EngineError> {
        let (talker_c, codec_c) = (c_path(talker)?, c_path(codec)?);
        let max_batch = c_int::try_from(max_batch)
            .map_err(|_| EngineError::InvalidInput("batch size is too large".to_owned()))?;
        let mut params = std::mem::MaybeUninit::<sys::qt_init_params>::uninit();
        // SAFETY: `qt_init_default_params` fully initialises the struct it is given.
        let mut params = unsafe {
            sys::qt_init_default_params(params.as_mut_ptr());
            params.assume_init()
        };
        params.talker_path = talker_c.as_ptr();
        params.codec_path = codec_c.as_ptr();
        params.max_batch = max_batch;
        // SAFETY: both path pointers outlive the call (the `CString`s live to the end of this
        // function) and `qt_init` only reads them during the call.
        let ctx = unsafe { sys::qt_init(&params) };
        if ctx.is_null() {
            return Err(EngineError::Load(last_error()));
        }
        let engine = Self { ctx };
        // From here `Drop` frees the context on every path.
        let kind = engine.model_type();
        if kind != "base" {
            return Err(EngineError::Load(format!(
                "the talker is a `{kind}` model; voice cloning needs a `base` model"
            )));
        }
        Ok(engine)
    }

    /// Model type of the loaded talker (`"base"` for the voice-cloning models).
    pub fn model_type(&self) -> String {
        // SAFETY: the context is valid until drop; the returned string lives until `qt_free`.
        unsafe { CStr::from_ptr(sys::qt_model_type(self.ctx)) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for NativeEngine {
    fn drop(&mut self) {
        // SAFETY: the context came from `qt_init`, no call is in flight (`&mut self`: every call
        // borrows the engine), and it is freed exactly once.
        unsafe { sys::qt_free(self.ctx) };
    }
}

/// Frees the library-owned buffers of a `qt_voice_ref` on every path.
struct VoiceRefGuard(sys::qt_voice_ref);

impl Drop for VoiceRefGuard {
    fn drop(&mut self) {
        // SAFETY: the struct is zeroed or was filled by `qt_extract_voice_ref`; the call handles both
        // and the buffers are freed once.
        unsafe { sys::qt_voice_ref_free(&mut self.0) };
    }
}

/// What the callbacks reach through the `user_data` pointer; lives on the stack of `synthesize`.
struct CallbackState<'a> {
    ctl: &'a dyn CallControl,
    panicked: AtomicBool,
}

impl CallbackState<'_> {
    fn guard<T>(&self, fallback: T, body: impl FnOnce() -> T) -> T {
        catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| {
            self.panicked.store(true, Ordering::Release);
            CALLBACK_PANICS.fetch_add(1, Ordering::Relaxed);
            fallback
        })
    }
}

/// Streaming chunk callback. Runs on the library's compute worker thread.
///
/// # Safety
/// `user` must point at the live `CallbackState` of the call and `samples` at `n` initialised floats
/// valid for the duration of the callback, as the library guarantees.
unsafe extern "C" fn on_chunk(samples: *const f32, n: c_int, user: *mut c_void) -> bool {
    // SAFETY: `user` is the pointer `synthesize` passed; the state outlives the blocking call and is
    // only read through a shared reference here.
    let state = unsafe { &*user.cast::<CallbackState<'_>>() };
    state.guard(false, || {
        let n = usize::try_from(n).unwrap_or(0);
        if !samples.is_null() && n > 0 {
            // SAFETY: the library passes `n` valid floats, valid for the duration of the callback.
            state
                .ctl
                .on_chunk(unsafe { std::slice::from_raw_parts(samples, n) });
        }
        // Never abort through the chunk callback: the cancel callback is faster.
        true
    })
}

/// Cancel poll. Runs on the library's compute worker thread.
///
/// # Safety
/// `user` must point at the live `CallbackState` of the call, as the library guarantees.
unsafe extern "C" fn on_cancel(user: *mut c_void) -> bool {
    // SAFETY: as in `on_chunk`.
    let state = unsafe { &*user.cast::<CallbackState<'_>>() };
    state.guard(true, || state.ctl.should_abort())
}

impl SynthEngine for NativeEngine {
    fn extract_voice(&self, samples_24k: &[f32]) -> Result<Vec<f32>, EngineError> {
        let n = c_int::try_from(samples_24k.len())
            .map_err(|_| EngineError::InvalidInput("reference is too long".to_owned()))?;
        let mut guard = VoiceRefGuard(sys::qt_voice_ref::zeroed());
        // SAFETY: the context is valid, the slice is valid for `n` floats during the call, and the
        // out struct is zero-initialised; the library fills it with malloc-owned buffers that the
        // guard releases.
        let status =
            unsafe { sys::qt_extract_voice_ref(self.ctx, samples_24k.as_ptr(), n, &mut guard.0) };
        if status != sys::QT_STATUS_OK {
            return Err(EngineError::Call {
                status,
                message: last_error(),
            });
        }
        let dim = usize::try_from(guard.0.ref_spk_dim).unwrap_or(0);
        if guard.0.ref_spk_emb.is_null() || dim == 0 {
            return Err(EngineError::Call {
                status,
                message: "the runtime returned no speaker embedding".to_owned(),
            });
        }
        // SAFETY: on success `ref_spk_emb` points at `ref_spk_dim` initialised floats owned by the
        // library until the guard drops; they are copied out here.
        Ok(unsafe { std::slice::from_raw_parts(guard.0.ref_spk_emb, dim) }.to_vec())
    }

    fn synthesize(
        &self,
        call: &SynthCall<'_>,
        ctl: &dyn CallControl,
    ) -> Result<CallEnd, EngineError> {
        let text = c_string("text", call.text)?;
        let lang = c_string("language", language_name(call.lang))?;
        let dim = c_int::try_from(call.speaker.len())
            .map_err(|_| EngineError::InvalidInput("speaker embedding is too long".to_owned()))?;
        let mut params = std::mem::MaybeUninit::<sys::qt_tts_params>::uninit();
        // SAFETY: `qt_tts_default_params` fully initialises the struct it is given.
        let mut params = unsafe {
            sys::qt_tts_default_params(params.as_mut_ptr());
            params.assume_init()
        };
        let state = CallbackState {
            ctl,
            panicked: AtomicBool::new(false),
        };
        let user = std::ptr::from_ref(&state).cast_mut().cast::<c_void>();
        params.text = text.as_ptr();
        params.lang = lang.as_ptr();
        params.seed = call.seed;
        params.max_new_tokens = c_int::try_from(call.max_new_tokens).unwrap_or(c_int::MAX);
        params.ref_spk_emb = call.speaker.as_ptr();
        params.ref_spk_dim = dim;
        params.on_chunk = Some(on_chunk);
        params.on_chunk_user_data = user;
        params.cancel = Some(on_cancel);
        params.cancel_user_data = user;

        let mut out = sys::qt_audio::zeroed();
        // SAFETY: the context is valid; `params` points at `text`, `lang` and the speaker slice, all
        // alive for the call; `state` lives on this stack frame for the whole blocking call and is
        // only read (through a shared reference) by the callbacks, which `catch_unwind` guards.
        let status = unsafe { sys::qt_synthesize(self.ctx, &params, &mut out) };
        // SAFETY: `out` is zero-initialised or filled by the call; `qt_audio_free` handles both
        // (in streaming mode it stays empty).
        unsafe { sys::qt_audio_free(&mut out) };
        if state.panicked.load(Ordering::Acquire) {
            return Err(EngineError::CallbackPanicked);
        }
        match status {
            sys::QT_STATUS_OK => Ok(CallEnd::Completed),
            sys::QT_STATUS_CANCELLED => Ok(CallEnd::Aborted),
            status => Err(EngineError::Call {
                status,
                message: last_error(),
            }),
        }
    }
}

impl QwenTts {
    /// Load the talker and codec models, start the workers and run the warm-up.
    ///
    /// `cfg.workers` is the engine's batch size. Blocks for the model load (0.5 to 0.9 s once the
    /// system has compiled the Metal pipelines, several seconds on the first launch after a rebuild)
    /// and the warm-up; call it from a loading thread.
    pub fn load_native(talker: &Path, codec: &Path, cfg: QwenTtsConfig) -> Result<Self, LoadError> {
        let engine = NativeEngine::load(talker, codec, cfg.workers)?;
        Self::start(std::sync::Arc::new(engine), cfg)
    }
}
