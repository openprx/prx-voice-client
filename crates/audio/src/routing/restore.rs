//! Which device the default input goes back to when call mode ends: the remembered one if usable, else the input policy's choice; never our own devices.

use prxvc_ports::routing::DeviceUid;

use super::events::DeviceRef;
use crate::device_directory::DeviceEntry;
use crate::input_policy::{InputDevice, OWN_MIC_NAME, OWN_MIC_SINK_UID, OWN_MIC_UID, choose_input};

/// Why a device was chosen to be restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreReason {
    /// It is the device that was the default before call mode.
    Saved,
    /// The remembered device is gone, not alive or none was remembered; the
    /// input policy chose a physical microphone.
    Policy,
}

/// A device to restore and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    /// The device to make the default input.
    pub target: DeviceRef,
    /// Why it was chosen.
    pub reason: RestoreReason,
}

/// Whether the UID or name is one of our virtual devices.
pub fn is_own(uid: &str, name: &str) -> bool {
    uid == OWN_MIC_UID || uid == OWN_MIC_SINK_UID || name == OWN_MIC_NAME
}

/// The input-capable, alive, visible devices as the input policy sees them.
pub fn input_candidates(devices: &[DeviceEntry], default: Option<&DeviceUid>) -> Vec<InputDevice> {
    devices
        .iter()
        .filter(|d| d.has_input && d.is_alive && !d.is_hidden)
        .map(|d| InputDevice {
            uid: d.uid.clone(),
            name: d.name.clone(),
            transport: d.transport,
            is_default: default.is_some_and(|u| u.as_str() == d.uid),
        })
        .collect()
}

/// The device to restore, or `None` when the system has no physical input.
///
/// `saved` is the device that was the default before call mode (`None` when it
/// was already ours or unknown). `default` is the current default input, which
/// during call mode is our microphone; the policy avoids it.
pub fn plan_restore(
    saved: Option<&DeviceRef>,
    devices: &[DeviceEntry],
    default: Option<&DeviceUid>,
) -> Option<RestorePlan> {
    if let Some(saved) = saved
        && !is_own(saved.uid.as_str(), &saved.name)
        && let Some(entry) = devices
            .iter()
            .find(|d| d.uid == saved.uid.as_str() && d.has_input && d.is_alive)
    {
        return Some(RestorePlan {
            target: DeviceRef {
                uid: saved.uid.clone(),
                name: entry.name.clone(),
            },
            reason: RestoreReason::Saved,
        });
    }
    let selection = choose_input(&input_candidates(devices, default), "").ok()?;
    let device = selection.device;
    if is_own(&device.uid, &device.name) {
        return None;
    }
    Some(RestorePlan {
        target: DeviceRef {
            uid: DeviceUid::new(device.uid),
            name: device.name,
        },
        reason: RestoreReason::Policy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_policy::InputTransport;

    fn entry(uid: &str, transport: InputTransport) -> DeviceEntry {
        DeviceEntry {
            uid: uid.to_owned(),
            name: format!("name of {uid}"),
            transport,
            has_input: true,
            has_output: false,
            is_alive: true,
            is_hidden: false,
        }
    }

    fn saved(uid: &str) -> DeviceRef {
        DeviceRef {
            uid: DeviceUid::new(uid),
            name: uid.to_owned(),
        }
    }

    fn list() -> Vec<DeviceEntry> {
        vec![
            entry(OWN_MIC_UID, InputTransport::Usb),
            entry("usb", InputTransport::Usb),
            entry("builtin", InputTransport::BuiltIn),
        ]
    }

    #[test]
    fn the_saved_device_wins_when_it_is_usable() {
        let plan = plan_restore(
            Some(&saved("usb")),
            &list(),
            Some(&DeviceUid::new(OWN_MIC_UID)),
        )
        .unwrap();
        assert_eq!(plan.reason, RestoreReason::Saved);
        assert_eq!(plan.target.uid.as_str(), "usb");
        assert_eq!(plan.target.name, "name of usb");
    }

    #[test]
    fn a_missing_or_dead_saved_device_falls_back_to_the_policy() {
        let mut devices = list();
        let plan = plan_restore(
            Some(&saved("gone")),
            &devices,
            Some(&DeviceUid::new(OWN_MIC_UID)),
        )
        .unwrap();
        assert_eq!(
            (plan.reason, plan.target.uid.as_str()),
            (RestoreReason::Policy, "builtin")
        );
        devices
            .iter_mut()
            .find(|d| d.uid == "usb")
            .unwrap()
            .is_alive = false;
        let plan = plan_restore(Some(&saved("usb")), &devices, None).unwrap();
        assert_eq!(plan.reason, RestoreReason::Policy);
    }

    #[test]
    fn our_own_devices_are_never_a_target() {
        let own = DeviceRef {
            uid: DeviceUid::new(OWN_MIC_UID),
            name: OWN_MIC_NAME.to_owned(),
        };
        let plan = plan_restore(Some(&own), &list(), Some(&DeviceUid::new(OWN_MIC_UID))).unwrap();
        assert_eq!(plan.reason, RestoreReason::Policy);
        assert_ne!(plan.target.uid.as_str(), OWN_MIC_UID);
        let only_own = vec![entry(OWN_MIC_UID, InputTransport::Usb)];
        assert_eq!(
            plan_restore(Some(&own), &only_own, Some(&DeviceUid::new(OWN_MIC_UID))),
            None
        );
        assert_eq!(plan_restore(None, &[], None), None);
    }

    #[test]
    fn a_remembered_virtual_device_of_another_vendor_is_restored_exactly() {
        let mut devices = list();
        devices.push(entry("blackhole", InputTransport::Virtual));
        let plan = plan_restore(Some(&saved("blackhole")), &devices, None).unwrap();
        assert_eq!(
            (plan.reason, plan.target.uid.as_str()),
            (RestoreReason::Saved, "blackhole")
        );
    }
}
