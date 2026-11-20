//! Owning wrapper over the `qt_*` text-to-speech API, just enough for the link spike.
//!
//! `TtsEngine` owns the `qt_context` and frees it on drop (mandatory: the Metal backend aborts at
//! process exit while a context is alive). `Voice` owns prepared reference latents. Synthesis is
//! blocking and records when each streamed chunk arrived; the chunk and cancel callbacks run on
//! the library's compute worker thread and are wrapped in `catch_unwind` (CS-2.10.4).
#![warn(clippy::undocumented_unsafe_blocks)]

use crate::NativeError;
use prxvc_qwentts_sys as sys;
use std::ffi::{CStr, CString, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Output sample rate of the synthesizer.
pub const SAMPLE_RATE: u32 = 24_000;

/// Message of the last failing call on this thread, copied out.
fn last_error() -> NativeError {
    // SAFETY: `qt_last_error` returns a NUL-terminated string (or null) that stays valid until the
    // next failing call on this thread; it is copied before any other call.
    let msg = unsafe {
        let p = sys::qt_last_error();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    };
    NativeError(msg)
}

fn c_path(p: &Path) -> Result<CString, NativeError> {
    CString::new(p.to_string_lossy().into_owned())
        .map_err(|_| NativeError(format!("{}: path contains NUL", p.display())))
}

/// Build identity of the linked library, `"<short hash> (<date>)"`.
pub fn version() -> String {
    // SAFETY: `qt_version` returns a static NUL-terminated string and is callable from any thread.
    unsafe {
        CStr::from_ptr(sys::qt_version())
            .to_string_lossy()
            .into_owned()
    }
}

/// A loaded talker and codec.
#[derive(Debug)]
pub struct TtsEngine {
    ctx: *mut sys::qt_context,
}

// SAFETY: `qt_synthesize` and `qt_extract_voice_ref` are thread safe on one context (header contract:
// "qt_synthesize itself stays blocking and thread safe"); `qt_free` runs in `Drop`, which needs
// exclusive ownership.
unsafe impl Send for TtsEngine {}
// SAFETY: see the `Send` impl; all methods take `&self` and only call thread-safe entries.
unsafe impl Sync for TtsEngine {}

impl TtsEngine {
    /// Load the talker and codec GGUF files. `max_batch` above 1 lets concurrent synthesis calls
    /// share decode steps.
    pub fn load(talker: &Path, codec: &Path, max_batch: i32) -> Result<Self, NativeError> {
        let (talker_c, codec_c) = (c_path(talker)?, c_path(codec)?);
        let mut params = std::mem::MaybeUninit::<sys::qt_init_params>::uninit();
        // SAFETY: `qt_init_default_params` fully initialises the struct it is given.
        let mut params = unsafe {
            sys::qt_init_default_params(params.as_mut_ptr());
            params.assume_init()
        };
        params.talker_path = talker_c.as_ptr();
        params.codec_path = codec_c.as_ptr();
        params.max_batch = max_batch;
        // SAFETY: both path pointers outlive the call (the CStrings are alive until the end of this
        // function) and `qt_init` only reads them during the call.
        let ctx = unsafe { sys::qt_init(&params) };
        if ctx.is_null() {
            Err(last_error())
        } else {
            Ok(Self { ctx })
        }
    }

    /// Model type of the loaded talker (`"base"` for the voice-cloning models).
    pub fn model_type(&self) -> String {
        // SAFETY: the context is valid until drop; the returned string lives until `qt_free`.
        unsafe {
            CStr::from_ptr(sys::qt_model_type(self.ctx))
                .to_string_lossy()
                .into_owned()
        }
    }

    /// Prepare reusable voice latents from 24 kHz mono samples.
    pub fn prepare_voice(&self, samples_24k: &[f32]) -> Result<Voice, NativeError> {
        let n = c_int::try_from(samples_24k.len())
            .map_err(|_| NativeError("reference too long".into()))?;
        let mut raw = sys::qt_voice_ref::zeroed();
        // SAFETY: the context is valid, the sample slice is valid for `n` floats during the call,
        // and `raw` is a zero-initialised out struct the library fills with malloc-owned buffers.
        let st = unsafe { sys::qt_extract_voice_ref(self.ctx, samples_24k.as_ptr(), n, &mut raw) };
        if st == sys::QT_STATUS_OK {
            Ok(Voice { raw })
        } else {
            Err(last_error())
        }
    }

    /// Synthesize `text` in speaker-embedding mode with the given voice, streaming. `cancel` is polled
    /// by the library before every decode step; setting it makes the call return `CANCELLED`.
    pub fn synthesize(
        &self,
        voice: &Voice,
        req: &Request<'_>,
        cancel: Option<&AtomicBool>,
    ) -> Result<Synthesis, NativeError> {
        let text = CString::new(req.text).map_err(|_| NativeError("text contains NUL".into()))?;
        let lang =
            CString::new(req.language).map_err(|_| NativeError("language contains NUL".into()))?;
        let mut sink = Sink {
            start: Instant::now(),
            first: None,
            last: None,
            pcm: Vec::new(),
            chunks: 0,
            cancel,
        };
        let mut params = std::mem::MaybeUninit::<sys::qt_tts_params>::uninit();
        // SAFETY: `qt_tts_default_params` fully initialises the struct it is given.
        let mut params = unsafe {
            sys::qt_tts_default_params(params.as_mut_ptr());
            params.assume_init()
        };
        params.text = text.as_ptr();
        params.lang = lang.as_ptr();
        params.seed = req.seed;
        params.max_new_tokens = req.max_new_tokens;
        params.ref_spk_emb = voice.raw.ref_spk_emb;
        params.ref_spk_dim = voice.raw.ref_spk_dim;
        params.on_chunk = Some(on_chunk);
        params.on_chunk_user_data = ptr::from_mut(&mut sink).cast::<c_void>();
        params.cancel = Some(on_cancel);
        params.cancel_user_data = ptr::from_mut(&mut sink).cast::<c_void>();

        let mut out = sys::qt_audio::zeroed();
        sink.start = Instant::now();
        // SAFETY: the context is valid; `params` points at `text`, `lang` and the voice latents, all alive
        // for the call; `sink` lives on this stack frame and is touched only by the worker thread while
        // this blocking call is in flight and by this thread after it returns.
        let st = unsafe { sys::qt_synthesize(self.ctx, &params, &mut out) };
        let total = sink.start.elapsed();
        // SAFETY: `out` is zero-initialised or filled by the call; `qt_audio_free` handles both.
        unsafe { sys::qt_audio_free(&mut out) };
        match st {
            sys::QT_STATUS_OK | sys::QT_STATUS_CANCELLED => Ok(Synthesis {
                status: st,
                pcm: sink.pcm,
                chunks: sink.chunks,
                time_to_first_chunk: sink.first,
                time_to_last_chunk: sink.last,
                total,
            }),
            _ => Err(last_error()),
        }
    }
}

impl Drop for TtsEngine {
    fn drop(&mut self) {
        // SAFETY: the context came from `qt_init`, no call is in flight (`&mut self`), and it is freed once.
        unsafe { sys::qt_free(self.ctx) };
    }
}

/// Prepared reference-voice latents (speaker embedding and codes).
#[derive(Debug)]
pub struct Voice {
    raw: sys::qt_voice_ref,
}

// SAFETY: the latents are plain malloc buffers that are only read after creation.
unsafe impl Send for Voice {}
// SAFETY: see the `Send` impl; there is no interior mutability.
unsafe impl Sync for Voice {}

impl Voice {
    /// Length of the speaker embedding.
    pub fn embedding_dim(&self) -> i32 {
        self.raw.ref_spk_dim
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        // SAFETY: the buffers were allocated by `qt_extract_voice_ref` (or the struct is zeroed) and are freed once.
        unsafe { sys::qt_voice_ref_free(&mut self.raw) };
    }
}

/// One synthesis request.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Text to speak.
    pub text: &'a str,
    /// Language name (`"english"`, `"chinese"`).
    pub language: &'a str,
    /// Sampler seed.
    pub seed: i64,
    /// Generation cap in codec frames.
    pub max_new_tokens: i32,
}

