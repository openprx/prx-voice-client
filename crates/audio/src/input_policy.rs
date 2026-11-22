//! Input-device policy: which microphone the client captures, decided from a device list and the configured preference.
//!
//! Pure and hardware-free. The client never opens "the default input" blindly:
//! the system default may be our own virtual microphone (so that call
//! applications pick it up), and capturing it would feed our synthesized speech
//! back into recognition. The policy runs on every device-list change.
//!
//! Decision table (first matching row wins):
//!
//! | Configured value | Situation | Result |
//! |---|---|---|
//! | names our virtual microphone or its sink (by UID or name) | any | [`InputPolicyError::OwnVirtualDevice`] |
//! | a UID or name present in the list | any | that device ([`SelectionReason::Configured`]) |
//! | a UID or name not in the list | continue with the rows below | the result carries [`SelectionReason::ConfiguredMissing`] |
//! | empty (or missing) | the default is a physical device | the default ([`SelectionReason::SystemDefault`]) |
//! | empty (or missing) | the default is virtual, aggregate or our own | built-in microphone, else the first physical input ([`SelectionReason::DefaultAvoided`]) |
//! | empty (or missing) | no default and no usable device | [`InputPolicyError::NoUsableDevice`] |
//!
//! Our own virtual microphone and its sink are never candidates. Bluetooth is
//! only recorded in [`InputTransport`]; the policy does not treat it specially.

use thiserror::Error;

/// UID of our virtual microphone driver (the visible input).
pub const OWN_MIC_UID: &str = "org.openprx.prx-voice-client.microphone";
/// UID of the hidden sink the client writes synthesized speech into.
pub const OWN_MIC_SINK_UID: &str = "org.openprx.prx-voice-client.microphone.sink";
/// Display name of our virtual microphone.
pub const OWN_MIC_NAME: &str = "PRX Voice Microphone";

/// How a device is connected, as reported by the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTransport {
    /// Built into the computer.
    BuiltIn,
    /// USB.
    Usb,
    /// Bluetooth classic or LE. Recorded only; no behaviour depends on it.
    Bluetooth,
    /// A software device created by a driver.
    Virtual,
    /// A device combining other devices.
    Aggregate,
    /// Any other known transport (FireWire, Thunderbolt, AirPlay, ...).
    Other,
    /// The platform does not say (non-macOS builds).
    Unknown,
}

/// One input-capable device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    /// Stable platform identifier; empty when the platform has none.
    pub uid: String,
    /// Display name.
    pub name: String,
    /// Connection type.
    pub transport: InputTransport,
    /// Whether this is the system default input.
    pub is_default: bool,
}

impl InputDevice {
    /// Whether this is our virtual microphone or its sink.
    pub fn is_own_virtual(&self) -> bool {
        is_own_identifier(&self.uid) || self.name == OWN_MIC_NAME
    }

    /// Whether the device is a stand-in the policy avoids for the default:
    /// virtual, aggregate or ours.
    fn is_synthetic(&self) -> bool {
        matches!(
            self.transport,
            InputTransport::Virtual | InputTransport::Aggregate
        ) || self.is_own_virtual()
    }
}

fn is_own_identifier(value: &str) -> bool {
    value == OWN_MIC_UID || value == OWN_MIC_SINK_UID || value == OWN_MIC_NAME
}

/// Why a device was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionReason {
    /// The configured device is present.
    Configured,
    /// Nothing configured; the system default is a physical device.
    SystemDefault,
    /// Nothing configured; the default was virtual, aggregate or ours, so a
    /// physical device replaced it.
    DefaultAvoided,
    /// A device is configured but absent; the default rules chose this one.
    /// The caller shows this: it is a fallback the user did not ask for.
    ConfiguredMissing,
}

/// The device to open and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSelection {
    /// The chosen device.
    pub device: InputDevice,
    /// Why it was chosen.
    pub reason: SelectionReason,
}

/// No input device can be chosen.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InputPolicyError {
    /// The device list holds no physical input.
    #[error("no usable microphone: only virtual or no input devices are present")]
    NoUsableDevice,
    /// The configured device is our own virtual microphone or its sink.
    #[error(
        "the configured input device \"{configured}\" is the PRX virtual microphone; \
         choose a physical microphone in the settings"
    )]
    OwnVirtualDevice {
        /// The configured value as written.
        configured: String,
    },
}

