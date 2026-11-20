use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use cpal::traits::{DeviceTrait, StreamTrait};
use uuid::Uuid;

use crate::input_devices::find_cpal_input;
use crate::input_policy::InputDevice;
use crate::pipeline::AudioInput;
use crate::{AudioError, AudioFormat, AudioFrame, AudioInputFrame, AudioState};

/// Wrapper to make `cpal::Stream` usable in `Send` contexts.
///
/// `cpal::Stream` on macOS (CoreAudio) is not `Send` due to internal raw pointers.
/// However, we only ever access the stream from the thread that created it, so
/// this is safe in practice. The stream's audio callbacks run on their own
/// dedicated audio thread managed by cpal.
/// The inner field is read by `Drop` — dropping the stream stops the audio device.
#[allow(dead_code)]
struct SendStream(cpal::Stream);

// SAFETY: The cpal::Stream is created and dropped on the same thread.
// Audio callbacks run on a separate cpal-managed thread and communicate
// via Arc<AtomicBool> / Arc<Mutex<Vec<_>>>, not through the Stream itself.
unsafe impl Send for SendStream {}

/// Real audio input using cpal for capturing from a hardware device.
pub struct CpalAudioInput {
    state: AudioState,
    session_id: Uuid,
    device: InputDevice,
    stream: Option<SendStream>,
    sample_rate: u32,
    channels: u16,
    buffer: Arc<parking_lot::Mutex<Vec<AudioFrame>>>,
    sequence: Arc<AtomicU64>,
    is_running: Arc<AtomicBool>,
}

impl CpalAudioInput {
    /// Create a cpal-backed audio input on an explicitly chosen device.
    ///
    /// The device comes from the input-device policy
    /// ([`crate::input_policy::choose_input`]); this type never opens "the
    /// system default input".
    ///
    /// # Errors
    /// [`AudioError::InputDeviceUnavailable`] if the device is no longer present.
    pub fn open(session_id: Uuid, device: &InputDevice) -> Result<Self, AudioError> {
        let cpal_device = find_cpal_input(device)?;
        let config = cpal_device
            .default_input_config()
            .map_err(|e| AudioError::InitializationFailed(format!("input config: {}", e)))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        Ok(Self {
            state: AudioState::InputClosed,
            session_id,
            device: device.clone(),
            stream: None,
            sample_rate,
            channels,
            buffer: Arc::new(parking_lot::Mutex::new(Vec::new())),
            sequence: Arc::new(AtomicU64::new(0)),
            is_running: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl AudioInput for CpalAudioInput {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.state != AudioState::InputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::InputStreaming.to_string(),
            });
        }

        let device = find_cpal_input(&self.device)?;

        let config = cpal::StreamConfig {
            channels: self.channels,
            sample_rate: cpal::SampleRate(self.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::clone(&self.buffer);
        let sequence = Arc::clone(&self.sequence);
        let is_running = Arc::clone(&self.is_running);
        let sample_rate = self.sample_rate;
        let channels = self.channels;

        is_running.store(true, Ordering::Release);

        let data_callback = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            if !is_running.load(Ordering::Acquire) {
                return;
            }

            // Convert f32 samples to bytes (little-endian)
            let mut bytes = Vec::with_capacity(data.len() * 4);
            for &sample in data {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }

            let seq = sequence.fetch_add(1, Ordering::Relaxed);
            let frame = AudioFrame {
                sequence_no: seq,
                sample_rate,
                channels,
                format: AudioFormat::F32,
                timestamp_ms: seq * 20, // approximate 20ms per frame
                data: bytes,
            };

            let mut buf = buffer.lock();
            // Cap buffer size to prevent unbounded growth
            if buf.len() < 500 {
                buf.push(frame);
            }
        };

        let err_callback = |err: cpal::StreamError| {
            tracing::error!("cpal input stream error: {}", err);
        };

        let stream = device
            .build_input_stream(&config, data_callback, err_callback, None)
            .map_err(|e| AudioError::InitializationFailed(format!("build input stream: {}", e)))?;

        stream
            .play()
            .map_err(|e| AudioError::InitializationFailed(format!("play input stream: {}", e)))?;

        self.stream = Some(SendStream(stream));
        self.state = AudioState::InputStreaming;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.is_running.store(false, Ordering::Release);
        // Dropping the stream stops it
        self.stream = None;
        self.state = AudioState::InputClosed;
        self.sequence.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn read_frame(&mut self) -> Result<Option<AudioInputFrame>, AudioError> {
        if self.state != AudioState::InputStreaming {
            return Ok(None);
        }

        let mut buf = self.buffer.lock();
        if buf.is_empty() {
            return Ok(None);
        }

        let frame = buf.remove(0);
        Ok(Some(AudioInputFrame {
            session_id: self.session_id,
            frame,
        }))
    }

    fn state(&self) -> AudioState {
        self.state
    }
}

impl Drop for CpalAudioInput {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Release);
        // stream is dropped automatically, which stops it
    }
}
