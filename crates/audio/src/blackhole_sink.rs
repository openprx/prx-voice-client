//! macOS virtual-mic injection via BlackHole.
//!
//! [`BlackHoleSink`] is a [`VirtualMicSink`] that plays synthesized audio into a
//! **BlackHole** output device. The meeting app is configured to use BlackHole
//! as its *microphone*, so whatever we write here is what the other party hears
//! — i.e. the cloned-voice English for direction A.
//!
//! The user must install BlackHole and select it as the meeting app's mic. This
//! sink only locates the device by name and streams f32 samples into it.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::routing::VirtualMicSink;
use crate::{AudioError, AudioFormat, AudioFrame, AudioState};

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
    sample_rate: u32,
    channels: u16,
    buffer: Arc<parking_lot::Mutex<VecDeque<f32>>>,
    is_running: Arc<AtomicBool>,
}

impl BlackHoleSink {
    /// Locate a BlackHole output device by the default name substring.
    pub fn new() -> Result<Self, AudioError> {
        Self::with_device_name(DEFAULT_BLACKHOLE_NAME)
    }

    /// Locate an output device whose name contains `name_substr` (case-sensitive).
    ///
    /// # Errors
    /// [`AudioError::OutputDeviceUnavailable`] if no matching device is found.
    pub fn with_device_name(name_substr: &str) -> Result<Self, AudioError> {
        let device = Self::find_device(name_substr)?;
        let config = device
            .default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("blackhole config: {e}")))?;
        Ok(Self {
            device_name: name_substr.to_owned(),
            state: AudioState::OutputClosed,
            stream: None,
            sample_rate: config.sample_rate().0,
            channels: config.channels(),
            buffer: Arc::new(parking_lot::Mutex::new(VecDeque::new())),
            is_running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Sample rate negotiated with the BlackHole device (typically 48 kHz).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count of the BlackHole device (typically 2).
    pub fn channels(&self) -> u16 {
        self.channels
    }

    fn find_device(name_substr: &str) -> Result<cpal::Device, AudioError> {
        let host = cpal::default_host();
        let devices = host
            .output_devices()
            .map_err(|e| AudioError::InitializationFailed(format!("enumerate outputs: {e}")))?;
        for device in devices {
            if let Ok(name) = device.name() {
                if name.contains(name_substr) {
                    return Ok(device);
                }
            }
        }
        Err(AudioError::OutputDeviceUnavailable)
    }

    /// Decode a frame's samples to f32 and up-/down-mix to the device channel
    /// count, returning interleaved samples ready for the device buffer.
    fn frame_to_device_samples(&self, frame: &AudioFrame) -> Vec<f32> {
        let src_channels = frame.channels.max(1) as usize;
        let mono_or_interleaved: Vec<f32> = match frame.format {
            AudioFormat::F32 => frame
                .data
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
            AudioFormat::I16 => frame
                .data
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / i16::MAX as f32)
                .collect(),
            AudioFormat::U8 => frame.data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
        };

        // Collapse to mono, then fan out to the device's channel count.
        let dst_channels = self.channels.max(1) as usize;
        let frames = mono_or_interleaved.len() / src_channels;
        let mut out = Vec::with_capacity(frames * dst_channels);
        for f in 0..frames {
            let mut acc = 0.0f32;
            for c in 0..src_channels {
                acc += mono_or_interleaved[f * src_channels + c];
            }
            let mono = acc / src_channels as f32;
            for _ in 0..dst_channels {
                out.push(mono);
            }
        }
        out
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
        let device = Self::find_device(&self.device_name)?;
        let config = cpal::StreamConfig {
            channels: self.channels,
            sample_rate: cpal::SampleRate(self.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::clone(&self.buffer);
        let is_running = Arc::clone(&self.is_running);
        is_running.store(true, Ordering::Release);

        let data_callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if !is_running.load(Ordering::Acquire) {
                for s in data.iter_mut() {
                    *s = 0.0;
                }
                return;
            }
            let mut buf = buffer.lock();
            for s in data.iter_mut() {
                *s = buf.pop_front().unwrap_or(0.0);
            }
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
        let samples = self.frame_to_device_samples(frame);
        self.buffer.lock().extend(samples);
        Ok(())
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
