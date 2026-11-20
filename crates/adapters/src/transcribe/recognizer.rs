//! A recognition session: decode one buffer of 16 kHz mono float audio.

use std::ffi::{CString, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use prxvc_media::Lang;
use prxvc_transcribe_sys as sys;

use super::{NativeError, TranscribeModel, check_finite, count_panic, cstr_or_empty, status_error};

/// What the abort callback reaches through its `user_data`; lives on the stack of `decode`.
struct AbortState<'a> {
    stop: &'a dyn Fn() -> bool,
    panicked: AtomicBool,
}

/// Abort poll. Runs on the thread that is inside `transcribe_run`.
///
/// # Safety
/// `user` must point at the live `AbortState` of the call, as `decode` guarantees by clearing the
/// callback before the state goes out of scope.
unsafe extern "C" fn on_abort(user: *mut c_void) -> bool {
    // SAFETY: `user` is the pointer `decode` installed; the state outlives the blocking call and is
    // only read through a shared reference here.
    let state = unsafe { &*user.cast::<AbortState<'_>>() };
    catch_unwind(AssertUnwindSafe(|| (state.stop)())).unwrap_or_else(|_| {
        state.panicked.store(true, Ordering::Release);
        count_panic();
        true
    })
}

/// A recognition session on a model. One thread uses it at a time; it can move between threads.
#[derive(Debug)]
pub struct NativeRecognizer {
    raw: *mut sys::transcribe_session,
    model: Arc<TranscribeModel>,
    zh: CString,
    en: CString,
}

// SAFETY: a session is single-threaded-at-a-time, not thread-affine; every call takes `&mut self`.
unsafe impl Send for NativeRecognizer {}

impl NativeRecognizer {
    /// Create a session on a model that serves the recognition role.
    pub fn new(model: &Arc<TranscribeModel>) -> Result<Self, NativeError> {
        if !model.is_asr() {
            return Err(NativeError::WrongRole(
                "the model does not serve the recognition role".to_owned(),
            ));
        }
        let mut raw = std::ptr::null_mut();
        // SAFETY: the model is valid (the session keeps an `Arc` of it), null parameters mean
        // defaults and `raw` is a valid out pointer.
        let status =
            unsafe { sys::transcribe_session_init(model.raw(), std::ptr::null(), &mut raw) };
        if status != sys::TRANSCRIBE_OK || raw.is_null() {
            return Err(status_error("session init", status));
        }
        Ok(Self {
            raw,
            model: Arc::clone(model),
            zh: CString::new(Lang::Zh.code()).unwrap_or_default(),
            en: CString::new(Lang::En.code()).unwrap_or_default(),
        })
    }

    /// Decode `audio` (16 kHz mono, finite samples) with the language hint `lang`; `stop` is polled
    /// by the runtime while it computes and a true result ends the call with
    /// [`NativeError::Aborted`]. The text is trimmed.
    pub fn decode(
        &mut self,
        audio: &[f32],
        lang: Lang,
        stop: &dyn Fn() -> bool,
    ) -> Result<String, NativeError> {
        check_finite(audio)?;
        let n = c_int::try_from(audio.len())
            .map_err(|_| NativeError::InvalidInput("audio is too long".to_owned()))?;
        let mut params = std::mem::MaybeUninit::<sys::transcribe_run_params>::uninit();
        // SAFETY: `transcribe_run_params_init` fully initialises the struct.
        let mut params = unsafe {
            sys::transcribe_run_params_init(params.as_mut_ptr());
            params.assume_init()
        };
        params.language = match lang {
            Lang::Zh => self.zh.as_ptr(),
            Lang::En => self.en.as_ptr(),
        };
        let state = AbortState {
            stop,
            panicked: AtomicBool::new(false),
        };
        let user = std::ptr::from_ref(&state).cast_mut().cast::<c_void>();
        let _compute = self.model.compute.lock();
        // SAFETY: the session is valid; `state` lives on this stack frame and the callback is
        // cleared below before it goes out of scope.
        unsafe { sys::transcribe_set_abort_callback(self.raw, Some(on_abort), user) };
        // SAFETY: the session is valid and used by this thread only; `audio` is valid for `n`
        // floats and `params` (with the language string behind its pointer, owned by `self`)
        // outlives the call.
        let status = unsafe { sys::transcribe_run(self.raw, audio.as_ptr(), n, &params) };
        // SAFETY: clearing the callback leaves no pointer to `state` inside the session.
        unsafe { sys::transcribe_set_abort_callback(self.raw, None, std::ptr::null_mut()) };
        if state.panicked.load(Ordering::Acquire) {
            return Err(NativeError::CallbackPanicked);
        }
        if status != sys::TRANSCRIBE_OK {
            return Err(status_error("run", status));
        }
        // SAFETY: the session is valid; the text is session-owned and copied at once.
        let text = cstr_or_empty(unsafe { sys::transcribe_full_text(self.raw) });
        Ok(text.trim().to_owned())
    }
}

impl Drop for NativeRecognizer {
    fn drop(&mut self) {
        // SAFETY: created by `transcribe_session_init`, freed once; the model is still alive
        // (`self.model` drops after this).
        unsafe { sys::transcribe_session_free(self.raw) };
    }
}
