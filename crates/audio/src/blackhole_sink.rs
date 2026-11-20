//! macOS virtual-mic injection via BlackHole.
//!
//! [`BlackHoleSink`] is a [`VirtualMicSink`] that plays synthesized audio into a
//! **BlackHole** output device. The meeting app is configured to use BlackHole
//! as its *microphone*, so whatever we write here is what the other party hears
//! — i.e. the cloned-voice English for direction A.
//!
//! The user must install BlackHole and select it as the meeting app's mic. This
//! sink locates the device by name, converts every frame to the device's rate
//! and channel count (see [`crate::output`]) and streams f32 samples into it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};

use crate::device::find_output_device;
use crate::output::{OutputCore, OutputStats, PlaybackBuffer};
use crate::routing::VirtualMicSink;
use crate::{AudioError, AudioFrame, AudioState};

/// Default substring used to locate the BlackHole output device.
pub const DEFAULT_BLACKHOLE_NAME: &str = "BlackHole";

/// Wrapper making `cpal::Stream` usable in `Send` contexts (same rationale as
/// `cpal_output::SendStream`): the stream is created/dropped on one thread and
/// the audio callback communicates only via `Arc`-shared state.
struct SendStream(#[allow(dead_code)] cpal::Stream);

// SAFETY: see module note — the stream itself is never touched from the audio
// callback thread; shared state goes through Arc<AtomicBool>/Arc<Mutex<_>>.
unsafe impl Send for SendStream {}

/// Injects audio into a BlackHole device so the meeting app's mic picks it up.
pub struct BlackHoleSink {
    device_name: String,
    state: AudioState,
    stream: Option<SendStream>,
    core: OutputCore,
    is_running: Arc<AtomicBool>,
}

impl BlackHoleSink {
    /// Locate a BlackHole output device by the default name substring.
    pub fn new() -> Result<Self, AudioError> {
        Self::with_device_name(DEFAULT_BLACKHOLE_NAME)
    }

    /// Locate an output device by name: an exact name, a name fragment, or an
    /// id from [`crate::device::list_audio_devices`]. An exact match wins over
    /// a fragment match.
    ///
    /// # Errors
    /// [`AudioError::OutputDeviceUnavailable`] if no matching device is found.
    pub fn with_device_name(name_substr: &str) -> Result<Self, AudioError> {
        let device = find_output_device(name_substr)?;
        let config = device
            .default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("blackhole config: {e}")))?;
        let core = OutputCore::with_default_capacity(config.sample_rate().0, config.channels())?;
        Ok(Self::with_core(name_substr, core))
    }

    fn with_core(device_name: &str, core: OutputCore) -> Self {
        Self {
            device_name: device_name.to_owned(),
            state: AudioState::OutputClosed,
            stream: None,
            core,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sample rate negotiated with the BlackHole device (typically 48 kHz).
    pub fn sample_rate(&self) -> u32 {
        self.core.device_rate()
    }

    /// Channel count of the BlackHole device (typically 2).
    pub fn channels(&self) -> u16 {
        self.core.device_channels()
    }

    /// Shared handle to the playback queue.
    pub fn buffer(&self) -> &Arc<PlaybackBuffer> {
        self.core.buffer()
    }
}

impl VirtualMicSink for BlackHoleSink {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.state != AudioState::OutputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::OutputPlaying.to_string(),
            });
        }
        let device = find_output_device(&self.device_name)?;
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
                data.fill(0.0);
                return;
            }
            buffer.pull(data);
        };
        let err_callback = |err: cpal::StreamError| {
            tracing::error!("blackhole sink stream error: {err}");
        };

        let stream = device
            .build_output_stream(&config, data_callback, err_callback, None)
            .map_err(|e| AudioError::InitializationFailed(format!("build blackhole stream: {e}")))?;
        stream
            .play()
            .map_err(|e| AudioError::InitializationFailed(format!("play blackhole stream: {e}")))?;

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

    fn write(&mut self, frame: &AudioFrame) -> Result<(), AudioError> {
        if self.state != AudioState::OutputPlaying {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: "writing".to_owned(),
            });
        }
        self.core.write(frame)
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

impl Drop for BlackHoleSink {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
impl BlackHoleSink {
    /// A sink in the playing state with a given device format and no device
    /// or stream behind it, for tests of the conversion path.
    fn detached(device_rate: u32, device_channels: u16) -> Self {
        let core = OutputCore::with_default_capacity(device_rate, device_channels)
            .expect("valid device format");
        let mut sink = Self::with_core(DEFAULT_BLACKHOLE_NAME, core);
        sink.state = AudioState::OutputPlaying;
        sink
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioFormat;

    fn f32_tone(rate: u32, samples: usize) -> AudioFrame {
        let mut data = Vec::with_capacity(samples * 4);
        for i in 0..samples {
            let v = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5;
            data.extend_from_slice(&v.to_le_bytes());
        }
        AudioFrame {
            sequence_no: 0,
            sample_rate: rate,
            channels: 1,
            format: AudioFormat::F32,
            timestamp_ms: 0,
            data,
        }
    }

    #[test]
    fn one_second_at_16k_and_24k_yields_48000_stereo_frames() {
        for rate in [16_000u32, 24_000] {
            let mut sink = BlackHoleSink::detached(48_000, 2);
            sink.write(&f32_tone(rate, rate as usize)).unwrap();
            // The resampler holds back its tail until the stream ends.
            sink.end_stream();
            assert_eq!(sink.buffer().buffered_samples(), 48_000 * 2, "{rate} Hz");
        }
    }

    #[test]
    fn flush_empties_the_queue_and_reports_drained() {
        let mut sink = BlackHoleSink::detached(48_000, 2);
        sink.write(&f32_tone(16_000, 16_000)).unwrap();
        assert!(!sink.is_drained());
        sink.flush().unwrap();
        assert_eq!(sink.buffer().buffered_samples(), 0);
        assert!(sink.is_drained());
    }

    #[test]
    fn unknown_device_name_is_reported() {
        assert!(BlackHoleSink::with_device_name("no such output device \u{1F50A}").is_err());
    }
}
