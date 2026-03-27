use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod buffer;
pub mod cpal_input;
pub mod cpal_output;
pub mod device;
pub mod mock;
pub mod pipeline;

// ---------------------------------------------------------------------------
// Audio format & frame types
// ---------------------------------------------------------------------------

/// Sample format for audio data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    F32,
    I16,
    U8,
}

/// A single audio frame carrying raw sample data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFrame {
    pub sequence_no: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: AudioFormat,
    pub timestamp_ms: u64,
    pub data: Vec<u8>,
}

/// Audio frame captured from an input device, tagged with a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInputFrame {
    pub session_id: Uuid,
    pub frame: AudioFrame,
}

/// Audio frame destined for an output device, tagged with a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioOutputFrame {
    pub session_id: Uuid,
    pub frame: AudioFrame,
}

// ---------------------------------------------------------------------------
// VAD / Wake-word
// ---------------------------------------------------------------------------

/// Result of voice-activity detection over a range of frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadDecision {
    pub session_id: Uuid,
    pub frame_range: std::ops::Range<u64>,
    pub speaking: bool,
    pub confidence: f32,
}

/// Result of a wake-word detection pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeResult {
    pub detector_id: String,
    pub detected: bool,
    pub confidence: f32,
    pub detected_at: u64,
}

// ---------------------------------------------------------------------------
// Audio pipeline state
// ---------------------------------------------------------------------------

/// High-level state of the audio subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioState {
    InputClosed,
    InputOpening,
    InputStreaming,
    OutputClosed,
    OutputOpening,
    OutputPlaying,
    OutputPaused,
    RouteSwitching,
    AudioFailed,
}

impl std::fmt::Display for AudioState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputClosed => write!(f, "InputClosed"),
            Self::InputOpening => write!(f, "InputOpening"),
            Self::InputStreaming => write!(f, "InputStreaming"),
            Self::OutputClosed => write!(f, "OutputClosed"),
            Self::OutputOpening => write!(f, "OutputOpening"),
            Self::OutputPlaying => write!(f, "OutputPlaying"),
            Self::OutputPaused => write!(f, "OutputPaused"),
            Self::RouteSwitching => write!(f, "RouteSwitching"),
            Self::AudioFailed => write!(f, "AudioFailed"),
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors originating from the audio subsystem.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AudioError {
    #[error("input device unavailable")]
    InputDeviceUnavailable,
    #[error("output device unavailable")]
    OutputDeviceUnavailable,
    #[error("format mismatch: expected {expected}, got {got}")]
    FormatMismatch { expected: String, got: String },
    #[error("buffer overflow")]
    BufferOverflow,
    #[error("initialization failed: {0}")]
    InitializationFailed(String),
    #[error("invalid state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },
}

// ---------------------------------------------------------------------------
// Device configuration
// ---------------------------------------------------------------------------

/// Configuration for audio input/output devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub input_device_id: Option<String>,
    pub output_device_id: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
    pub echo_control: bool,
    pub input_gain: f32,
    pub output_gain: f32,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self {
            input_device_id: None,
            output_device_id: None,
            sample_rate: 16_000,
            channels: 1,
            echo_control: true,
            input_gain: 1.0,
            output_gain: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

pub mod prelude {
    pub use crate::buffer::AudioBuffer;
    pub use crate::cpal_input::CpalAudioInput;
    pub use crate::cpal_output::CpalAudioOutput;
    pub use crate::device::{
        AudioDeviceInfo, default_input_device_name, default_output_device_name, list_audio_devices,
    };
    pub use crate::mock::{MockAudioInput, MockAudioOutput};
    pub use crate::pipeline::{AudioInput, AudioOutput, AudioPipeline, VadProcessor};
    pub use crate::{
        AudioError, AudioFormat, AudioFrame, AudioInputFrame, AudioOutputFrame, AudioState,
        DeviceProfile, VadDecision, WakeResult,
    };
}
