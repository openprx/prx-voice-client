//! `SystemAudioRouting` on macOS: default input by UID, device existence, call activity, and change events from the HAL listener and the power monitor.

use prxvc_ports::routing::{
    BundleId, CallActivity, DeviceUid, RoutingChange, RoutingError, SystemAudioRouting,
};

use super::changes::{ChangeQueue, Notice, QUEUE_CAPACITY, coalesce};
use super::hal_listener::HalListener;
use super::power::PowerMonitor;
use crate::call_activity::native::read_app_activity;
use crate::core_audio_defaults::{
    DefaultKind, DefaultsError, default_device_uid, set_default_input,
};
use crate::device_directory::list_devices_with;

/// The operating-system routing of this machine.
///
/// Created once; the listeners run until it is dropped. Changes are queued by
/// the native callbacks and handed out by [`SystemAudioRouting::poll_changes`].
pub struct CoreAudioRouting {
    queue: ChangeQueue,
    _hal: HalListener,
    /// `None` when the power notifications could not be registered (logged; call mode works without them).
    _power: Option<PowerMonitor>,
}

impl CoreAudioRouting {
    /// Start listening for default-device, device-list and power changes.
    ///
    /// # Errors
    /// [`RoutingError::Unavailable`] when the Core Audio listeners cannot be installed.
    pub fn new() -> Result<Self, RoutingError> {
        let queue = ChangeQueue::new(QUEUE_CAPACITY);
        let hal = HalListener::start(queue.sink()).map_err(|status| RoutingError::Unavailable {
            reason: format!("Core Audio change listener failed (status {status})"),
        })?;
        let power = match PowerMonitor::start(queue.sink()) {
            Ok(p) => Some(p),
            Err(status) => {
                tracing::warn!(status, "sleep and wake notifications are unavailable");
                None
            }
        };
        Ok(Self {
            queue,
            _hal: hal,
            _power: power,
        })
    }
}

fn map_error(e: DefaultsError, uid: Option<&DeviceUid>) -> RoutingError {
    match (e, uid) {
        (DefaultsError::UnknownDevice, Some(u)) => RoutingError::UnknownDevice(u.clone()),
        (other, _) => RoutingError::Unavailable {
            reason: other.to_string(),
        },
    }
}

impl SystemAudioRouting for CoreAudioRouting {
    fn default_input(&self) -> Result<DeviceUid, RoutingError> {
        default_device_uid(DefaultKind::Input)
            .map(DeviceUid::new)
            .map_err(|e| map_error(e, None))
    }

    fn set_default_input(&self, uid: &DeviceUid) -> Result<(), RoutingError> {
        set_default_input(uid.as_str()).map_err(|e| map_error(e, Some(uid)))
    }

    fn device_exists(&self, uid: &DeviceUid) -> bool {
        list_devices_with(&[uid.as_str()])
            .is_ok_and(|devices| devices.iter().any(|d| d.uid == uid.as_str()))
    }

    fn call_activity(&self, app: &BundleId) -> Result<CallActivity, RoutingError> {
        let a = read_app_activity(app.as_str()).map_err(|e| RoutingError::Unavailable {
            reason: e.to_string(),
        })?;
        let uids = |devices: &[crate::call_activity::ProcessDevice]| {
            devices
                .iter()
                .map(|d| DeviceUid::new(d.uid.clone()))
                .collect()
        };
        Ok(CallActivity {
            input_running: a.input_running,
            output_running: a.output_running,
            input_devices: uids(&a.input_devices),
            output_devices: uids(&a.output_devices),
        })
    }

    fn poll_changes(&mut self) -> Vec<RoutingChange> {
        let mut out = Vec::new();
        for notice in coalesce(&self.queue.drain()) {
            match notice {
                Notice::DefaultInput => match self.default_input() {
                    Ok(uid) => out.push(RoutingChange::DefaultInputChanged(uid)),
                    Err(e) => {
                        tracing::warn!(error = %e, "default input changed but cannot be read")
                    }
                },
                Notice::DefaultOutput => match default_device_uid(DefaultKind::Output) {
                    Ok(uid) => out.push(RoutingChange::DefaultOutputChanged(DeviceUid::new(uid))),
                    Err(e) => {
                        tracing::warn!(error = %e, "default output changed but cannot be read")
                    }
                },
                Notice::DeviceList => out.push(RoutingChange::DeviceListChanged),
                Notice::Sleep => out.push(RoutingChange::Sleep),
                Notice::Wake => out.push(RoutingChange::Wake),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Read-only smoke tests against the real system; they change nothing.
    // Machines without audio hardware (CI) may have no default input, which is accepted.

    #[test]
    fn the_listeners_start_and_stop_and_the_default_input_is_readable_when_present() {
        let mut routing = CoreAudioRouting::new().expect("listeners install");
        if let Ok(uid) = routing.default_input() {
            assert!(
                routing.device_exists(&uid),
                "the default input is a listed device"
            );
        }
        assert!(!routing.device_exists(&DeviceUid::new("no.such.device.uid")));
        let _ = routing.poll_changes();
    }

    #[test]
    fn setting_an_unknown_device_fails_without_changing_the_default() {
        let routing = CoreAudioRouting::new().expect("listeners install");
        let before = routing.default_input();
        let ghost = DeviceUid::new("no.such.device.uid");
        assert_eq!(
            routing.set_default_input(&ghost),
            Err(RoutingError::UnknownDevice(ghost))
        );
        assert_eq!(routing.default_input(), before);
    }
}
