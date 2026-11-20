//! Tests of the supervised microphone source with a fake device backend.

use std::sync::Arc;

use parking_lot::Mutex;

use super::*;
use crate::AudioError;
use crate::input_policy::{InputTransport, OWN_MIC_NAME, OWN_MIC_UID};
use crate::mock::MockAudioSource;

#[derive(Default)]
struct World {
    devices: Vec<InputDevice>,
    opened: Vec<String>,
}

struct FakeBackend(Arc<Mutex<World>>);

impl InputBackend for FakeBackend {
    fn list(&mut self) -> Result<Vec<InputDevice>, AudioError> {
        Ok(self.0.lock().devices.clone())
    }

    fn open(&mut self, _id: Uuid, device: &InputDevice) -> Result<Box<dyn AudioSource>, AudioError> {
        self.0.lock().opened.push(device.uid.clone());
        Ok(Box::new(MockAudioSource::new(SampleRate::HZ_16000)))
    }
}

fn dev(uid: &str, name: &str, transport: InputTransport, is_default: bool) -> InputDevice {
    InputDevice { uid: uid.into(), name: name.into(), transport, is_default }
}

fn setup(devices: Vec<InputDevice>) -> (SupervisedMic<FakeBackend>, Arc<Mutex<World>>, EventQueue) {
    let world = Arc::new(Mutex::new(World { devices, opened: Vec::new() }));
    let events = EventQueue::new();
    let mic = SupervisedMic::new(FakeBackend(Arc::clone(&world)), "", events.clone());
    (mic, world, events)
}

fn builtin(default: bool) -> InputDevice {
    dev("builtin", "Built-in Mic", InputTransport::BuiltIn, default)
}

fn own(default: bool) -> InputDevice {
    dev(OWN_MIC_UID, OWN_MIC_NAME, InputTransport::Virtual, default)
}

#[test]
fn start_reports_the_chosen_device_and_never_opens_our_microphone() {
    let (mut mic, world, events) = setup(vec![own(true), builtin(false)]);
    let info = mic.start().expect("start");
    assert_eq!(info.device, "Built-in Mic");
    assert_eq!(world.lock().opened, ["builtin"]);
    assert!(events.drain().is_empty(), "the first choice is not a change");
    assert_eq!(mic.device().map(|d| d.uid.as_str()), Some("builtin"));
}

#[test]
fn start_with_no_physical_microphone_is_a_typed_unavailable_error() {
    let (mut mic, world, _events) = setup(vec![own(true)]);
    let err = mic.start().unwrap_err();
    assert!(matches!(err, EndpointError::Unavailable { .. }), "{err:?}");
    assert!(world.lock().opened.is_empty());
}

#[test]
fn read_delivers_frames_and_counts_them() {
    let (mut mic, _world, _events) = setup(vec![builtin(true)]);
    assert_eq!(mic.read(&mut Vec::new(), Duration::ZERO), Err(EndpointError::NotStarted));
    mic.start().unwrap();
    let mut out = Vec::new();
    mic.read(&mut out, Duration::from_millis(50)).unwrap();
    assert!(!out.is_empty());
    assert_eq!(mic.health().frames, out.len() as u64);
    mic.stop();
    assert!(!mic.health().running);
}

#[test]
fn losing_the_microphone_is_an_event_not_an_error_and_capture_resumes() {
    let (mut mic, world, events) = setup(vec![builtin(true)]);
    mic.start().unwrap();
    world.lock().devices = vec![own(true)];
    // The supervisor re-reads the device list once a second.
    std::thread::sleep(Duration::from_millis(1_100));
    let mut out = Vec::new();
    mic.read(&mut out, Duration::from_millis(5)).expect("not an error");
    assert!(out.is_empty());
    assert!(matches!(events.drain().as_slice(), [PumpEvent::DeviceUnavailable { .. }]));
    assert!(mic.device().is_none());

    world.lock().devices = vec![own(false), dev("usb", "USB Mic", InputTransport::Usb, true)];
    std::thread::sleep(Duration::from_millis(1_100));
    mic.read(&mut out, Duration::from_millis(50)).unwrap();
    assert_eq!(
        events.drain(),
        [PumpEvent::DeviceChanged { device: "USB Mic".into() }]
    );
    assert!(!out.is_empty(), "capture resumed on the new device");
}
