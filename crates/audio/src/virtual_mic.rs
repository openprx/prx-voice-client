//! Virtual-microphone endpoint: an `AudioSink` that opens the driver's hidden sink device by UID and feeds it 48 kHz mono audio through the playback path.
//!
//! What the other party of a call hears is whatever is written here: the
//! driver echoes its hidden sink into the visible "PRX Voice Microphone".
//!
//! * The device is chosen by **UID**, never by a name. The driver's sink is
//!   hidden, which Core Audio leaves out of the device list that cpal
//!   enumerates, so the stream is opened through `core_audio_output`, which
//!   resolves the UID itself.
//! * The sink UID is a parameter. Empty (the config default of
//!   `audio.virtual_mic_device`) means our driver's sink
//!   ([`OWN_MIC_SINK_UID`]). Any other UID is "an output device chosen by UID":
//!   a third-party virtual device works that way, is never detected or
//!   suggested, and is not subject to the driver checks below.
//! * For our driver the endpoint refuses to open when [`DriverStatus::check`]
//!   finds a problem (missing, not loaded, stale, wrong variant) and when the
//!   sink's format is not the driver's fixed 48 kHz mono. Each is a typed
//!   [`VirtualMicError`], so the session can show the right action.
//! * Resampling, the bounded ring, the real-time callback and the stream
//!   controls are those of [`PlaybackSink`]; this file only chooses and checks
//!   the device.

use std::time::Duration;

use prxvc_media::AudioFrame;
use prxvc_ports::{AudioSink, EndpointError, EndpointHealth, SinkInfo, WriteOutcome};

use crate::AudioError;
use crate::core_audio_output::{open as open_stream, output_format};
use crate::device_directory::{DeviceEntry, find_by_uid, list_devices_with};
use crate::driver_status::{DriverIssue, DriverStatus, DriverVariant};
use crate::input_policy::{OWN_MIC_SINK_UID, OWN_MIC_UID};
use crate::output::DEFAULT_BUFFER_SECONDS;
use crate::output::OutputStats;
use crate::playback_sink::{PlaybackSink, SinkOpener};
use crate::stream_sink::StreamSink;

/// The driver's fixed format.
const DRIVER_RATE: u32 = 48_000;
const DRIVER_CHANNELS: u16 = 1;

/// Why the virtual microphone sink could not be opened.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VirtualMicError {
    /// Our driver is not usable; the issue says how (install, update, ...).
    #[error("{0}")]
    Driver(#[from] DriverIssue),
    /// The chosen output device is not listed, or is not an output.
    #[error("output device `{uid}` is not present")]
    DeviceMissing {
        /// The UID asked for.
        uid: String,
    },
    /// Our sink is not at the driver's fixed format.
    #[error(
        "the virtual microphone sink runs at {rate} Hz with {channels} channel(s); 48000 Hz mono is required"
    )]
    UnexpectedFormat {
        /// Rate the device reports.
        rate: u32,
        /// Channel count the device reports.
        channels: u16,
    },
    /// The device was found but could not be set up.
    #[error("{0}")]
    Audio(#[from] AudioError),
}

/// What to open.
#[derive(Debug, Clone, Default)]
pub struct VirtualMicOptions {
    /// UID of the output device; empty means our driver's hidden sink.
    pub sink_uid: String,
    /// The driver variant the call app needs (`None` accepts any). Only
    /// checked for our own driver.
    pub required_variant: Option<DriverVariant>,
}

impl VirtualMicOptions {
    /// Options for the configured `audio.virtual_mic_device` value.
    pub fn from_config(virtual_mic_device: &str, required_variant: Option<DriverVariant>) -> Self {
        Self {
            sink_uid: virtual_mic_device.trim().to_owned(),
            required_variant,
        }
    }

    /// The UID that will be opened.
    pub fn effective_uid(&self) -> &str {
        if self.sink_uid.is_empty() {
            OWN_MIC_SINK_UID
        } else {
            &self.sink_uid
        }
    }

    /// Whether this is our own driver's sink.
    pub fn is_own_driver(&self) -> bool {
        self.effective_uid() == OWN_MIC_SINK_UID
    }
}

