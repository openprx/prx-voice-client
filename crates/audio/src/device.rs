use cpal::traits::{DeviceTrait, HostTrait};

use crate::AudioError;

/// Enumerated audio device info.
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
    pub is_default: bool,
}

/// Enumerate all available audio devices.
pub fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, AudioError> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    // Get default devices for comparison
    let default_input_name = host.default_input_device().and_then(|d| d.name().ok());
    let default_output_name = host.default_output_device().and_then(|d| d.name().ok());

    // List input devices
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            let name = device.name().unwrap_or_else(|_| "Unknown Input".into());
            let is_default = default_input_name.as_deref() == Some(&name);
            devices.push(AudioDeviceInfo {
                id: format!("input:{}", name),
                name: name.clone(),
                is_input: true,
                is_output: false,
                is_default,
            });
        }
    }

    // List output devices
    if let Ok(output_devices) = host.output_devices() {
        for device in output_devices {
            let name = device.name().unwrap_or_else(|_| "Unknown Output".into());
            let is_default = default_output_name.as_deref() == Some(&name);
            devices.push(AudioDeviceInfo {
                id: format!("output:{}", name),
                name: name.clone(),
                is_input: false,
                is_output: true,
                is_default,
            });
        }
    }

    Ok(devices)
}

/// Get default input device name.
pub fn default_input_device_name() -> Option<String> {
    let host = cpal::default_host();
    host.default_input_device().and_then(|d| d.name().ok())
}

/// Get default output device name.
pub fn default_output_device_name() -> Option<String> {
    let host = cpal::default_host();
    host.default_output_device().and_then(|d| d.name().ok())
}

/// Get the default supported input config.
pub fn default_input_config() -> Result<cpal::SupportedStreamConfig, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(AudioError::InputDeviceUnavailable)?;
    device
        .default_input_config()
        .map_err(|e| AudioError::InitializationFailed(format!("input config: {}", e)))
}

/// Get the default supported output config.
pub fn default_output_config() -> Result<cpal::SupportedStreamConfig, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(AudioError::OutputDeviceUnavailable)?;
    device
        .default_output_config()
        .map_err(|e| AudioError::InitializationFailed(format!("output config: {}", e)))
}

/// Find an output device by name.
///
/// `name` may be a bare device name, a name fragment, or an id as produced by
/// [`list_audio_devices`] (`"output:<name>"`). An exact name match wins over a
/// substring match, and the match is case-sensitive.
///
/// # Errors
/// [`AudioError::OutputDeviceUnavailable`] if no output device matches.
pub fn find_output_device(name: &str) -> Result<cpal::Device, AudioError> {
    let wanted = name.strip_prefix("output:").unwrap_or(name);
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|e| AudioError::InitializationFailed(format!("enumerate outputs: {e}")))?;
    let mut partial = None;
    for device in devices {
        let Ok(device_name) = device.name() else {
            continue;
        };
        if device_name == wanted {
            return Ok(device);
        }
        if partial.is_none() && device_name.contains(wanted) {
            partial = Some(device);
        }
    }
    partial.ok_or(AudioError::OutputDeviceUnavailable)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_audio_devices_no_panic() {
        // In CI there may be no audio devices; just verify it returns Ok.
        let result = list_audio_devices();
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_device_names() {
        // These may return None in CI; just verify they don't panic.
        let _input = default_input_device_name();
        let _output = default_output_device_name();
    }
}
