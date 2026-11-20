use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod audio_loop;
#[cfg(target_os = "macos")]
pub mod blackhole_sink;
pub mod buffer;
#[cfg(target_os = "macos")]
pub mod core_audio_devices;
#[cfg(target_os = "macos")]
pub mod core_audio_tap;
pub mod cpal_input;
pub mod cpal_output;
pub mod device;
pub mod echo_gate;
pub mod endpoint;
pub mod input_devices;
pub mod input_policy;
pub mod input_supervisor;
pub mod mock;
pub mod output;
pub mod pipeline;
pub mod playback;
pub mod resample;
pub mod routing;
pub mod tap_format;
pub mod tap_ring;
#[cfg(target_os = "macos")]
mod tap_supervisor;
pub mod tap_types;
pub mod tap_watchdog;
pub mod turn_policy;
pub mod vad_window;
#[cfg(feature = "whisper")]
pub mod whisper_asr;

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
    #[error("{0}")]
    Tap(#[from] crate::tap_types::TapError),
    #[error("{0}")]
    InputPolicy(#[from] crate::input_policy::InputPolicyError),
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
    #[cfg(target_os = "macos")]
    pub use crate::blackhole_sink::BlackHoleSink;
    #[cfg(target_os = "macos")]
    pub use crate::core_audio_tap::{CoreAudioTap, CoreAudioTapSource, TapReceiver, frame_peak_amplitude};
    pub use crate::resample::{
        ASR_SAMPLE_RATE, MonoResampler, MonoResampler16k, downmix_to_mono, mono_16k_frame,
    };
    pub use crate::routing::{AppAudioSource, ProcessFilter, VirtualMicSink};
    pub use crate::cpal_input::CpalAudioInput;
    pub use crate::input_policy::{InputDevice, InputSelector, InputTransport, choose_input};
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
