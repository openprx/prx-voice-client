//! Audio endpoints of the desktop client: capture and playback devices behind the `AudioSource` and `AudioSink` ports, plus the local speech helpers around them.
//!
//! # Endpoints
//!
//! * Capture: [`capture_source::CaptureSource`] implements
//!   [`prxvc_ports::AudioSource`]. [`cpal_input::open`] builds one on a
//!   microphone; [`core_audio_tap::CoreAudioTapSource`] (macOS) captures one
//!   application's output.
//! * Playback: [`playback_sink::PlaybackSink`] implements
//!   [`prxvc_ports::AudioSink`] and [`stream_sink::StreamSink`] (end of stream
//!   and the drain signal). [`cpal_output::open_default`] and
//!   [`cpal_output::open_named`] build one on an output device;
//!   [`virtual_mic::VirtualMicSink`] opens the virtual microphone driver's
//!   hidden sink by UID, checked by [`driver_status::DriverStatus`].
//!
//! * Mixing: [`mix::Mixer`] combines the translated speech and the original
//!   voice of one direction into the stream a sink plays.
//!
//! Frames are the `prxvc-media` types: mono `f32`, a sample rate, a monotonic
//! timestamp and a sequence number.
//!
//! # Real-time convention
//!
//! Code that runs on a device callback (an audio thread) must not allocate,
//! lock, log, block or send on a channel (code standard CS-4.1.1). Such code
//! is kept apart from the rest so that this can be checked by a text search:
//!
//! * Every function or method that a device callback runs lives in a file
//!   named `*_rt.rs` (today `cpal_input_rt.rs`, `cpal_output_rt.rs`,
//!   `core_audio_output_rt.rs` and `playback_buffer_rt.rs`, and for the process
//!   tap `tap_rt.rs`, `tap_format_rt.rs` and `tap_ring_rt.rs`).
//! * The closure handed to the device (`build_input_stream`,
//!   `build_output_stream`) does nothing but call a function of an `_rt`
//!   module, so no real-time logic hides in a non-`_rt` file.
//! * Constructors and anything that allocates (the rings, the buffers) live in
//!   ordinary files; an `_rt` file only moves values it was given.
//! * The stream error callbacks are not audio-thread callbacks (cpal reports
//!   stream errors from its own thread); they stay in the ordinary files and
//!   may log.
//!
//! The test `tests/realtime_files.rs` fails when an `_rt` file (outside its
//! `#[cfg(test)]` section) mentions `Mutex`, `RwLock`, `lock(`, `Vec::new`,
//! `vec!`, `Box::new`, `format!`, `tracing::`, a channel send, or another
//! allocation (`Vec::with_capacity`, `.to_vec(`, `.to_string(`, `.collect(`),
//! or when a registered device callback calls anything but an `_rt` function.
//! The Core Audio process-tap callback is covered too: `tap_rt.rs` holds the IO
//! callback, `tap_format_rt.rs` its per-cycle sample handling and
//! `tap_ring_rt.rs` the producer side of the ring; `core_audio_tap.rs` only
//! registers `on_buffers` from `tap_rt`, which the test checks.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod audio_loop;
pub mod buffer;
pub mod call_activity;
pub mod capture_reader;
pub mod capture_source;
#[cfg(target_os = "macos")]
pub mod core_audio_devices;
#[cfg(target_os = "macos")]
pub(crate) mod core_audio_output;
#[cfg(target_os = "macos")]
pub(crate) mod core_audio_output_rt;
#[cfg(target_os = "macos")]
pub mod core_audio_tap;
pub mod cpal_input;
pub mod cpal_input_rt;
pub mod cpal_output;
pub(crate) mod cpal_output_rt;
pub mod cpal_stream;
pub mod device;
pub mod device_directory;
pub mod driver_plist;
pub mod driver_status;
pub mod echo_gate;
pub mod endpoint;
pub mod input_devices;
pub mod input_policy;
pub mod input_supervisor;
pub mod mix;
pub mod mock;
pub mod output;
pub mod pipeline;
pub mod playback;
pub mod playback_buffer;
mod playback_buffer_rt;
pub mod playback_sink;
pub mod resample;
pub mod routing;
pub mod stream_sink;
pub mod tap_format;
pub(crate) mod tap_format_rt;
pub mod tap_host_time;
pub mod tap_ring;
pub(crate) mod tap_ring_rt;
#[cfg(target_os = "macos")]
pub(crate) mod tap_rt;
#[cfg(target_os = "macos")]
pub mod tap_consent;
#[cfg(target_os = "macos")]
mod tap_supervisor;
pub mod tap_types;
pub mod tap_watchdog;
pub mod tap_zero_watchdog;
pub mod turn_policy;
#[cfg(target_os = "macos")]
pub mod virtual_mic;
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
    #[error("{0}")]
    Endpoint(#[from] prxvc_ports::EndpointError),
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
    pub use crate::core_audio_tap::{CoreAudioTap, CoreAudioTapSource, TapReceiver, frame_peak_amplitude};
    pub use crate::resample::{
        ASR_SAMPLE_RATE, MonoResampler16k, downmix_to_mono, mono_16k_frame,
    };
    pub use crate::routing::ProcessFilter;
    pub use crate::capture_source::CaptureSource;
    pub use crate::input_policy::{InputDevice, InputSelector, InputTransport, choose_input};
    pub use crate::playback_sink::PlaybackSink;
    pub use crate::stream_sink::StreamSink;
    pub use crate::device::{
        AudioDeviceInfo, default_input_device_name, default_output_device_name, list_audio_devices,
    };
    pub use crate::mock::MockAudioSource;
    pub use crate::pipeline::{AudioPipeline, VadProcessor};
    pub use crate::{
        AudioError, AudioFormat, AudioFrame, AudioState, DeviceProfile, VadDecision, WakeResult,
    };
}
