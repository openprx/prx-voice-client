//! Core Audio device enumeration (macOS): safe wrapper over `native/prxvc_devices.m`, adding UID, transport type, direction and liveness that cpal does not expose.

use std::ffi::CStr;
use std::os::raw::c_char;

use crate::AudioError;
use crate::device_directory::DeviceEntry;
use crate::input_policy::{InputDevice, InputTransport};

const STRING_LEN: usize = 256;
/// Upper bound on devices read in one enumeration.
const MAX_DEVICES: usize = 64;

/// Mirrors `PrxvcInputDevice` in `native/prxvc_devices.h`.
#[repr(C)]
struct RawInputDevice {
    _device_id: u32,
    uid: [c_char; STRING_LEN],
    name: [c_char; STRING_LEN],
    transport: u32,
    is_default: i32,
}

/// Mirrors `PrxvcDevice` in `native/prxvc_devices.h`.
#[repr(C)]
struct RawDevice {
    _device_id: u32,
    uid: [c_char; STRING_LEN],
    name: [c_char; STRING_LEN],
    transport: u32,
    has_input: i32,
    has_output: i32,
    is_alive: i32,
    is_hidden: i32,
    _is_default_input: i32,
}

unsafe extern "C" {
    fn prxvc_list_input_devices(out: *mut RawInputDevice, capacity: i32) -> i32;
    fn prxvc_list_devices(out: *mut RawDevice, capacity: i32) -> i32;
    fn prxvc_device_by_uid(uid: *const c_char, out: *mut RawDevice) -> i32;
}

fn map_transport(raw: u32) -> InputTransport {
    match raw {
        1 => InputTransport::BuiltIn,
        2 => InputTransport::Usb,
        3 => InputTransport::Bluetooth,
        4 => InputTransport::Virtual,
        5 => InputTransport::Aggregate,
        6 => InputTransport::Other,
        _ => InputTransport::Unknown,
    }
}

fn read_string(field: &[c_char; STRING_LEN]) -> String {
    // SAFETY: the shim NUL-terminates every string it writes and the struct is
    // zero-initialised, so the array always contains a terminator.
    unsafe { CStr::from_ptr(field.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

/// Enumerate input-capable devices through Core Audio.
///
/// # Errors
/// [`AudioError::InitializationFailed`] with the Core Audio status when the
/// device list cannot be read.
pub fn list_input_devices() -> Result<Vec<InputDevice>, AudioError> {
    // SAFETY: an all-zero `RawInputDevice` is a valid value (integers and char arrays).
    let mut raw: Vec<RawInputDevice> = (0..MAX_DEVICES)
        .map(|_| unsafe { std::mem::zeroed() })
        .collect();
    // SAFETY: `raw` holds `MAX_DEVICES` initialised elements; the shim writes at
    // most `capacity` entries and returns the count or a negative status.
    let n = unsafe { prxvc_list_input_devices(raw.as_mut_ptr(), MAX_DEVICES as i32) };
    if n < 0 {
        return Err(AudioError::InitializationFailed(format!(
            "Core Audio device enumeration failed (status {n})"
        )));
    }
    Ok(raw
        .iter()
        .take(n as usize)
        .map(|d| InputDevice {
            uid: read_string(&d.uid),
            name: read_string(&d.name),
            transport: map_transport(d.transport),
            is_default: d.is_default != 0,
        })
        .collect())
}

/// Enumerate every device (both directions, hidden ones included).
///
/// # Errors
/// [`AudioError::InitializationFailed`] with the Core Audio status when the
/// device list cannot be read.
pub fn list_devices() -> Result<Vec<DeviceEntry>, AudioError> {
    // SAFETY: an all-zero `RawDevice` is a valid value (integers and char arrays).
    let mut raw: Vec<RawDevice> = (0..MAX_DEVICES)
        .map(|_| unsafe { std::mem::zeroed() })
        .collect();
    // SAFETY: `raw` holds `MAX_DEVICES` initialised elements; the shim writes at
    // most `capacity` entries and returns the count or a negative status.
    let n = unsafe { prxvc_list_devices(raw.as_mut_ptr(), MAX_DEVICES as i32) };
    if n < 0 {
        return Err(AudioError::InitializationFailed(format!(
            "Core Audio device enumeration failed (status {n})"
        )));
    }
    Ok(raw.iter().take(n as usize).map(entry_of).collect())
}

fn entry_of(d: &RawDevice) -> DeviceEntry {
    DeviceEntry {
        uid: read_string(&d.uid),
        name: read_string(&d.name),
        transport: map_transport(d.transport),
        has_input: d.has_input != 0,
        has_output: d.has_output != 0,
        is_alive: d.is_alive != 0,
        is_hidden: d.is_hidden != 0,
    }
}

/// Look a device up by UID, hidden devices included.
///
/// Core Audio leaves hidden devices (the virtual microphone's sink) out of the
/// device list but still resolves their UID.
pub fn device_by_uid(uid: &str) -> Option<DeviceEntry> {
    let uid = std::ffi::CString::new(uid).ok()?;
    // SAFETY: an all-zero `RawDevice` is a valid value (integers and char arrays).
    let mut raw: RawDevice = unsafe { std::mem::zeroed() };
    // SAFETY: `uid` is NUL-terminated and `raw` is a valid out-pointer for the call.
    let found = unsafe { prxvc_device_by_uid(uid.as_ptr(), &mut raw) };
    (found == 0).then(|| entry_of(&raw))
}
