//! Platform abstraction for listing input devices and opening one by identity: Core Audio on macOS, cpal elsewhere.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::AudioError;
use crate::input_policy::InputDevice;
#[cfg(not(target_os = "macos"))]
use crate::input_policy::InputTransport;

/// List input devices with UID and transport type.
///
/// On macOS this goes through Core Audio. On other platforms it uses cpal; the
/// UID is the device name and the transport is [`InputTransport::Unknown`].
///
/// # Errors
/// [`AudioError::InitializationFailed`] when the platform list cannot be read.
pub fn list_input_devices() -> Result<Vec<InputDevice>, AudioError> {
    #[cfg(target_os = "macos")]
    {
        crate::core_audio_devices::list_input_devices()
    }
    #[cfg(not(target_os = "macos"))]
    {
        list_input_devices_cpal()
    }
}

/// cpal-based enumeration: names only.
#[cfg(not(target_os = "macos"))]
fn list_input_devices_cpal() -> Result<Vec<InputDevice>, AudioError> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let devices = host
        .input_devices()
        .map_err(|e| AudioError::InitializationFailed(format!("enumerate inputs: {e}")))?;
    Ok(devices
        .filter_map(|d| d.name().ok())
        .map(|name| InputDevice {
            uid: name.clone(),
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
            transport: InputTransport::Unknown,
        })
        .collect())
}

/// Open the cpal device that corresponds to a chosen [`InputDevice`].
///
/// cpal identifies devices by name only, so two devices sharing a name resolve
/// to the first one cpal lists.
///
/// # Errors
/// [`AudioError::InputDeviceUnavailable`] when no cpal input has that name.
pub fn find_cpal_input(device: &InputDevice) -> Result<cpal::Device, AudioError> {
    let host = cpal::default_host();
    let mut candidates = host
        .input_devices()
        .map_err(|e| AudioError::InitializationFailed(format!("enumerate inputs: {e}")))?;
    candidates
        .find(|d| d.name().ok().as_deref() == Some(device.name.as_str()))
        .ok_or(AudioError::InputDeviceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_returns_named_devices_or_a_typed_error() {
        // CI may have no audio hardware; the call must not panic.
        if let Ok(devices) = list_input_devices() {
            assert!(
                devices
                    .iter()
                    .all(|d| !d.uid.is_empty() || !d.name.is_empty())
            );
        }
    }
}
