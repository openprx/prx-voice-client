//! Unit tests for the input-device policy (pure, no hardware).

use super::*;

fn dev(uid: &str, name: &str, transport: InputTransport, is_default: bool) -> InputDevice {
    InputDevice { uid: uid.into(), name: name.into(), transport, is_default }
}

fn builtin(default: bool) -> InputDevice {
    dev("BuiltInMicrophoneDevice", "MacBook Pro Microphone", InputTransport::BuiltIn, default)
}

fn usb(default: bool) -> InputDevice {
    dev("AppleUSBAudioEngine:Blue:Yeti", "Yeti", InputTransport::Usb, default)
}

fn own(default: bool) -> InputDevice {
    dev(OWN_MIC_UID, OWN_MIC_NAME, InputTransport::Virtual, default)
}

fn aggregate(default: bool) -> InputDevice {
    dev("agg-1", "Aggregate Device", InputTransport::Aggregate, default)
}

fn loopback(default: bool) -> InputDevice {
    dev("com.example.loopback", "Loopback", InputTransport::Virtual, default)
}

#[test]
fn configured_device_present_wins_over_default() {
    let list = [builtin(true), usb(false)];
    let sel = choose_input(&list, "AppleUSBAudioEngine:Blue:Yeti").unwrap();
    assert_eq!(sel.device.name, "Yeti");
    assert_eq!(sel.reason, SelectionReason::Configured);
}

#[test]
fn configured_by_name_when_uid_does_not_match() {
    let list = [builtin(true), usb(false)];
    let sel = choose_input(&list, "Yeti").unwrap();
    assert_eq!(sel.device.uid, "AppleUSBAudioEngine:Blue:Yeti");
    assert_eq!(sel.reason, SelectionReason::Configured);
}

#[test]
fn configured_virtual_device_that_is_not_ours_is_honoured() {
    let list = [builtin(true), loopback(false)];
    let sel = choose_input(&list, "com.example.loopback").unwrap();
    assert_eq!(sel.device.name, "Loopback");
}

#[test]
fn configured_device_absent_follows_default_and_says_so() {
    let list = [builtin(true), usb(false)];
    let sel = choose_input(&list, "gone-uid").unwrap();
    assert_eq!(sel.device.name, "MacBook Pro Microphone");
    assert_eq!(sel.reason, SelectionReason::ConfiguredMissing);
}

#[test]
fn default_builtin_is_used() {
    let list = [usb(false), builtin(true)];
    let sel = choose_input(&list, "").unwrap();
    assert_eq!(sel.device.uid, "BuiltInMicrophoneDevice");
    assert_eq!(sel.reason, SelectionReason::SystemDefault);
}

#[test]
fn default_physical_non_builtin_is_followed() {
    let list = [builtin(false), usb(true)];
    let sel = choose_input(&list, "").unwrap();
    assert_eq!(sel.device.name, "Yeti");
    assert_eq!(sel.reason, SelectionReason::SystemDefault);
}

#[test]
fn default_bluetooth_is_followed_and_recorded() {
    let bt = dev("bt-1", "AirPods", InputTransport::Bluetooth, true);
    let list = [builtin(false), bt];
    let sel = choose_input(&list, "").unwrap();
    assert_eq!(sel.device.transport, InputTransport::Bluetooth);
    assert_eq!(sel.reason, SelectionReason::SystemDefault);
}

#[test]
fn default_is_our_virtual_microphone_picks_builtin() {
    let list = [usb(false), own(true), builtin(false)];
    let sel = choose_input(&list, "").unwrap();
    assert_eq!(sel.device.uid, "BuiltInMicrophoneDevice");
    assert_eq!(sel.reason, SelectionReason::DefaultAvoided);
}

#[test]
fn default_is_other_virtual_or_aggregate_picks_builtin() {
    for stand_in in [loopback(true), aggregate(true)] {
        let list = [stand_in, usb(false), builtin(false)];
        let sel = choose_input(&list, "").unwrap();
        assert_eq!(sel.device.uid, "BuiltInMicrophoneDevice");
        assert_eq!(sel.reason, SelectionReason::DefaultAvoided);
    }
}

#[test]
fn default_virtual_without_builtin_picks_first_physical() {
    let list = [aggregate(true), usb(false)];
    let sel = choose_input(&list, "").unwrap();
    assert_eq!(sel.device.name, "Yeti");
    assert_eq!(sel.reason, SelectionReason::DefaultAvoided);
}

