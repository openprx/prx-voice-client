use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::pipeline::AudioOutput;
use crate::{AudioError, AudioFormat, AudioOutputFrame, AudioState};

/// Wrapper to make `cpal::Stream` usable in `Send` contexts.
///
/// See `cpal_input::SendStream` for the safety rationale.
/// The inner field is read by `Drop` — dropping the stream stops the audio device.
#[allow(dead_code)]
struct SendStream(cpal::Stream);

// SAFETY: The cpal::Stream is created and dropped on the same thread.
// Audio callbacks run on a separate cpal-managed thread and communicate
// via Arc<AtomicBool> / Arc<Mutex<VecDeque<_>>>, not through the Stream itself.
unsafe impl Send for SendStream {}

/// Real audio output using cpal for playback through a hardware device.
pub struct CpalAudioOutput {
    state: AudioState,
    stream: Option<SendStream>,
    sample_rate: u32,
    channels: u16,
    buffer: Arc<parking_lot::Mutex<std::collections::VecDeque<f32>>>,
    is_running: Arc<AtomicBool>,
}

impl CpalAudioOutput {
    /// Create a new cpal-backed audio output.
    ///
    /// Returns an error if no default output device is available.
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::OutputDeviceUnavailable)?;
        let config = device
            .default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("output config: {}", e)))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        Ok(Self {
            state: AudioState::OutputClosed,
            stream: None,
            sample_rate,
            channels,
            buffer: Arc::new(parking_lot::Mutex::new(std::collections::VecDeque::new())),
            is_running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The sample rate negotiated with the output device.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The channel count negotiated with the output device.
    pub fn channels(&self) -> u16 {
        self.channels
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

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::OutputDeviceUnavailable)?;

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
                // Fill with silence when not running
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
                return;
            }

            let mut buf = buffer.lock();
            for sample in data.iter_mut() {
                *sample = buf.pop_front().unwrap_or(0.0);
            }
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

        let samples = match frame.frame.format {
            AudioFormat::F32 => {
                // Interpret data as little-endian f32
                frame
                    .frame
                    .data
                    .chunks_exact(4)
                    .map(|chunk| {
                        let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                        f32::from_le_bytes(bytes)
                    })
                    .collect::<Vec<f32>>()
            }
            AudioFormat::I16 => {
                // Convert i16 samples to f32 in [-1.0, 1.0]
                frame
                    .frame
                    .data
                    .chunks_exact(2)
                    .map(|chunk| {
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                        sample as f32 / i16::MAX as f32
                    })
                    .collect::<Vec<f32>>()
            }
            AudioFormat::U8 => {
                // Convert u8 samples to f32 in [-1.0, 1.0]
                frame
                    .frame
                    .data
                    .iter()
                    .map(|&b| (b as f32 - 128.0) / 128.0)
                    .collect::<Vec<f32>>()
            }
        };

        let mut buf = self.buffer.lock();
        buf.extend(samples);
        Ok(())
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