/// Decide, without touching a device, which output device to open.
///
/// Our driver: the status check runs first, then the sink must be listed.
/// Another UID: only presence as an output device is required.
///
/// # Errors
/// [`VirtualMicError::Driver`] or [`VirtualMicError::DeviceMissing`].
pub fn plan_device<'a>(
    options: &VirtualMicOptions,
    status: &DriverStatus,
    devices: &'a [DeviceEntry],
) -> Result<&'a DeviceEntry, VirtualMicError> {
    let uid = options.effective_uid();
    if options.is_own_driver() {
        status.check(options.required_variant)?;
    }
    find_by_uid(devices, uid)
        .filter(|d| d.has_output)
        .ok_or_else(|| VirtualMicError::DeviceMissing {
            uid: uid.to_owned(),
        })
}

/// The driver's sink must be 48 kHz mono; other devices may be anything.
///
/// # Errors
/// [`VirtualMicError::UnexpectedFormat`].
pub fn check_format(
    options: &VirtualMicOptions,
    rate: u32,
    channels: u16,
) -> Result<(), VirtualMicError> {
    if options.is_own_driver() && (rate != DRIVER_RATE || channels != DRIVER_CHANNELS) {
        return Err(VirtualMicError::UnexpectedFormat { rate, channels });
    }
    Ok(())
}

/// The playback endpoint of the virtual microphone (not yet started).
///
/// Implements [`AudioSink`] and [`StreamSink`] by delegating to the
/// [`PlaybackSink`] it wraps.
pub struct VirtualMicSink {
    inner: PlaybackSink,
    uid: String,
}

impl VirtualMicSink {
    /// Detect the driver and the devices of this machine and open the sink.
    ///
    /// # Errors
    /// See [`VirtualMicError`].
    pub fn open(options: &VirtualMicOptions) -> Result<Self, VirtualMicError> {
        let devices = list_devices_with(&[OWN_MIC_UID, options.effective_uid()])?;
        let status = DriverStatus::detect();
        Self::open_with(options, &status, &devices)
    }

    /// [`open`](Self::open) with the status and device list given.
    ///
    /// # Errors
    /// See [`VirtualMicError`].
    pub fn open_with(
        options: &VirtualMicOptions,
        status: &DriverStatus,
        devices: &[DeviceEntry],
    ) -> Result<Self, VirtualMicError> {
        let device = plan_device(options, status, devices)?;
        let (rate, channels) = output_format(&device.uid)
            .map_err(|e| AudioError::InitializationFailed(e.to_string()))?;
        check_format(options, rate, channels)?;
        let (uid, name) = (device.uid.clone(), device.name.clone());
        let opener: SinkOpener =
            Box::new(move |buffer, running| open_stream(&uid, &name, buffer, running));
        let inner = PlaybackSink::new(rate, channels, DEFAULT_BUFFER_SECONDS, opener)?;
        Self::wrap(options, inner)
    }

    /// Wrap an opened playback sink after checking its format.
    ///
    /// # Errors
    /// [`VirtualMicError::UnexpectedFormat`].
    pub fn wrap(options: &VirtualMicOptions, inner: PlaybackSink) -> Result<Self, VirtualMicError> {
        check_format(options, inner.sample_rate(), inner.channels())?;
        Ok(Self {
            inner,
            uid: options.effective_uid().to_owned(),
        })
    }

    /// The UID of the device this endpoint writes to.
    pub fn uid(&self) -> &str {
        &self.uid
    }

    /// The wrapped playback sink (ring, device format).
    pub fn playback(&self) -> &PlaybackSink {
        &self.inner
    }
}

impl AudioSink for VirtualMicSink {
    fn start(&mut self) -> Result<SinkInfo, EndpointError> {
        self.inner.start()
    }

    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
        self.inner.write(frame)
    }

    fn flush(&mut self) {
        self.inner.flush();
    }

    fn queued(&self) -> Duration {
        self.inner.queued()
    }

    fn health(&self) -> EndpointHealth {
        self.inner.health()
    }

    fn stop(&mut self) {
        self.inner.stop();
    }
}

impl StreamSink for VirtualMicSink {
    fn end_stream(&mut self) {
        self.inner.end_stream();
    }

    fn is_drained(&self) -> bool {
        self.inner.is_drained()
    }

    fn wait_drained(&self, timeout: Duration) -> bool {
        self.inner.wait_drained(timeout)
    }

    fn stats(&self) -> OutputStats {
        self.inner.stats()
    }
}

#[cfg(test)]
mod tests;
