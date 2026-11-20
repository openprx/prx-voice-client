//! The audio devices and the input policy's verdict, as the settings window shows them.
//!
//! The lists come from `prxvc-audio` (read-only queries: `input_devices::list_input_devices`,
//! `device_directory::list_devices`, `input_policy::choose_input`); the mapping to view models is
//! pure. Our own virtual microphone and its sink are never offered.

use prxvc_audio::device_directory::DeviceEntry;
use prxvc_audio::input_policy::{
    self, InputDevice, InputPolicyError, InputTransport, OWN_MIC_NAME, OWN_MIC_SINK_UID,
    OWN_MIC_UID, SelectionReason,
};
use prxvc_desktop_ui::settings_data::{
    AudioData, DeviceView, InputChoiceView, InputReason, TransportView,
};

/// Read the devices of this machine and decide the input for `configured_input`.
pub(crate) fn probe(configured_input: &str) -> AudioData {
    let inputs = prxvc_audio::input_devices::list_input_devices();
    let outputs = prxvc_audio::device_directory::list_devices();
    let list_error = inputs
        .as_ref()
        .err()
        .map(ToString::to_string)
        .or_else(|| outputs.as_ref().err().map(ToString::to_string));
    build(
        &inputs.unwrap_or_default(),
        &outputs.unwrap_or_default(),
        prxvc_audio::device::default_output_device_name(),
        configured_input,
        list_error,
    )
}

/// Build the view from device lists (pure).
pub(crate) fn build(
    inputs: &[InputDevice],
    directory: &[DeviceEntry],
    default_output: Option<String>,
    configured_input: &str,
    list_error: Option<String>,
) -> AudioData {
    let physical: Vec<&InputDevice> = inputs.iter().filter(|d| !d.is_own_virtual()).collect();
    let input_views = physical.iter().map(|d| input_view(d)).collect();
    let output_views = directory
        .iter()
        .filter(|d| d.has_output && !d.is_hidden && d.is_alive && !is_own(&d.uid, &d.name))
        .map(|d| DeviceView {
            uid: d.uid.clone(),
            name: d.name.clone(),
            transport: transport_view(d.transport),
            is_default: default_output.as_deref() == Some(d.name.as_str()),
        })
        .collect();
    AudioData {
        inputs: input_views,
        outputs: output_views,
        input_choice: Some(choice_view(input_policy::choose_input(
            inputs,
            configured_input,
        ))),
        default_input: input_policy::choose_input(inputs, "")
            .ok()
            .map(|s| s.device.name),
        default_output,
        list_error,
    }
}

fn is_own(uid: &str, name: &str) -> bool {
    uid == OWN_MIC_UID || uid == OWN_MIC_SINK_UID || name == OWN_MIC_NAME
}

fn input_view(d: &InputDevice) -> DeviceView {
    DeviceView {
        uid: d.uid.clone(),
        name: d.name.clone(),
        transport: transport_view(d.transport),
        is_default: d.is_default,
    }
}

fn transport_view(t: InputTransport) -> TransportView {
    match t {
        InputTransport::BuiltIn => TransportView::BuiltIn,
        InputTransport::Usb => TransportView::Usb,
        InputTransport::Bluetooth => TransportView::Bluetooth,
        InputTransport::Virtual => TransportView::Virtual,
        InputTransport::Aggregate => TransportView::Aggregate,
        InputTransport::Other => TransportView::Other,
        InputTransport::Unknown => TransportView::Unknown,
    }
}

