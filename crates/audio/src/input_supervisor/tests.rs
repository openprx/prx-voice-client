//! Tests for the capture supervisor with a fake device backend.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};

use prxvc_media::SampleRate;

use super::*;
use crate::input_policy::{InputTransport, OWN_MIC_NAME, OWN_MIC_UID};
use crate::mock::MockAudioSource;

#[derive(Default)]
struct World {
    devices: Vec<InputDevice>,
    opened: Vec<String>,
}

struct FakeBackend(Rc<RefCell<World>>);

impl InputBackend for FakeBackend {
    fn list(&mut self) -> Result<Vec<InputDevice>, AudioError> {
        Ok(self.0.borrow().devices.clone())
    }
    fn open(
        &mut self,
        _session_id: Uuid,
        device: &InputDevice,
    ) -> Result<Box<dyn AudioSource>, AudioError> {
        self.0.borrow_mut().opened.push(device.uid.clone());
        Ok(Box::new(MockAudioSource::new(SampleRate::HZ_16000)))
    }
}

fn dev(uid: &str, name: &str, transport: InputTransport, is_default: bool) -> InputDevice {
    InputDevice {
        uid: uid.into(),
        name: name.into(),
        transport,
        is_default,
    }
}

fn builtin(d: bool) -> InputDevice {
    dev("builtin", "Built-in", InputTransport::BuiltIn, d)
}
fn usb(d: bool) -> InputDevice {
    dev("usb", "USB Mic", InputTransport::Usb, d)
}
fn own(d: bool) -> InputDevice {
    dev(OWN_MIC_UID, OWN_MIC_NAME, InputTransport::Virtual, d)
}

fn setup(
    pref: &str,
    devices: Vec<InputDevice>,
) -> (
    InputSupervisor<FakeBackend>,
    Rc<RefCell<World>>,
    Receiver<AudioLoopEvent>,
) {
    let world = Rc::new(RefCell::new(World {
        devices,
        opened: Vec::new(),
    }));
    let (tx, rx) = channel();
    let sup = InputSupervisor::new(FakeBackend(Rc::clone(&world)), Uuid::nil(), pref, tx);
    (sup, world, rx)
}

fn later(t: Instant, secs: u64) -> Instant {
    t + Duration::from_secs(secs)
}

#[test]
fn start_opens_builtin_when_default_is_our_virtual_microphone() {
    let (mut sup, world, rx) = setup("", vec![own(true), builtin(false)]);
    sup.start(Instant::now()).unwrap();
    assert_eq!(world.borrow().opened, ["builtin"]);
    match rx.try_recv().unwrap() {
        AudioLoopEvent::InputOpened {
            device,
            reason,
            previous,
        } => {
            assert_eq!(device.uid, "builtin");
            assert_eq!(reason, SelectionReason::DefaultAvoided);
            assert!(previous.is_none());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn start_with_only_virtual_devices_is_a_typed_error() {
    let (mut sup, world, _rx) = setup("", vec![own(true)]);
    let err = sup.start(Instant::now()).unwrap_err();
    assert!(matches!(err, AudioError::InputPolicy(_)));
    assert!(world.borrow().opened.is_empty());
}

#[test]
fn start_with_configured_own_microphone_is_rejected() {
    let (mut sup, world, _rx) = setup(OWN_MIC_UID, vec![own(true), builtin(false)]);
    let err = sup.start(Instant::now()).unwrap_err();
    assert!(err.to_string().contains("PRX virtual microphone"));
    assert!(world.borrow().opened.is_empty());
}

#[test]
fn default_change_restarts_capture_on_the_new_device() {
    let t0 = Instant::now();
    let (mut sup, world, rx) = setup("", vec![builtin(true), usb(false)]);
    sup.start(t0).unwrap();
    rx.try_recv().unwrap();

    // Before the poll interval nothing is read.
    world.borrow_mut().devices = vec![builtin(false), usb(true)];
    assert!(!sup.tick(later(t0, 0)));
    assert!(sup.tick(later(t0, 2)));
    assert_eq!(world.borrow().opened, ["builtin", "usb"]);
    match rx.try_recv().unwrap() {
        AudioLoopEvent::InputOpened {
            device, previous, ..
        } => {
            assert_eq!(device.uid, "usb");
            assert_eq!(previous.unwrap().uid, "builtin");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn vanished_device_stops_capture_without_falling_back_to_virtual() {
    let t0 = Instant::now();
    let (mut sup, world, rx) = setup("", vec![builtin(true)]);
    sup.start(t0).unwrap();
    rx.try_recv().unwrap();

    world.borrow_mut().devices = vec![own(true)];
    assert!(sup.tick(later(t0, 2)));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioLoopEvent::InputUnavailable { .. }
    ));
    assert!(sup.read_frame().is_none());
    assert_eq!(world.borrow().opened, ["builtin"]);

    // The loss is reported once, then capture resumes when a device returns.
    assert!(!sup.tick(later(t0, 4)));
    assert!(rx.try_recv().is_err());
    world.borrow_mut().devices = vec![own(false), usb(true)];
    assert!(sup.tick(later(t0, 6)));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioLoopEvent::InputOpened { .. }
    ));
    assert!(sup.read_frame().is_some());
}
