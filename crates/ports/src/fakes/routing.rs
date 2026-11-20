//! Fake system audio routing: an in-memory set of devices with a settable default input and scripted change events.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::routing::{
    BundleId, CallActivity, DeviceUid, RoutingChange, RoutingError, SystemAudioRouting,
};

#[derive(Default)]
struct RoutingState {
    default_input: Option<DeviceUid>,
    devices: HashSet<DeviceUid>,
    activity: HashMap<BundleId, CallActivity>,
    pending: Vec<RoutingChange>,
    set_calls: Vec<DeviceUid>,
    fail_next_set: Option<RoutingError>,
    fail_queries: Option<RoutingError>,
}

/// A [`SystemAudioRouting`] backed by memory. Clones of its [`RoutingProbe`]
/// let a test change the "system" while the controller owns the fake.
pub struct FakeSystemAudioRouting {
    state: Arc<Mutex<RoutingState>>,
}

/// The test's handle on the fake's state.
#[derive(Clone)]
pub struct RoutingProbe {
    state: Arc<Mutex<RoutingState>>,
}

impl FakeSystemAudioRouting {
    /// A system with `devices` and `default_input` (which is added to the devices).
    pub fn new(default_input: &str, devices: &[&str]) -> Self {
        let mut known: HashSet<DeviceUid> = devices.iter().map(|d| DeviceUid::new(*d)).collect();
        known.insert(DeviceUid::new(default_input));
        let state = RoutingState {
            default_input: Some(DeviceUid::new(default_input)),
            devices: known,
            ..RoutingState::default()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// A handle for the test to drive and inspect the fake.
    pub fn probe(&self) -> RoutingProbe {
        RoutingProbe {
            state: Arc::clone(&self.state),
        }
    }
}

impl RoutingProbe {
    /// Declare what `app` is doing with audio devices.
    pub fn set_activity(&self, app: &str, activity: CallActivity) {
        self.state
            .lock()
            .activity
            .insert(BundleId::new(app), activity);
    }

    /// Make a device appear and queue the change event.
    pub fn plug(&self, uid: &str) {
        let mut s = self.state.lock();
        s.devices.insert(DeviceUid::new(uid));
        s.pending.push(RoutingChange::DeviceListChanged);
    }

    /// Make a device disappear and queue the change event.
    pub fn unplug(&self, uid: &str) {
        let mut s = self.state.lock();
        s.devices.remove(&DeviceUid::new(uid));
        s.pending.push(RoutingChange::DeviceListChanged);
    }

    /// Change the default input as the user would in System Settings: no
    /// `set_default_input` call is recorded, a change event is queued.
    pub fn user_selects_input(&self, uid: &str) {
        let mut s = self.state.lock();
        let uid = DeviceUid::new(uid);
        s.default_input = Some(uid.clone());
        s.pending.push(RoutingChange::DefaultInputChanged(uid));
    }

    /// Queue an arbitrary change event (sleep, wake, default output).
    pub fn push_change(&self, change: RoutingChange) {
        self.state.lock().pending.push(change);
    }

    /// Make the next `set_default_input` fail.
    pub fn fail_next_set(&self, error: RoutingError) {
        self.state.lock().fail_next_set = Some(error);
    }

    /// Make `default_input` and `call_activity` fail until cleared with `None`.
    pub fn fail_queries(&self, error: Option<RoutingError>) {
        self.state.lock().fail_queries = error;
    }

    /// The default input now.
    pub fn default_input(&self) -> Option<DeviceUid> {
        self.state.lock().default_input.clone()
    }

    /// Every successful `set_default_input` call, in order.
    pub fn set_calls(&self) -> Vec<DeviceUid> {
        self.state.lock().set_calls.clone()
    }
}

impl SystemAudioRouting for FakeSystemAudioRouting {
    fn default_input(&self) -> Result<DeviceUid, RoutingError> {
        let s = self.state.lock();
        if let Some(e) = &s.fail_queries {
            return Err(e.clone());
        }
        s.default_input
            .clone()
            .ok_or_else(|| RoutingError::Unavailable {
                reason: "no default input".to_owned(),
            })
    }

    fn set_default_input(&self, uid: &DeviceUid) -> Result<(), RoutingError> {
        let mut s = self.state.lock();
        if let Some(e) = s.fail_next_set.take() {
            return Err(e);
        }
        if !s.devices.contains(uid) {
            return Err(RoutingError::UnknownDevice(uid.clone()));
        }
        s.default_input = Some(uid.clone());
        s.set_calls.push(uid.clone());
        Ok(())
    }

    fn device_exists(&self, uid: &DeviceUid) -> bool {
        self.state.lock().devices.contains(uid)
    }

    fn call_activity(&self, app: &BundleId) -> Result<CallActivity, RoutingError> {
        let s = self.state.lock();
        if let Some(e) = &s.fail_queries {
            return Err(e.clone());
        }
        Ok(s.activity.get(app).cloned().unwrap_or_default())
    }

    fn poll_changes(&mut self) -> Vec<RoutingChange> {
        std::mem::take(&mut self.state.lock().pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(s: &str) -> DeviceUid {
        DeviceUid::new(s)
    }

    #[test]
    fn the_default_input_can_be_switched_and_restored() {
        let routing = FakeSystemAudioRouting::new("mic", &["virtual"]);
        let probe = routing.probe();
        assert_eq!(routing.default_input(), Ok(uid("mic")));
        routing.set_default_input(&uid("virtual")).expect("switch");
        assert_eq!(routing.default_input(), Ok(uid("virtual")));
        routing.set_default_input(&uid("mic")).expect("restore");
        assert_eq!(probe.set_calls(), vec![uid("virtual"), uid("mic")]);
        assert!(routing.device_exists(&uid("virtual")));
    }

    #[test]
    fn switching_to_an_unknown_device_fails_without_changing_anything() {
        let routing = FakeSystemAudioRouting::new("mic", &[]);
        assert_eq!(
            routing.set_default_input(&uid("ghost")),
            Err(RoutingError::UnknownDevice(uid("ghost")))
        );
        assert_eq!(routing.default_input(), Ok(uid("mic")));
        assert!(routing.probe().set_calls().is_empty());
    }

    #[test]
    fn injected_failures_hit_the_chosen_call_only() {
        let routing = FakeSystemAudioRouting::new("mic", &["virtual"]);
        let probe = routing.probe();
        probe.fail_next_set(RoutingError::Unavailable {
            reason: "denied".to_owned(),
        });
        assert!(routing.set_default_input(&uid("virtual")).is_err());
        assert!(
            routing.set_default_input(&uid("virtual")).is_ok(),
            "only the next call failed"
        );

        probe.fail_queries(Some(RoutingError::Internal("hal".to_owned())));
        assert_eq!(
            routing.default_input(),
            Err(RoutingError::Internal("hal".to_owned()))
        );
        assert!(routing.call_activity(&BundleId::new("app")).is_err());
        probe.fail_queries(None);
        assert!(routing.default_input().is_ok());
    }

    #[test]
    fn changes_are_delivered_once_in_order() {
        let mut routing = FakeSystemAudioRouting::new("mic", &[]);
        let probe = routing.probe();
        probe.plug("usb");
        probe.user_selects_input("usb");
        probe.push_change(RoutingChange::Sleep);
        assert_eq!(
            routing.poll_changes(),
            vec![
                RoutingChange::DeviceListChanged,
                RoutingChange::DefaultInputChanged(uid("usb")),
                RoutingChange::Sleep
            ]
        );
        assert!(routing.poll_changes().is_empty());
        assert!(
            probe.set_calls().is_empty(),
            "a user change is not one of our own calls"
        );
        probe.unplug("usb");
        assert!(!routing.device_exists(&uid("usb")));
    }

    #[test]
    fn call_activity_is_what_the_test_declared() {
        let routing = FakeSystemAudioRouting::new("mic", &[]);
        let app = BundleId::new("net.whatsapp.WhatsApp");
        assert_eq!(routing.call_activity(&app), Ok(CallActivity::default()));
        let activity = CallActivity {
            input_running: true,
            input_devices: vec![uid("mic")],
            ..CallActivity::default()
        };
        routing.probe().set_activity(app.as_str(), activity.clone());
        assert_eq!(routing.call_activity(&app), Ok(activity));
    }
}
