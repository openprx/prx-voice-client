//! System sleep and wake notifications (macOS): safe wrapper over `native/prxvc_power.m`.
//!
//! IOKit's `IORegisterForSystemPower` with a dispatch queue was chosen over
//! `NSWorkspace` notifications because it needs neither a run loop nor the main
//! thread, so it works the same in the application and in a command-line tool.
//! The native callback runs on a private serial dispatch queue; this trampoline
//! catches panics and does one non-blocking post (CS-2.10.4). The shim never
//! vetoes sleep and acknowledges it right after the post.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use super::changes::{ChangeSink, Notice};

const POWER_WILL_SLEEP: i32 = 1;
const POWER_DID_WAKE: i32 = 2;

type PowerCb = extern "C" fn(kind: i32, ctx: *mut c_void);

unsafe extern "C" {
    fn prxvc_power_start(cb: PowerCb, ctx: *mut c_void, status: *mut i32) -> *mut c_void;
    fn prxvc_power_stop(monitor: *mut c_void);
}

extern "C" fn on_power(kind: i32, ctx: *mut c_void) {
    // A panic must never unwind into C.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `ctx` is the pointer from `Arc::into_raw` in `start`; `Drop` stops the
        // callbacks before it releases that `Arc`.
        let sink = unsafe { &*(ctx as *const ChangeSink) };
        match kind {
            POWER_WILL_SLEEP => sink.post(Notice::Sleep),
            POWER_DID_WAKE => sink.post(Notice::Wake),
            _ => {}
        }
    }));
}

/// Running monitor; dropping it stops the notifications.
pub struct PowerMonitor {
    handle: *mut c_void,
    ctx: *const ChangeSink,
}

// SAFETY: the handle is an opaque pointer only passed to the shim's stop function from
// `Drop`, and the context is an `Arc<ChangeSink>` whose target is `Send + Sync`.
unsafe impl Send for PowerMonitor {}

impl PowerMonitor {
    /// Start the monitor; notices go to `sink`.
    ///
    /// # Errors
    /// The shim's status when the registration failed.
    pub fn start(sink: Arc<ChangeSink>) -> Result<Self, i32> {
        let ctx = Arc::into_raw(sink);
        let mut status = 0_i32;
        // SAFETY: `on_power` is a valid callback, `ctx` stays valid until `Drop`, and `status`
        // is a valid out-pointer for the call.
        let handle = unsafe { prxvc_power_start(on_power, ctx as *mut c_void, &mut status) };
        if handle.is_null() {
            // SAFETY: the shim kept no reference to `ctx` when it failed.
            drop(unsafe { Arc::from_raw(ctx) });
            return Err(status);
        }
        Ok(Self { handle, ctx })
    }
}

impl Drop for PowerMonitor {
    fn drop(&mut self) {
        // SAFETY: `handle` came from `start`. The stop call returns only when no callback is
        // running or will run, after which the context can be released.
        unsafe {
            prxvc_power_stop(self.handle);
            drop(Arc::from_raw(self.ctx));
        }
    }
}
