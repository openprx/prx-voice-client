//! Opens a cpal input device as a [`CaptureSource`] endpoint.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, StreamTrait};
use prxvc_media::{Clock, SampleRate, SystemClock};
use prxvc_ports::EndpointError;

use crate::AudioError;
use crate::capture_source::{CaptureLinks, CaptureSource, SourceOpener};
use crate::cpal_input_rt::capture_callback;
use crate::cpal_stream::{OpenedStream, SendStream};
use crate::input_devices::find_cpal_input;
use crate::input_policy::InputDevice;

/// Open a cpal-backed capture on an explicitly chosen device (not yet started).
///
/// The device comes from the input-device policy
/// ([`crate::input_policy::choose_input`]); this never opens "the system
/// default input". Frames are stamped on a fresh [`SystemClock`].
///
/// # Errors
/// [`AudioError::InputDeviceUnavailable`] if the device is no longer present.
pub fn open(device: &InputDevice) -> Result<CaptureSource, AudioError> {
    open_with_clock(device, Arc::new(SystemClock::new()))
}

/// [`open`] with frames stamped on `clock`, so that a pipeline shares one clock.
///
/// # Errors
/// As [`open`].
pub fn open_with_clock(
    device: &InputDevice,
    clock: Arc<dyn Clock>,
) -> Result<CaptureSource, AudioError> {
    let cpal_device = find_cpal_input(device)?;
    let config = cpal_device
        .default_input_config()
        .map_err(|e| AudioError::InitializationFailed(format!("input config: {e}")))?;
    let rate = SampleRate::new(config.sample_rate().0)
        .map_err(|e| AudioError::InitializationFailed(e.to_string()))?;
    let channels = config.channels();
    Ok(CaptureSource::new(
        rate,
        channels,
        clock,
        opener(device.clone(), rate, channels),
    ))
}

/// The device is looked up again on every start.
fn opener(device: InputDevice, rate: SampleRate, channels: u16) -> SourceOpener {
    Box::new(move |links: CaptureLinks| {
        let cpal_device = find_cpal_input(&device).map_err(|e| EndpointError::Unavailable {
            reason: e.to_string(),
        })?;
        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(rate.hz()),
            buffer_size: cpal::BufferSize::Default,
        };
        let CaptureLinks { mut writer, lost } = links;
        let stream = cpal_device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    capture_callback(&mut writer, data);
                },
                move |err: cpal::StreamError| {
                    // Not the audio thread: cpal reports stream errors from its own thread.
                    tracing::error!("cpal input stream error: {err}");
                    if matches!(err, cpal::StreamError::DeviceNotAvailable) {
                        lost.store(true, std::sync::atomic::Ordering::Release);
                    }
                },
                None,
            )
            .map_err(|e| EndpointError::Unavailable {
                reason: format!("build input stream: {e}"),
            })?;
        stream.play().map_err(|e| EndpointError::Unavailable {
            reason: format!("play input stream: {e}"),
        })?;
        Ok(OpenedStream {
            handle: Box::new(SendStream(stream)),
            device: device.name.clone(),
        })
    })
}
