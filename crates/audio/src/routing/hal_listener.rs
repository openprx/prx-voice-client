//! Listener for default-device and device-list changes of the Core Audio HAL (macOS): safe wrapper over `native/prxvc_defaults.m`.
//!
//! The native callback runs on a private serial dispatch queue (not the main
//! thread, not a real-time thread). It is an `extern "C"` trampoline that
//! catches panics and does one non-blocking [`ChangeSink::post`]; what changed
//! is read later by the poller (CS-2.10.4).

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use super::changes::{ChangeSink, Notice};

const CHANGE_DEFAULT_INPUT: i32 = 1;
const CHANGE_DEFAULT_OUTPUT: i32 = 2;
const CHANGE_DEVICE_LIST: i32 = 3;

type ChangeCb = extern "C" fn(kind: i32, ctx: *mut c_void);

unsafe extern "C" {
    fn prxvc_change_listener_start(cb: ChangeCb, ctx: *mut c_void, status: *mut i32)
    -> *mut c_void;
    fn prxvc_change_listener_stop(listener: *mut c_void);
}

extern "C" fn on_change(kind: i32, ctx: *mut c_void) {
    // A panic must never unwind into C.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `ctx` is the pointer from `Arc::into_raw` in `start`; the listener's
        // `Drop` stops the callbacks before it releases that `Arc`.
        let sink = unsafe { &*(ctx as *const ChangeSink) };
        match kind {
            CHANGE_DEFAULT_INPUT => sink.post(Notice::DefaultInput),
            CHANGE_DEFAULT_OUTPUT => sink.post(Notice::DefaultOutput),
            CHANGE_DEVICE_LIST => sink.post(Notice::DeviceList),
            _ => {}
        }
    }));
}

/// Running listener; dropping it stops the notifications.
pub struct HalListener {
    handle: *mut c_void,
    ctx: *const ChangeSink,
}

// SAFETY: the handle is an opaque pointer only passed to the shim's stop function from
// `Drop`, and the context is an `Arc<ChangeSink>` whose target is `Send + Sync`.
unsafe impl Send for HalListener {}

impl HalListener {
    /// Start listening; notices go to `sink`.
    ///
    /// # Errors
    /// The Core Audio status when the listeners could not be installed.
    pub fn start(sink: Arc<ChangeSink>) -> Result<Self, i32> {
        let ctx = Arc::into_raw(sink);
        let mut status = 0_i32;
        // SAFETY: `on_change` is a valid callback, `ctx` stays valid until `Drop`, and `status`
        // is a valid out-pointer for the call.
        let handle =
            unsafe { prxvc_change_listener_start(on_change, ctx as *mut c_void, &mut status) };
        if handle.is_null() {
            // SAFETY: the shim kept no reference to `ctx` when it failed.
            drop(unsafe { Arc::from_raw(ctx) });
            return Err(status);
        }
        Ok(Self { handle, ctx })
    }
}

impl Drop for HalListener {
    fn drop(&mut self) {
        // SAFETY: `handle` came from `start`. The stop call returns only when no callback is
        // running or will run, after which the context can be released.
        unsafe {
            prxvc_change_listener_stop(self.handle);
            drop(Arc::from_raw(self.ctx));
        }
    }
}
