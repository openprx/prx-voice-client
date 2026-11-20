//! Speech synthesis port: one complete clause in, audio chunks out as they are produced.

use prxvc_media::{ClauseId, Lang, PcmChunk, SampleRate};
use tokio_util::sync::CancellationToken;

use crate::descriptor::ProviderDescriptor;

/// A TTS failure.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum TtsError {
    /// The request was cancelled through its token.
    #[error("synthesis cancelled")]
    Cancelled,
    /// The provider did not answer in time.
    #[error("synthesis timed out")]
    Timeout,
    /// The provider cannot be reached or has stopped working.
    #[error("synthesis unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The engine kept producing audio far beyond what the text can need.
    #[error("synthesis ran away after {produced_secs:.1} s of audio")]
    Runaway {
        /// Audio produced before the guard stopped it.
        produced_secs: f32,
    },
    /// The provider refused the request (an HTTP-style status).
    #[error("synthesis rejected with status {status}")]
    Rejected {
        /// The status the provider answered with.
        status: u16,
    },
    /// The voice is not a preset of the provider or was never prepared.
    #[error("unknown voice `{0}`")]
    UnknownVoice(String),
    /// The provider does not synthesise this language.
    #[error("synthesis does not support language `{0}`")]
    UnsupportedLanguage(Lang),
    /// An unexpected provider failure.
    #[error("synthesis internal error: {0}")]
    Internal(String),
}

impl TtsError {
    /// Whether the error is the result of the caller's own cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Which voice speaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceSelection {
    /// A voice that ships with the provider.
    Preset {
        /// The preset's name.
        name: String,
    },
    /// A voice prepared from a reference recording (see [`SpeechSynth::prepare_voice`]).
    Cloned {
        /// The profile the voice was prepared from.
        profile_id: String,
    },
}

/// A reference recording a voice is prepared from.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceReference {
    /// The profile the recording belongs to.
    pub profile_id: String,
    /// Mono samples of the recording.
    pub samples: Vec<f32>,
    /// Their sample rate.
    pub rate: SampleRate,
    /// What is said in the recording, when known.
    pub transcript: Option<String>,
    /// The language of the recording.
    pub lang: Lang,
}

/// The engine-specific result of preparing a voice, to be stored with the profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedVoice {
    /// The handle the engine knows the voice by.
    pub handle: String,
    /// The engine that prepared it.
    pub engine: String,
    /// Hash of the model files it was prepared with; a different hash means
    /// the voice must be prepared again.
    pub model_hash: String,
    /// Named engine-specific data.
    pub blobs: Vec<(String, Vec<u8>)>,
}

/// One synthesis request: one complete translated clause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsRequest {
    /// The clause being spoken.
    pub clause: ClauseId,
    /// The text to speak.
    pub text: String,
    /// The language of `text`.
    pub lang: Lang,
    /// The voice to speak with.
    pub voice: VoiceSelection,
}

/// One step of a synthesis stream.
#[derive(Clone, Debug, PartialEq)]
pub enum TtsItem {
    /// More audio, to play after the previous chunks.
    Chunk(PcmChunk),
    /// Synthesis is complete.
    Done {
        /// Total audio produced for the request.
        audio_secs: f32,
    },
}

/// The stream of one synthesis request; it is that request's own handle.
pub trait TtsStream: Send {
    /// Block until the next item. `None` after [`TtsItem::Done`] or an
    /// error. When `cancel` fires the next result is
    /// `Some(Err(TtsError::Cancelled))`, the engine's work for this request
    /// is stopped and no further audio is produced for it.
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<TtsItem, TtsError>>;

    /// The voice this request is using.
    fn voice_used(&self) -> &VoiceSelection;
}

/// A synthesis engine, shared by both directions.
pub trait SpeechSynth: Send + Sync {
    /// What the provider is and supports.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Prepare a cloned voice from a reference recording.
    fn prepare_voice(&self, reference: &VoiceReference) -> Result<PreparedVoice, TtsError>;

    /// Release a prepared voice.
    fn release_voice(&self, handle: &str) -> Result<(), TtsError>;

    /// Start a request. Concurrent requests share the engine (and may be
    /// batched) but are independent: cancelling one stream leaves every
    /// other stream running.
    fn synthesize(&self, req: TtsRequest) -> Result<Box<dyn TtsStream>, TtsError>;
}
