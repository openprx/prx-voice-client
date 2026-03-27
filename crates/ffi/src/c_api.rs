//! C-compatible API wrappers for mobile platform integration.
//!
//! These functions use C-compatible types (pointers and integers) and are
//! suitable for calling from Swift (iOS) and Kotlin/JNI (Android).
//!
//! # Safety
//! All functions that take raw pointers require valid, non-null, UTF-8 string pointers.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Initialize the engine. Returns a JSON string (caller must free with [`prxvc_free_string`]).
///
/// # Safety
/// `config_json` must be a valid null-terminated UTF-8 string pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_initialize(config_json: *const c_char) -> *mut c_char {
    // SAFETY: caller guarantees a valid null-terminated UTF-8 pointer or null.
    let config = unsafe { c_str_to_rust(config_json) };

    match crate::ffi_initialize(&config) {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Shutdown the engine.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn prxvc_shutdown() -> i32 {
    match crate::ffi_shutdown() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Create a session. Returns JSON with session_id (caller must free with [`prxvc_free_string`]).
///
/// # Safety
/// `config_json` must be a valid null-terminated UTF-8 string pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_create_session(config_json: *const c_char) -> *mut c_char {
    // SAFETY: caller guarantees a valid null-terminated UTF-8 pointer or null.
    let config = unsafe { c_str_to_rust(config_json) };
    match crate::ffi_create_session(&config) {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Start a session. Returns JSON status (caller must free with [`prxvc_free_string`]).
///
/// # Safety
/// `session_id` must be a valid null-terminated UTF-8 string pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_start_session(session_id: *const c_char) -> *mut c_char {
    // SAFETY: caller guarantees a valid null-terminated UTF-8 pointer or null.
    let sid = unsafe { c_str_to_rust(session_id) };
    match crate::ffi_start_session(&sid) {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Stop a session. Returns JSON status (caller must free with [`prxvc_free_string`]).
///
/// # Safety
/// `session_id` must be a valid null-terminated UTF-8 string pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_stop_session(session_id: *const c_char) -> *mut c_char {
    // SAFETY: caller guarantees a valid null-terminated UTF-8 pointer or null.
    let sid = unsafe { c_str_to_rust(session_id) };
    match crate::ffi_stop_session(&sid) {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Interrupt playback. Returns JSON status (caller must free with [`prxvc_free_string`]).
///
/// # Safety
/// `session_id` must be a valid null-terminated UTF-8 string pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_interrupt(session_id: *const c_char) -> *mut c_char {
    // SAFETY: caller guarantees a valid null-terminated UTF-8 pointer or null.
    let sid = unsafe { c_str_to_rust(session_id) };
    match crate::ffi_interrupt(&sid) {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Poll events. Returns JSON array of events (caller must free with [`prxvc_free_string`]).
#[unsafe(no_mangle)]
pub extern "C" fn prxvc_poll_events() -> *mut c_char {
    match crate::ffi_poll_events() {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Get session state. Returns JSON (caller must free with [`prxvc_free_string`]).
///
/// # Safety
/// `session_id` must be a valid null-terminated UTF-8 string pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_get_state(session_id: *const c_char) -> *mut c_char {
    // SAFETY: caller guarantees a valid null-terminated UTF-8 pointer or null.
    let sid = unsafe { c_str_to_rust(session_id) };
    match crate::ffi_get_session_state(&sid) {
        Ok(result) => string_to_c(result),
        Err(e) => string_to_c(error_json(&e.to_string())),
    }
}

/// Free a string previously returned by `prxvc_*` functions.
///
/// # Safety
/// `ptr` must be a pointer returned by a `prxvc_*` function, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prxvc_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: ptr was allocated by CString::into_raw in string_to_c.
        let _ = unsafe { CString::from_raw(ptr) };
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a C string pointer to an owned Rust `String`.
///
/// Returns an empty string for null pointers or invalid UTF-8.
///
/// # Safety
/// `ptr` must be null or a valid null-terminated C string.
unsafe fn c_str_to_rust(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: caller guarantees ptr is a valid null-terminated string.
    match unsafe { CStr::from_ptr(ptr) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => String::new(),
    }
}

/// Convert a Rust `String` into a heap-allocated C string.
///
/// Returns null if the string contains interior NUL bytes.
fn string_to_c(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Build a minimal JSON error object from a message string.
fn error_json(msg: &str) -> String {
    format!(r#"{{"error":"{}"}}"#, msg.replace('"', "'"))
}
