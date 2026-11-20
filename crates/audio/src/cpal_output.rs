use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::device::find_output_device;
use crate::output::{OutputCore, OutputStats, PlaybackBuffer};
use crate::pipeline::AudioOutput;
use crate::{AudioError, AudioOutputFrame, AudioState};

/// Wrapper to make `cpal::Stream` usable in `Send` contexts.
///
/// See `cpal_input::SendStream` for the safety rationale.
/// The inner field is read by `Drop` — dropping the stream stops the audio device.
#[allow(dead_code)]
struct SendStream(cpal::Stream);

// SAFETY: The cpal::Stream is created and dropped on the same thread.
// Audio callbacks run on a separate cpal-managed thread and communicate
// via Arc<AtomicBool> / Arc<PlaybackBuffer>, not through the Stream itself.
unsafe impl Send for SendStream {}

/// Real audio output using cpal for playback through a hardware device.
///
/// Frames may have any sample rate, channel count and sample format: they are
/// converted to the device's rate and channel count on write (see
/// [`crate::output`]). The queue is bounded, [`flush`](AudioOutput::flush)
/// discards queued audio, and [`wait_drained`](AudioOutput::wait_drained)
/// blocks until it has all been played.
pub struct CpalAudioOutput {
    state: AudioState,
    stream: Option<SendStream>,
    /// Requested device name; `None` is the system default output.
    device_name: Option<String>,
    core: OutputCore,
    is_running: Arc<AtomicBool>,
}

impl CpalAudioOutput {
    /// Create a new cpal-backed audio output on the default output device.
    ///
    /// Returns an error if no default output device is available.
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::OutputDeviceUnavailable)?;
        Self::from_device(device, None)
    }

    /// Open the output device named `name` (exact name, name fragment, or an
    /// id from [`crate::device::list_audio_devices`]).
    ///
    /// # Errors
    /// [`AudioError::OutputDeviceUnavailable`] if no device matches.
    pub fn with_device_name(name: &str) -> Result<Self, AudioError> {
        let device = find_output_device(name)?;
        Self::from_device(device, Some(name.to_owned()))
    }

    fn from_device(device: cpal::Device, device_name: Option<String>) -> Result<Self, AudioError> {
        let config = device
            .default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("output config: {}", e)))?;
        let core = OutputCore::with_default_capacity(config.sample_rate().0, config.channels())?;
        Ok(Self::with_core(core, device_name))
    }

    fn with_core(core: OutputCore, device_name: Option<String>) -> Self {
        Self {
            state: AudioState::OutputClosed,
            stream: None,
            device_name,
            core,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The sample rate negotiated with the output device.
    pub fn sample_rate(&self) -> u32 {
        self.core.device_rate()
    }

    /// The channel count negotiated with the output device.
    pub fn channels(&self) -> u16 {
        self.core.device_channels()
    }

    /// Shared handle to the playback queue.
    pub fn buffer(&self) -> &Arc<PlaybackBuffer> {
        self.core.buffer()
    }

    fn open_device(&self) -> Result<cpal::Device, AudioError> {
        match &self.device_name {
            Some(name) => find_output_device(name),
            None => cpal::default_host()
                .default_output_device()
                .ok_or(AudioError::OutputDeviceUnavailable),
        }
    }
}

impl AudioOutput for CpalAudioOutput {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.state != AudioState::OutputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::OutputPlaying.to_string(),
            });
        }

        let device = self.open_device()?;

        let config = cpal::StreamConfig {
            channels: self.core.device_channels(),
            sample_rate: cpal::SampleRate(self.core.device_rate()),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::clone(self.core.buffer());
        let is_running = Arc::clone(&self.is_running);

        is_running.store(true, Ordering::Release);

        let data_callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if !is_running.load(Ordering::Acquire) {
                // Fill with silence when not running
                data.fill(0.0);
                return;
            }
            buffer.pull(data);
        };

        let err_callback = |err: cpal::StreamError| {
            tracing::error!("cpal output stream error: {}", err);
        };

        let stream = device
            .build_output_stream(&config, data_callback, err_callback, None)
            .map_err(|e| AudioError::InitializationFailed(format!("build output stream: {}", e)))?;

        stream
            .play()
            .map_err(|e| AudioError::InitializationFailed(format!("play output stream: {}", e)))?;

        self.stream = Some(SendStream(stream));
        self.state = AudioState::OutputPlaying;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.is_running.store(false, Ordering::Release);
        self.stream = None;
        self.core.flush();
        self.state = AudioState::OutputClosed;
        Ok(())
    }

    fn write_frame(&mut self, frame: &AudioOutputFrame) -> Result<(), AudioError> {
        if self.state != AudioState::OutputPlaying {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: "writing".to_string(),
            });
        }
        self.core.write(&frame.frame)
    }

    fn flush(&mut self) -> Result<(), AudioError> {
        self.core.flush();
        Ok(())
    }

    fn end_stream(&mut self) {
        if let Err(e) = self.core.end_stream() {
            tracing::warn!("resampler tail lost at end of stream: {e}");
        }
    }

    fn is_drained(&self) -> bool {
        self.core.buffer().is_drained()
    }

    fn wait_drained(&self, timeout: Duration) -> bool {
        self.core.buffer().wait_drained(timeout)
    }

    fn stats(&self) -> OutputStats {
        self.core.buffer().stats()
    }

    fn state(&self) -> AudioState {
        self.state
    }
}

