//! Raw bindings to `libprxvc_apple_translation.dylib`, the Swift bridge to Apple's on-device
//! Translation framework (`native/prxvc_apple_translation.h`).
//!
//! Compiled to nothing unless the `apple-translation` feature is on. Every function is `unsafe`;
//! the safe wrapper lives in the adapters crate (CS-2.10.1).
//!
//! # Threading and ownership
//!
//! - Every function is thread-safe. `prxat_prepare`, `prxat_translate` and the availability and
//!   create calls BLOCK the calling thread: never call them from a device callback.
//! - Calls on one session are serialised. `prxat_cancel` may be called from any thread and makes
//!   the blocked call return [`PRXAT_CANCELLED`]. `prxat_destroy` must not race a running call.
//! - The framework delivers its work through the main dispatch queue: the process's main thread
//!   must run its run loop (an application does; a command-line tool or a test binary runs
//!   `prxat_run_main_loop` on its main thread and works on another thread). Otherwise the calls
//!   return [`PRXAT_NO_MAIN_LOOP`].
//! - A returned string is owned by the caller and released with `prxat_free`.
#![cfg(feature = "apple-translation")]
#![allow(non_camel_case_types)]

use std::ffi::c_char;

/// Success.
pub const PRXAT_OK: i32 = 0;
/// The system is older than macOS 26.
pub const PRXAT_OS_TOO_OLD: i32 = 1;
/// The pair is supported but its language pack is not downloaded.
pub const PRXAT_PACK_NOT_INSTALLED: i32 = 2;
/// The framework does not translate this pair.
pub const PRXAT_PAIR_UNSUPPORTED: i32 = 3;
/// No answer within the timeout.
pub const PRXAT_TIMEOUT: i32 = 4;
/// The call was cancelled with `prxat_cancel`.
pub const PRXAT_CANCELLED: i32 = 5;
/// Any other framework failure.
pub const PRXAT_FRAMEWORK_ERROR: i32 = 6;
/// A null or empty argument.
pub const PRXAT_INVALID_ARGUMENT: i32 = 7;
/// The main thread does not service the main dispatch queue.
pub const PRXAT_NO_MAIN_LOOP: i32 = 8;

/// The ABI version this binding was written against.
pub const PRXAT_ABI_VERSION: i32 = 1;

/// Default strategy of the framework.
pub const PRXAT_STRATEGY_DEFAULT: i32 = 0;
/// Low latency (macOS 26.4; older systems use the default).
pub const PRXAT_STRATEGY_LOW_LATENCY: i32 = 1;
/// High fidelity (macOS 26.4; older systems use the default).
pub const PRXAT_STRATEGY_HIGH_FIDELITY: i32 = 2;

/// An opaque session of one language pair.
#[repr(C)]
pub struct prxat_session {
    _private: [u8; 0],
}

// SAFETY of the declarations: they match `native/prxvc_apple_translation.h`.
unsafe extern "C" {
    /// ABI version of the library.
    pub fn prxat_abi_version() -> i32;
    /// `PRXAT_OK` when this system can run the framework.
    pub fn prxat_os_support() -> i32;
    /// Availability of the pair; `msg` (may be null) receives a description.
    pub fn prxat_availability(
        source: *const c_char,
        target: *const c_char,
        msg: *mut c_char,
        msg_cap: usize,
    ) -> i32;
    /// A session for an installed pair, or null with `*code` and `msg` set.
    pub fn prxat_create(
        source: *const c_char,
        target: *const c_char,
        strategy: i32,
        code: *mut i32,
        msg: *mut c_char,
        msg_cap: usize,
    ) -> *mut prxat_session;
    /// Load the pair's resources.
    pub fn prxat_prepare(
        session: *mut prxat_session,
        timeout_ms: i32,
        msg: *mut c_char,
        msg_cap: usize,
    ) -> i32;
    /// Translate one text; on success `*out` is a string to release with `prxat_free`.
    pub fn prxat_translate(
        session: *mut prxat_session,
        text: *const c_char,
        timeout_ms: i32,
        out: *mut *mut c_char,
        msg: *mut c_char,
        msg_cap: usize,
    ) -> i32;
    /// Make the blocked call of the session return `PRXAT_CANCELLED`.
    pub fn prxat_cancel(session: *mut prxat_session);
    /// Run the main run loop until `prxat_stop_main_loop` (main thread only).
    pub fn prxat_run_main_loop();
    /// Stop `prxat_run_main_loop`.
    pub fn prxat_stop_main_loop();
    /// Destroy a session.
    pub fn prxat_destroy(session: *mut prxat_session);
    /// Release a string returned by the library.
    pub fn prxat_free(text: *mut c_char);
}
