//! Core Audio default devices (macOS): read the system default input and output, and set the default input, by UID; safe wrapper over `native/prxvc_defaults.m`.
//!
//! Setting the default input needs no administrator rights. Nothing here runs
//! on a real-time thread; change notifications are in
//! [`crate::routing::hal_listener`].

use std::ffi::{CString, c_char};

use crate::device_directory::{DeviceEntry, find_by_uid};

const UID_LEN: usize = 256;

unsafe extern "C" {
    fn prxvc_default_device_uid(is_input: i32, uid_out: *mut c_char, capacity: i32) -> i32;
    fn prxvc_set_default_input_uid(uid: *const c_char) -> i32;
}

/// Which default device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultKind {
    /// The system default input.
    Input,
    /// The system default output.
    Output,
}

/// A default-device call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DefaultsError {
    /// The system has no such default device.
    #[error("the system has no default device")]
    NoDefault,
    /// No device has the UID.
    #[error("no audio device with that UID")]
    UnknownDevice,
    /// The device has no input channel, so it cannot be the default input.
    #[error("the device has no input channel")]
    NotAnInput,
    /// Core Audio refused (the `OSStatus`).
    #[error("Core Audio status {0}")]
    Os(i32),
}

/// The UID of the system default input or output device.
///
/// # Errors
/// [`DefaultsError::NoDefault`] when there is none, [`DefaultsError::Os`] on a Core Audio failure.
pub fn default_device_uid(kind: DefaultKind) -> Result<String, DefaultsError> {
    let mut buf = [0 as c_char; UID_LEN];
    // SAFETY: `buf` is `UID_LEN` bytes and the shim writes at most `capacity` bytes, NUL-terminated.
    let status = unsafe {
        prxvc_default_device_uid(
            i32::from(kind == DefaultKind::Input),
            buf.as_mut_ptr(),
            UID_LEN as i32,
        )
    };
    match status {
        0 => {
            let bytes: Vec<u8> = buf
                .iter()
                .map(|&c| c as u8)
                .take_while(|&b| b != 0)
                .collect();
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        -1 => Err(DefaultsError::NoDefault),
        other => Err(DefaultsError::Os(other)),
    }
}

/// The directory entry of the system default input or output device.
pub fn default_entry(kind: DefaultKind) -> Option<DeviceEntry> {
    let uid = default_device_uid(kind).ok()?;
    let devices = crate::device_directory::list_devices_with(&[&uid]).ok()?;
    find_by_uid(&devices, &uid).cloned()
}

/// Make the device with this UID the system default input.
///
/// # Errors
/// [`DefaultsError::UnknownDevice`], [`DefaultsError::NotAnInput`] or [`DefaultsError::Os`].
pub fn set_default_input(uid: &str) -> Result<(), DefaultsError> {
    let c_uid = CString::new(uid).map_err(|_| DefaultsError::UnknownDevice)?;
    // SAFETY: `c_uid` is NUL-terminated and outlives the call.
    match unsafe { prxvc_set_default_input_uid(c_uid.as_ptr()) } {
        0 => Ok(()),
        -1 => Err(DefaultsError::UnknownDevice),
        -2 => Err(DefaultsError::NotAnInput),
        other => Err(DefaultsError::Os(other)),
    }
}
