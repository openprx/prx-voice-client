//! The real `SystemTranslationApi`: the Swift bridge of `prxvc-apple-translation-sys`.
//!
//! Every call into the library is blocking and thread-safe (see the sys crate's threading rules);
//! none of them may run in a device callback, and this module is only used from the request
//! threads of `AppleSystemMt` and from engine construction.

use std::ffi::{CStr, CString, c_char};
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use prxvc_apple_translation_sys as sys;

use super::api::{Availability, Strategy, SystemSession, SystemTranslationApi};
use super::fault::{SystemFault, SystemFaultKind};

const MESSAGE_CAP: usize = 512;

/// The operating system's translation framework through the Swift bridge.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppleFramework;

fn fault_of(code: i32, message: &[c_char; MESSAGE_CAP]) -> SystemFault {
    // SAFETY: the library NUL terminates every message it writes into the buffer, and the buffer
    // starts zeroed, so it holds a terminator in every case.
    let message = unsafe { CStr::from_ptr(message.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let kind = match code {
        sys::PRXAT_OS_TOO_OLD => SystemFaultKind::OsTooOld,
        sys::PRXAT_PACK_NOT_INSTALLED => SystemFaultKind::PackNotInstalled,
        sys::PRXAT_PAIR_UNSUPPORTED => SystemFaultKind::PairUnsupported,
        sys::PRXAT_TIMEOUT => SystemFaultKind::Timeout,
        sys::PRXAT_CANCELLED => SystemFaultKind::Cancelled,
        sys::PRXAT_NO_MAIN_LOOP => SystemFaultKind::NoMainLoop,
        _ => SystemFaultKind::Framework,
    };
    SystemFault::new(kind, message)
}

fn c_string(text: &str) -> Result<CString, SystemFault> {
    CString::new(text).map_err(|_| {
        SystemFault::new(
            SystemFaultKind::Framework,
            "the text contains a NUL character",
        )
    })
}

fn timeout_ms(timeout: Duration) -> i32 {
    i32::try_from(timeout.as_millis())
        .unwrap_or(i32::MAX)
        .max(1)
}

impl SystemTranslationApi for AppleFramework {
    fn os_support(&self) -> Result<(), SystemFault> {
        // SAFETY: plain calls without arguments.
        let (abi, os) = unsafe { (sys::prxat_abi_version(), sys::prxat_os_support()) };
        if abi != sys::PRXAT_ABI_VERSION {
            return Err(SystemFault::new(
                SystemFaultKind::Framework,
                format!(
                    "the bridge library has ABI {abi}, this build expects {}",
                    sys::PRXAT_ABI_VERSION
                ),
            ));
        }
        if os == sys::PRXAT_OK {
            Ok(())
        } else {
            Err(SystemFault::new(SystemFaultKind::OsTooOld, ""))
        }
    }

    fn availability(&self, source: &str, target: &str) -> Result<Availability, SystemFault> {
        let (s, t) = (c_string(source)?, c_string(target)?);
        let mut msg = [0 as c_char; MESSAGE_CAP];
        // SAFETY: both strings are NUL terminated and outlive the call; `msg` is a writable buffer
        // of the stated capacity.
        let code = unsafe {
            sys::prxat_availability(s.as_ptr(), t.as_ptr(), msg.as_mut_ptr(), MESSAGE_CAP)
        };
        match code {
            sys::PRXAT_OK => Ok(Availability::Installed),
            sys::PRXAT_PACK_NOT_INSTALLED => Ok(Availability::PackNotInstalled),
            sys::PRXAT_PAIR_UNSUPPORTED => Ok(Availability::Unsupported),
            other => Err(fault_of(other, &msg)),
        }
    }

    fn open(
        &self,
        source: &str,
        target: &str,
        strategy: Strategy,
    ) -> Result<Arc<dyn SystemSession>, SystemFault> {
        let (s, t) = (c_string(source)?, c_string(target)?);
        let strategy = match strategy {
            Strategy::Default => sys::PRXAT_STRATEGY_DEFAULT,
            Strategy::LowLatency => sys::PRXAT_STRATEGY_LOW_LATENCY,
            Strategy::HighFidelity => sys::PRXAT_STRATEGY_HIGH_FIDELITY,
        };
        let mut msg = [0 as c_char; MESSAGE_CAP];
        let mut code = sys::PRXAT_OK;
        // SAFETY: strings are NUL terminated and outlive the call; `code` and `msg` are writable.
        let raw = unsafe {
            sys::prxat_create(
                s.as_ptr(),
                t.as_ptr(),
                strategy,
                &raw mut code,
                msg.as_mut_ptr(),
                MESSAGE_CAP,
            )
        };
        if raw.is_null() {
            return Err(fault_of(code, &msg));
        }
        Ok(Arc::new(FrameworkSession { raw }))
    }
}

/// An owned bridge session.
struct FrameworkSession {
    raw: *mut sys::prxat_session,
}

// SAFETY: the bridge serialises calls on a session and `prxat_cancel` is documented as callable
// from any thread; the pointer is only freed in `Drop`, when no call can be running (the engine's
// request threads hold an `Arc` of the session).
unsafe impl Send for FrameworkSession {}
// SAFETY: see above.
unsafe impl Sync for FrameworkSession {}

impl Drop for FrameworkSession {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `prxat_create` and is destroyed exactly once.
        unsafe { sys::prxat_destroy(self.raw) };
    }
}

impl SystemSession for FrameworkSession {
    fn prepare(&self, timeout: Duration) -> Result<(), SystemFault> {
        let mut msg = [0 as c_char; MESSAGE_CAP];
        // SAFETY: `raw` is a live session; `msg` is a writable buffer of the stated capacity.
        let code = unsafe {
            sys::prxat_prepare(self.raw, timeout_ms(timeout), msg.as_mut_ptr(), MESSAGE_CAP)
        };
        if code == sys::PRXAT_OK {
            Ok(())
        } else {
            Err(fault_of(code, &msg))
        }
    }

    fn translate(&self, text: &str, timeout: Duration) -> Result<String, SystemFault> {
        let input = c_string(text)?;
        let mut msg = [0 as c_char; MESSAGE_CAP];
        let mut out: *mut c_char = ptr::null_mut();
        // SAFETY: `raw` is live; `input` is NUL terminated; `out` and `msg` are writable.
        let code = unsafe {
            sys::prxat_translate(
                self.raw,
                input.as_ptr(),
                timeout_ms(timeout),
                &raw mut out,
                msg.as_mut_ptr(),
                MESSAGE_CAP,
            )
        };
        if code != sys::PRXAT_OK {
            return Err(fault_of(code, &msg));
        }
        if out.is_null() {
            return Err(SystemFault::new(
                SystemFaultKind::Framework,
                "the framework returned no text",
            ));
        }
        // SAFETY: on success `out` is a NUL-terminated string owned by us; it is copied and then
        // released once with `prxat_free`.
        let text = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: see above.
        unsafe { sys::prxat_free(out) };
        Ok(text)
    }

    fn cancel(&self) {
        // SAFETY: `raw` is live; cancel is callable from any thread.
        unsafe { sys::prxat_cancel(self.raw) };
    }
}

/// Read the pack state of a pair without opening a session: the system and the availability.
///
/// # Errors
/// `OsTooOld` or a framework failure.
pub fn pair_availability(source: &str, target: &str) -> Result<Availability, SystemFault> {
    AppleFramework.os_support()?;
    AppleFramework.availability(source, target)
}

/// One line for the bundle check: the bridge's ABI version and whether this system can run the
/// framework. Proves that the library was found through `@rpath` and answers; it does not call
/// the framework, so it needs no run loop.
pub fn runtime_version() -> String {
    // SAFETY: plain calls without arguments.
    let abi = unsafe { sys::prxat_abi_version() };
    let os = match AppleFramework.os_support() {
        Ok(()) => "macOS 26 or later: available".to_owned(),
        Err(_) => "this macOS is older than 26: system translation unavailable".to_owned(),
    };
    format!("bridge abi {abi}, {os}")
}
