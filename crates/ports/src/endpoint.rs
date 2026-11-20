//! Audio endpoint ports: a source that delivers captured frames and a sink that accepts frames to play.

use std::time::Duration;

use prxvc_media::{AudioFrame, SampleRate};

/// An endpoint failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EndpointError {
    /// The device or process cannot be opened.
    #[error("endpoint unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The device went away while running.
    #[error("endpoint lost its device `{device}`")]
    DeviceLost {
        /// The device that disappeared.
        device: String,
    },
    /// The endpoint was used before `start` or after `stop`.
    #[error("endpoint is not started")]
    NotStarted,
    /// An unexpected failure.
    #[error("endpoint internal error: {0}")]
    Internal(String),
}

/// What a started source delivers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    /// Rate of the delivered frames.
    pub rate: SampleRate,
    /// The device or process the audio comes from, for display.
    pub device: String,
}

/// What a started sink expects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkInfo {
    /// Rate of the frames it accepts.
    pub rate: SampleRate,
    /// The device audio goes to, for display.
    pub device: String,
}

/// Counters and state of an endpoint, safe to read from any thread's snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointHealth {
    /// Between a successful `start` and `stop`.
    pub running: bool,
    /// Frames delivered or accepted so far.
    pub frames: u64,
    /// Samples lost to overrun (source) or a full queue (sink).
    pub dropped_samples: u64,
    /// Times the sink ran dry and played silence.
    pub underruns: u64,
    /// The last failure, if any.
    pub last_error: Option<String>,
}

/// The result of one [`AudioSink::write`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    /// Samples queued for playing.
    pub accepted: usize,
    /// Samples refused because the queue was full.
    pub dropped: usize,
}

/// A device or process that delivers audio. The real-time side lives inside
/// the adapter; these methods run on ordinary threads.
pub trait AudioSource: Send {
    /// Open the source and begin capturing.
    fn start(&mut self) -> Result<SourceInfo, EndpointError>;

    /// Append the frames captured so far to `out`, waiting up to `max_wait`
    /// for the first one. An empty result after the wait is not an error.
    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError>;

    /// Counters and state.
    fn health(&self) -> EndpointHealth;

    /// Stop capturing and release the device.
    fn stop(&mut self);
}

/// A device that plays audio. The real-time side lives inside the adapter.
pub trait AudioSink: Send {
    /// Open the sink.
    fn start(&mut self) -> Result<SinkInfo, EndpointError>;

    /// Queue a frame for playing.
    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome;

    /// Discard everything queued and not yet played.
    fn flush(&mut self);

    /// Audio queued and not yet played.
    fn queued(&self) -> Duration;

    /// Counters and state.
    fn health(&self) -> EndpointHealth;

    /// Stop playing and release the device.
    fn stop(&mut self);
}
