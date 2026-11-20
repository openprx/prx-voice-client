//! Lists every audio device of the system, both directions and hidden ones included, with UID, transport type and liveness.
//!
//! The input list of [`crate::input_devices`] cannot see an output-only device,
//! and cpal knows neither UIDs nor hidden devices (Core Audio omits those from
//! the device list; [`list_devices_with`] resolves them by UID). The virtual microphone's
//! hidden sink and the driver status need exactly that, so this directory is
//! the one place that answers "which devices exist, by UID". On macOS it goes
//! through Core Audio; elsewhere it falls back to cpal names (the UID is the
//! name, the transport unknown, nothing hidden).

use crate::AudioError;
use crate::input_policy::InputTransport;

/// One audio device as the platform lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Stable platform identifier; empty when the platform has none.
    pub uid: String,
    /// Display name (the name cpal opens the device by).
    pub name: String,
    /// Connection type as the device reports it now.
    pub transport: InputTransport,
    /// At least one input channel.
    pub has_input: bool,
    /// At least one output channel.
    pub has_output: bool,
    /// The device reports itself alive.
    pub is_alive: bool,
    /// The device asks not to be shown to the user.
    pub is_hidden: bool,
}

/// List all devices.
///
/// # Errors
/// [`AudioError::InitializationFailed`] when the platform list cannot be read.
pub fn list_devices() -> Result<Vec<DeviceEntry>, AudioError> {
    #[cfg(target_os = "macos")]
    {
        crate::core_audio_devices::list_devices()
    }
    #[cfg(not(target_os = "macos"))]
    {
        list_devices_cpal()
    }
}

/// [`list_devices`] plus the devices with the given UIDs when the list lacks
/// them. Needed for hidden devices, which the list leaves out on macOS.
///
/// # Errors
/// As [`list_devices`].
pub fn list_devices_with(extra_uids: &[&str]) -> Result<Vec<DeviceEntry>, AudioError> {
    let mut devices = list_devices()?;
    for uid in extra_uids {
        if find_by_uid(&devices, uid).is_none()
            && let Some(entry) = lookup_uid(uid)
        {
            devices.push(entry);
        }
    }
    Ok(devices)
}

fn lookup_uid(uid: &str) -> Option<DeviceEntry> {
    #[cfg(target_os = "macos")]
    {
        crate::core_audio_devices::device_by_uid(uid)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = uid;
        None
    }
}

/// The entry with exactly this UID, if present.
pub fn find_by_uid<'a>(devices: &'a [DeviceEntry], uid: &str) -> Option<&'a DeviceEntry> {
    devices.iter().find(|d| d.uid == uid)
}

#[cfg(not(target_os = "macos"))]
fn list_devices_cpal() -> Result<Vec<DeviceEntry>, AudioError> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let mut entries: Vec<DeviceEntry> = Vec::new();
    let mut add = |name: String, input: bool, output: bool| {
        if let Some(e) = entries.iter_mut().find(|e| e.name == name) {
            e.has_input |= input;
            e.has_output |= output;
        } else {
            entries.push(DeviceEntry {
                uid: name.clone(),
                name,
                transport: InputTransport::Unknown,
                has_input: input,
                has_output: output,
                is_alive: true,
                is_hidden: false,
            });
        }
    };
    let inputs = host
        .input_devices()
        .map_err(|e| AudioError::InitializationFailed(format!("enumerate inputs: {e}")))?;
    for d in inputs.filter_map(|d| d.name().ok()) {
        add(d, true, false);
    }
    let outputs = host
        .output_devices()
        .map_err(|e| AudioError::InitializationFailed(format!("enumerate outputs: {e}")))?;
    for d in outputs.filter_map(|d| d.name().ok()) {
        add(d, false, true);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uid: &str) -> DeviceEntry {
        DeviceEntry {
            uid: uid.to_owned(),
            name: uid.to_owned(),
            transport: InputTransport::Virtual,
            has_input: false,
            has_output: true,
            is_alive: true,
            is_hidden: true,
        }
    }

    #[test]
    fn find_by_uid_is_exact() {
        let list = [entry("a.sink"), entry("a")];
        assert_eq!(find_by_uid(&list, "a").unwrap().uid, "a");
        assert!(find_by_uid(&list, "a.").is_none());
    }

    #[test]
    fn listing_does_not_panic_without_hardware() {
        let _ = list_devices();
    }
}
