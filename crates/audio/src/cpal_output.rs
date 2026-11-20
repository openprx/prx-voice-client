//! Opens cpal output devices (the default output or a named one) as [`PlaybackSink`] endpoints.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use prxvc_ports::EndpointError;

use crate::AudioError;
use crate::cpal_output_rt::fill_output;
use crate::cpal_stream::{OpenedStream, SendStream};
use crate::device::find_output_device;
use crate::output::DEFAULT_BUFFER_SECONDS;
use crate::playback_buffer::PlaybackBuffer;
use crate::playback_sink::{PlaybackSink, SinkOpener};

/// Open the system default output device as a sink (not yet started).
///
/// # Errors
/// [`AudioError::OutputDeviceUnavailable`] if there is no default output.
pub fn open_default() -> Result<PlaybackSink, AudioError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(AudioError::OutputDeviceUnavailable)?;
    sink_for(&device, None)
}

/// Open the output device named `name` (exact name, name fragment, or an id
/// from [`crate::device::list_audio_devices`]) as a sink (not yet started).
///
/// # Errors
/// [`AudioError::OutputDeviceUnavailable`] if no device matches.
pub fn open_named(name: &str) -> Result<PlaybackSink, AudioError> {
    let device = find_output_device(name)?;
    sink_for(&device, Some(name.to_owned()))
}

fn sink_for(device: &cpal::Device, name: Option<String>) -> Result<PlaybackSink, AudioError> {
    let config = device
        .default_output_config()
        .map_err(|e| AudioError::InitializationFailed(format!("output config: {e}")))?;
    let (rate, channels) = (config.sample_rate().0, config.channels());
    PlaybackSink::new(
        rate,
        channels,
        DEFAULT_BUFFER_SECONDS,
        opener(name, rate, channels),
    )
}

/// The device is looked up again on every start, so a restart picks up a
/// device that was replugged.
fn opener(name: Option<String>, rate: u32, channels: u16) -> SinkOpener {
    Box::new(
        move |buffer: &Arc<PlaybackBuffer>, running: &Arc<AtomicBool>| {
            let device = match &name {
                Some(name) => find_output_device(name),
                None => cpal::default_host()
                    .default_output_device()
                    .ok_or(AudioError::OutputDeviceUnavailable),
            }
            .map_err(unavailable)?;
            let config = cpal::StreamConfig {
                channels,
                sample_rate: cpal::SampleRate(rate),
                buffer_size: cpal::BufferSize::Default,
            };
            let (buffer, running) = (Arc::clone(buffer), Arc::clone(running));
            let stream = device
                .build_output_stream(
                    &config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        fill_output(&running, &buffer, data);
                    },
                    |err: cpal::StreamError| {
                        // Not the audio thread: cpal reports stream errors from its own thread.
                        tracing::error!("cpal output stream error: {err}");
                    },
                    None,
                )
                .map_err(|e| EndpointError::Unavailable {
                    reason: format!("build output stream: {e}"),
                })?;
            stream.play().map_err(|e| EndpointError::Unavailable {
                reason: format!("play output stream: {e}"),
            })?;
            Ok(OpenedStream {
                handle: Box::new(SendStream(stream)),
                device: device.name().unwrap_or_else(|_| "output device".to_owned()),
            })
        },
    )
}

fn unavailable(e: AudioError) -> EndpointError {
    EndpointError::Unavailable {
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_device_name_is_reported() {
        let r = open_named("no such output device \u{1F50A}");
        assert!(r.is_err());
    }
}
