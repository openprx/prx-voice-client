//! What the local output device is: headphones, the built-in loudspeakers, or not known (pipeline-design section 6).
//!
//! Core Audio reports a device's connection type and name, not whether a headphone is plugged
//! into the built-in jack, so the built-in output counts as the loudspeakers unless its name says
//! otherwise. A device the rules do not recognise is `Unknown`: no warning, and in `Auto` echo
//! mode the half-duplex gate treats it as speakers (the design's "speakers or unknown route").

use prxvc_audio::core_audio_defaults::{DefaultKind, default_entry};
use prxvc_audio::core_audio_devices::list_devices;
use prxvc_audio::device_directory::DeviceEntry;
use prxvc_audio::input_policy::InputTransport;
use prxvc_config::EchoMode;
use prxvc_session::call::OutputRoute;

const HEADPHONE_WORDS: [&str; 5] = ["headphone", "headset", "airpods", "earbuds", "earphone"];

/// The route of an output device from its connection type and name.
pub(crate) fn classify(transport: InputTransport, name: &str) -> OutputRoute {
    let lower = name.to_lowercase();
    if HEADPHONE_WORDS.iter().any(|w| lower.contains(w)) || transport == InputTransport::Bluetooth {
        return OutputRoute::Headphones;
    }
    if transport == InputTransport::BuiltIn {
        return OutputRoute::Speakers;
    }
    OutputRoute::Unknown
}

/// The device the local output will play to: the configured one by name, else the system default.
pub(crate) fn output_entry(configured: &str) -> Option<DeviceEntry> {
    let configured = configured.trim();
    if configured.is_empty() {
        return default_entry(DefaultKind::Output);
    }
    list_devices()
        .ok()?
        .into_iter()
        .find(|d| d.has_output && d.name.eq_ignore_ascii_case(configured))
}

/// The route of the local output now.
pub(crate) fn current_route(configured: &str) -> OutputRoute {
    output_entry(configured).map_or(OutputRoute::Unknown, |d| classify(d.transport, &d.name))
}

/// Whether the half-duplex echo gate is on for `route` (design 6.3): always in `Always` mode,
/// never in `Off`, and in `Auto` whenever the route is not known to be headphones.
pub(crate) fn gate_enabled(mode: EchoMode, route: OutputRoute) -> bool {
    match mode {
        EchoMode::Always => true,
        EchoMode::Off => false,
        EchoMode::Auto => route != OutputRoute::Headphones,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_output_is_the_loudspeakers_unless_its_name_says_headphones() {
        assert_eq!(
            classify(InputTransport::BuiltIn, "MacBook Pro Speakers"),
            OutputRoute::Speakers
        );
        assert_eq!(
            classify(InputTransport::BuiltIn, "External Headphones"),
            OutputRoute::Headphones
        );
    }

    #[test]
    fn bluetooth_and_named_headsets_are_headphones() {
        assert_eq!(
            classify(InputTransport::Bluetooth, "WH-1000XM5"),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify(InputTransport::Usb, "Jabra Evolve2 Headset"),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify(InputTransport::Other, "AirPods Pro"),
            OutputRoute::Headphones
        );
    }

    #[test]
    fn anything_else_is_not_known() {
        assert_eq!(
            classify(InputTransport::Usb, "Audio Interface"),
            OutputRoute::Unknown
        );
        assert_eq!(
            classify(InputTransport::Virtual, "BlackHole"),
            OutputRoute::Unknown
        );
    }

    #[test]
    fn the_gate_follows_the_mode_and_the_route() {
        for route in [
            OutputRoute::Unknown,
            OutputRoute::Speakers,
            OutputRoute::Headphones,
        ] {
            assert!(gate_enabled(EchoMode::Always, route));
            assert!(!gate_enabled(EchoMode::Off, route));
        }
        assert!(gate_enabled(EchoMode::Auto, OutputRoute::Speakers));
        assert!(gate_enabled(EchoMode::Auto, OutputRoute::Unknown));
        assert!(!gate_enabled(EchoMode::Auto, OutputRoute::Headphones));
    }
}