#[test]
fn no_default_flag_picks_builtin() {
    let list = [usb(false), builtin(false)];
    let sel = choose_input(&list, "").unwrap();
    assert_eq!(sel.device.uid, "BuiltInMicrophoneDevice");
}

#[test]
fn only_virtual_devices_is_a_typed_error() {
    let list = [own(true), aggregate(false), loopback(false)];
    assert_eq!(choose_input(&list, ""), Err(InputPolicyError::NoUsableDevice));
    assert_eq!(choose_input(&[], ""), Err(InputPolicyError::NoUsableDevice));
}

#[test]
fn configured_own_virtual_microphone_is_rejected_not_replaced() {
    let list = [own(true), builtin(false)];
    for value in [OWN_MIC_UID, OWN_MIC_SINK_UID, OWN_MIC_NAME] {
        let err = choose_input(&list, value).unwrap_err();
        assert_eq!(err, InputPolicyError::OwnVirtualDevice { configured: value.into() });
        assert!(err.to_string().contains("PRX virtual microphone"));
    }
}

#[test]
fn own_device_is_never_a_candidate_even_if_mislabelled() {
    // A driver reporting a physical transport type must still be excluded.
    let disguised = dev(OWN_MIC_UID, OWN_MIC_NAME, InputTransport::Usb, true);
    let list = [disguised.clone()];
    assert_eq!(choose_input(&list, ""), Err(InputPolicyError::NoUsableDevice));
    assert_eq!(
        choose_input(&[disguised, builtin(false)], "").unwrap().device.uid,
        "BuiltInMicrophoneDevice"
    );
}

#[test]
fn selector_first_update_reports_a_change() {
    let mut s = InputSelector::new("");
    let list = [builtin(true)];
    match s.update(&list) {
        SelectionUpdate::Changed { from: None, to } => assert_eq!(to.device.name, "MacBook Pro Microphone"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(s.update(&list), SelectionUpdate::Unchanged);
}

#[test]
fn selector_follows_default_change_mid_session() {
    let mut s = InputSelector::new("");
    s.update(&[builtin(true), usb(false)]);
    // The user switches the system default to the USB microphone.
    match s.update(&[builtin(false), usb(true)]) {
        SelectionUpdate::Changed { from: Some(from), to } => {
            assert_eq!(from.uid, "BuiltInMicrophoneDevice");
            assert_eq!(to.device.name, "Yeti");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn selector_never_moves_to_our_virtual_microphone_when_default_flips() {
    let mut s = InputSelector::new("");
    s.update(&[builtin(true), own(false)]);
    assert_eq!(s.update(&[builtin(false), own(true)]), SelectionUpdate::Unchanged);
    assert_eq!(s.current().unwrap().device.uid, "BuiltInMicrophoneDevice");
}

#[test]
fn selector_device_disappears_then_new_right_device_is_picked() {
    let mut s = InputSelector::new("Yeti");
    s.update(&[builtin(true), usb(false)]);
    assert_eq!(s.current().unwrap().device.name, "Yeti");
    // Unplugged: the configured device is gone, so the default rules apply.
    match s.update(&[builtin(true)]) {
        SelectionUpdate::Changed { from: Some(from), to } => {
            assert_eq!(from.name, "Yeti");
            assert_eq!(to.reason, SelectionReason::ConfiguredMissing);
        }
        other => panic!("unexpected {other:?}"),
    }
    // Plugged in again: back on the configured device.
    match s.update(&[builtin(true), usb(false)]) {
        SelectionUpdate::Changed { to, .. } => assert_eq!(to.reason, SelectionReason::Configured),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn selector_reports_loss_once_and_recovers() {
    let mut s = InputSelector::new("");
    s.update(&[builtin(true)]);
    assert_eq!(s.update(&[own(true)]), SelectionUpdate::Lost(InputPolicyError::NoUsableDevice));
    assert!(s.current().is_none());
    assert_eq!(s.update(&[own(true)]), SelectionUpdate::Unchanged);
    assert!(matches!(
        s.update(&[own(false), usb(true)]),
        SelectionUpdate::Changed { from: None, .. }
    ));
}

#[test]
fn selector_with_own_device_configured_is_lost_with_the_clear_error() {
    let mut s = InputSelector::new(OWN_MIC_UID);
    assert!(matches!(
        s.update(&[own(true), builtin(false)]),
        SelectionUpdate::Lost(InputPolicyError::OwnVirtualDevice { .. })
    ));
}