/// Choose the input device.
///
/// `configured` is the configured UID or name; empty means "no preference".
///
/// # Errors
/// [`InputPolicyError::OwnVirtualDevice`] when `configured` names our virtual
/// microphone or its sink; [`InputPolicyError::NoUsableDevice`] when no physical
/// input exists.
pub fn choose_input(
    devices: &[InputDevice],
    configured: &str,
) -> Result<InputSelection, InputPolicyError> {
    let configured = configured.trim();
    if is_own_identifier(configured) {
        return Err(InputPolicyError::OwnVirtualDevice {
            configured: configured.to_owned(),
        });
    }

    let mut missing = false;
    if !configured.is_empty() {
        // UID first, then name; our own devices are never matched.
        let found = devices
            .iter()
            .find(|d| !d.is_own_virtual() && d.uid == configured)
            .or_else(|| {
                devices
                    .iter()
                    .find(|d| !d.is_own_virtual() && d.name == configured)
            });
        if let Some(device) = found {
            return Ok(InputSelection {
                device: device.clone(),
                reason: SelectionReason::Configured,
            });
        }
        missing = true;
    }

    let (device, avoided) = default_choice(devices)?;
    let reason = if missing {
        SelectionReason::ConfiguredMissing
    } else if avoided {
        SelectionReason::DefaultAvoided
    } else {
        SelectionReason::SystemDefault
    };
    Ok(InputSelection { device, reason })
}

/// The device the default rules pick, and whether the system default was avoided.
fn default_choice(devices: &[InputDevice]) -> Result<(InputDevice, bool), InputPolicyError> {
    let default = devices.iter().find(|d| d.is_default);
    if let Some(d) = default.filter(|d| !d.is_synthetic()) {
        return Ok((d.clone(), false));
    }
    let avoided = default.is_some();
    let physical = |d: &&InputDevice| !d.is_synthetic();
    let built_in = devices
        .iter()
        .filter(physical)
        .find(|d| d.transport == InputTransport::BuiltIn);
    built_in
        .or_else(|| devices.iter().find(physical))
        .map(|d| (d.clone(), avoided))
        .ok_or(InputPolicyError::NoUsableDevice)
}

/// What a re-evaluation changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionUpdate {
    /// The same device remains selected (its reason may differ); nothing to do.
    Unchanged,
    /// Capture must move to this selection (first choice or a switch).
    Changed {
        /// The device capture was on, if any.
        from: Option<InputDevice>,
        /// The new selection.
        to: InputSelection,
    },
    /// No device can be chosen now; capture must stop until one appears.
    Lost(InputPolicyError),
}

/// Tracks the current choice across device-list changes.
#[derive(Debug, Clone)]
pub struct InputSelector {
    configured: String,
    current: Option<InputSelection>,
    /// The last reported failure, so a persisting error is reported once.
    failed: Option<InputPolicyError>,
}

impl InputSelector {
    /// A selector with the configured preference and no current device.
    pub fn new(configured: impl Into<String>) -> Self {
        Self {
            configured: configured.into(),
            current: None,
            failed: None,
        }
    }

    /// The device capture is on, if any.
    pub fn current(&self) -> Option<&InputSelection> {
        self.current.as_ref()
    }

    /// Forget the current device (its capture could not be opened), so the next
    /// [`update`](Self::update) reports a change again.
    pub fn forget_current(&mut self) {
        self.current = None;
    }

    /// Re-evaluate the policy against a fresh device list.
    pub fn update(&mut self, devices: &[InputDevice]) -> SelectionUpdate {
        match choose_input(devices, &self.configured) {
            Ok(selection) => {
                self.failed = None;
                let same = self
                    .current
                    .as_ref()
                    .is_some_and(|c| same_device(&c.device, &selection.device));
                if same {
                    self.current = Some(selection);
                    return SelectionUpdate::Unchanged;
                }
                let from = self.current.replace(selection.clone()).map(|c| c.device);
                SelectionUpdate::Changed {
                    from,
                    to: selection,
                }
            }
            Err(error) => {
                self.current = None;
                if self.failed.as_ref() == Some(&error) {
                    return SelectionUpdate::Unchanged;
                }
                self.failed = Some(error.clone());
                SelectionUpdate::Lost(error)
            }
        }
    }
}

fn same_device(a: &InputDevice, b: &InputDevice) -> bool {
    if a.uid.is_empty() || b.uid.is_empty() {
        a.name == b.name
    } else {
        a.uid == b.uid
    }
}

#[cfg(test)]
mod tests;