impl Drop for CpalAudioOutput {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Release);
        // stream is dropped automatically, which stops it
    }
}

#[cfg(test)]
impl CpalAudioOutput {
    /// A sink in the playing state with a given device format and no device
    /// or stream behind it, for tests of the conversion path.
    fn detached(device_rate: u32, device_channels: u16) -> Self {
        let core = OutputCore::with_default_capacity(device_rate, device_channels)
            .expect("valid device format");
        let mut out = Self::with_core(core, None);
        out.state = AudioState::OutputPlaying;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioFormat, AudioFrame};
    use uuid::Uuid;

    fn tone_frame(rate: u32, samples: usize) -> AudioOutputFrame {
        let mut data = Vec::with_capacity(samples * 2);
        for i in 0..samples {
            let v = ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()
                * 0.5
                * i16::MAX as f32) as i16;
            data.extend_from_slice(&v.to_le_bytes());
        }
        AudioOutputFrame {
            session_id: Uuid::nil(),
            frame: AudioFrame {
                sequence_no: 0,
                sample_rate: rate,
                channels: 1,
                format: AudioFormat::I16,
                timestamp_ms: 0,
                data,
            },
        }
    }

    #[test]
    fn one_second_at_16k_and_24k_yields_48000_stereo_frames() {
        for rate in [16_000u32, 24_000] {
            let mut out = CpalAudioOutput::detached(48_000, 2);
            out.write_frame(&tone_frame(rate, rate as usize)).unwrap();
            // The resampler holds back its tail until the stream ends.
            out.end_stream();
            assert_eq!(out.buffer().buffered_samples(), 48_000 * 2, "{rate} Hz");
        }
    }

    #[test]
    fn flush_empties_the_queue_and_reports_drained() {
        let mut out = CpalAudioOutput::detached(48_000, 2);
        out.write_frame(&tone_frame(16_000, 16_000)).unwrap();
        assert!(!out.is_drained());
        out.flush().unwrap();
        assert_eq!(out.buffer().buffered_samples(), 0);
        assert!(out.is_drained());
        assert!(out.wait_drained(Duration::from_millis(1)));
    }

    #[test]
    fn writing_requires_the_playing_state() {
        let core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        let mut out = CpalAudioOutput::with_core(core, None);
        assert!(out.write_frame(&tone_frame(16_000, 160)).is_err());
    }

    #[test]
    fn unknown_device_name_is_reported() {
        let r = CpalAudioOutput::with_device_name("no such output device \u{1F50A}");
        assert!(r.is_err());
    }
}
