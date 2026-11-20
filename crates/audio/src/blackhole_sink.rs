//! macOS virtual-mic injection via BlackHole: opens a BlackHole output device as a playback sink.
//!
//! The sink plays synthesized audio into a **BlackHole** output device. The
//! meeting app is configured to use BlackHole as its *microphone*, so whatever
//! is written here is what the other party hears.
//!
//! The user must install BlackHole and select it as the meeting app's mic. The
//! device is located by name; frames are converted to its rate and channel
//! count by the shared [`PlaybackSink`](crate::playback_sink::PlaybackSink).

use crate::AudioError;
use crate::cpal_output::open_named;
use crate::playback_sink::PlaybackSink;

/// Default substring used to locate the BlackHole output device.
pub const DEFAULT_BLACKHOLE_NAME: &str = "BlackHole";

/// Open the BlackHole output device located by the default name substring.
///
/// # Errors
/// [`AudioError::OutputDeviceUnavailable`] if no matching device is found.
pub fn open_blackhole() -> Result<PlaybackSink, AudioError> {
    open_named(DEFAULT_BLACKHOLE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_device_name_is_reported() {
        assert!(open_named("no such output device \u{1F50A}").is_err());
    }
}