fn choice_view(choice: Result<input_policy::InputSelection, InputPolicyError>) -> InputChoiceView {
    match choice {
        Ok(selection) => InputChoiceView::Chosen {
            name: selection.device.name,
            reason: match selection.reason {
                SelectionReason::Configured => InputReason::Configured,
                SelectionReason::SystemDefault => InputReason::SystemDefault,
                SelectionReason::DefaultAvoided => InputReason::DefaultAvoided,
                SelectionReason::ConfiguredMissing => InputReason::ConfiguredMissing,
            },
        },
        Err(InputPolicyError::OwnVirtualDevice { .. }) => InputChoiceView::OwnVirtualDevice,
        Err(InputPolicyError::NoUsableDevice) => InputChoiceView::NoUsableDevice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(uid: &str, name: &str, transport: InputTransport, is_default: bool) -> InputDevice {
        InputDevice {
            uid: uid.into(),
            name: name.into(),
            transport,
            is_default,
        }
    }

    fn entry(uid: &str, name: &str, has_output: bool, hidden: bool, alive: bool) -> DeviceEntry {
        DeviceEntry {
            uid: uid.into(),
            name: name.into(),
            transport: InputTransport::BuiltIn,
            has_input: !has_output,
            has_output,
            is_alive: alive,
            is_hidden: hidden,
        }
    }

    fn devices() -> Vec<InputDevice> {
        vec![
            input(
                "builtin",
                "Built-in Microphone",
                InputTransport::BuiltIn,
                false,
            ),
            input("usb", "USB Mic", InputTransport::Usb, false),
            input(OWN_MIC_UID, OWN_MIC_NAME, InputTransport::Usb, true),
        ]
    }

    #[test]
    fn our_own_virtual_microphone_is_never_offered_as_an_input() {
        let data = build(&devices(), &[], None, "", None);
        let uids: Vec<&str> = data.inputs.iter().map(|d| d.uid.as_str()).collect();
        assert_eq!(uids, ["builtin", "usb"]);
    }

    #[test]
    fn system_default_shows_the_device_the_policy_would_choose() {
        // The system default is our own virtual microphone: the policy avoids it.
        let data = build(&devices(), &[], None, "", None);
        assert_eq!(data.default_input.as_deref(), Some("Built-in Microphone"));
        assert_eq!(
            data.input_choice,
            Some(InputChoiceView::Chosen {
                name: "Built-in Microphone".into(),
                reason: InputReason::DefaultAvoided
            })
        );
    }

    #[test]
    fn a_configured_microphone_that_is_missing_is_reported_with_the_stand_in() {
        let data = build(&devices(), &[], None, "gone", None);
        assert_eq!(
            data.input_choice,
            Some(InputChoiceView::Chosen {
                name: "Built-in Microphone".into(),
                reason: InputReason::ConfiguredMissing
            })
        );
        let data = build(&devices(), &[], None, "usb", None);
        assert_eq!(
            data.input_choice,
            Some(InputChoiceView::Chosen {
                name: "USB Mic".into(),
                reason: InputReason::Configured
            })
        );
    }

    #[test]
    fn choosing_our_own_microphone_or_having_none_is_a_verdict_not_a_crash() {
        let data = build(&devices(), &[], None, OWN_MIC_UID, None);
        assert_eq!(data.input_choice, Some(InputChoiceView::OwnVirtualDevice));
        let data = build(&[], &[], None, "", Some("no list".into()));
        assert_eq!(data.input_choice, Some(InputChoiceView::NoUsableDevice));
        assert_eq!(data.default_input, None);
        assert_eq!(data.list_error.as_deref(), Some("no list"));
    }

    #[test]
    fn outputs_skip_inputs_hidden_dead_and_our_own_devices() {
        let directory = [
            entry("speakers", "Speakers", true, false, true),
            entry("headphones", "Headphones", true, false, true),
            entry("mic-only", "Mic", false, false, true),
            entry("hidden", "Hidden", true, true, true),
            entry("dead", "Dead", true, false, false),
            entry(
                OWN_MIC_SINK_UID,
                "PRX Voice Microphone Sink",
                true,
                true,
                true,
            ),
            entry(OWN_MIC_UID, OWN_MIC_NAME, true, false, true),
        ];
        let data = build(&[], &directory, Some("Headphones".into()), "", None);
        let names: Vec<(&str, bool)> = data
            .outputs
            .iter()
            .map(|d| (d.name.as_str(), d.is_default))
            .collect();
        assert_eq!(names, [("Speakers", false), ("Headphones", true)]);
        assert_eq!(data.default_output.as_deref(), Some("Headphones"));
    }

    #[test]
    fn every_transport_has_a_view() {
        for (t, v) in [
            (InputTransport::BuiltIn, TransportView::BuiltIn),
            (InputTransport::Usb, TransportView::Usb),
            (InputTransport::Bluetooth, TransportView::Bluetooth),
            (InputTransport::Virtual, TransportView::Virtual),
            (InputTransport::Aggregate, TransportView::Aggregate),
            (InputTransport::Other, TransportView::Other),
            (InputTransport::Unknown, TransportView::Unknown),
        ] {
            assert_eq!(transport_view(t), v);
        }
    }
}