/// Generation cap from the text length: expected duration at 0.28 s per English word, times 3,
/// in 80 ms frames, with a floor (voice-cloning design, requirement 1 of section 0.3).
pub fn max_new_tokens_for(text: &str) -> i32 {
    let words = text.split_whitespace().count().max(1) as f64;
    let frames = (words * 0.28 * 3.0 * 12.5).ceil() as i32;
    frames.max(40)
}

/// Result of a synthesis call.
#[derive(Debug)]
pub struct Synthesis {
    /// `QT_STATUS_OK` or `QT_STATUS_CANCELLED`.
    pub status: c_int,
    /// All streamed samples, 24 kHz mono.
    pub pcm: Vec<f32>,
    /// Number of chunk callbacks.
    pub chunks: usize,
    /// Call start to first chunk.
    pub time_to_first_chunk: Option<Duration>,
    /// Call start to last chunk.
    pub time_to_last_chunk: Option<Duration>,
    /// Call start to return.
    pub total: Duration,
}

impl Synthesis {
    /// Audio duration in seconds.
    pub fn audio_seconds(&self) -> f64 {
        self.pcm.len() as f64 / f64::from(SAMPLE_RATE)
    }

    /// Real-time factor: wall time over audio duration (0.2 means 5x faster than real time).
    pub fn rtf(&self) -> f64 {
        self.total.as_secs_f64() / self.audio_seconds().max(1e-9)
    }
}

/// State shared with the callbacks, owned by the `synthesize` stack frame.
struct Sink<'a> {
    start: Instant,
    first: Option<Duration>,
    last: Option<Duration>,
    pcm: Vec<f32>,
    chunks: usize,
    cancel: Option<&'a AtomicBool>,
}

/// Streaming chunk callback. Runs on the library's compute worker thread.
///
/// # Safety
/// `user` must point at a live `Sink` and `samples` at `n` floats, as the library guarantees.
unsafe extern "C" fn on_chunk(samples: *const f32, n: c_int, user: *mut c_void) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `user` is the `Sink` the caller of `synthesize` passed; it is alive for the whole
        // call and only this callback thread touches it during the call.
        let sink = unsafe { &mut *user.cast::<Sink<'_>>() };
        let at = sink.start.elapsed();
        sink.first.get_or_insert(at);
        sink.last = Some(at);
        sink.chunks += 1;
        let n = usize::try_from(n).unwrap_or(0);
        if !samples.is_null() && n > 0 {
            // SAFETY: the library passes `n` valid floats, valid for the duration of the callback.
            sink.pcm
                .extend_from_slice(unsafe { std::slice::from_raw_parts(samples, n) });
        }
        !sink.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }))
    .unwrap_or(false)
}

/// Cancel poll. Runs on the library's compute worker thread.
///
/// # Safety
/// `user` must point at a live `Sink`, as the library guarantees.
unsafe extern "C" fn on_cancel(user: *mut c_void) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: as in `on_chunk`; only the shared `cancel` flag is read here.
        let sink = unsafe { &*user.cast::<Sink<'_>>() };
        sink.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }))
    .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_scales_with_text_and_has_a_floor() {
        assert_eq!(max_new_tokens_for("hi"), 40);
        let ten = max_new_tokens_for("one two three four five six seven eight nine ten");
        assert_eq!(ten, 105);
    }
}
