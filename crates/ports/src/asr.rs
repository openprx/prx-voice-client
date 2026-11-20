//! Speech recognition ports: streaming recognition with partial and final hypotheses, and the whole-clause refine pass.

use prxvc_media::{Lang, MonoTime, SampleRate};
use tokio_util::sync::CancellationToken;

use crate::descriptor::ProviderDescriptor;

/// An ASR failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AsrError {
    /// The call was cancelled through its token.
    #[error("recognition cancelled")]
    Cancelled,
    /// The provider did not answer in time.
    #[error("recognition timed out")]
    Timeout,
    /// The provider cannot be reached or has stopped working.
    #[error("recognition unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The provider does not recognise this language.
    #[error("recognition does not support language `{0}`")]
    UnsupportedLanguage(Lang),
    /// The audio is not at a rate the provider accepts.
    #[error("recognition does not accept {0}")]
    UnsupportedRate(SampleRate),
    /// An unexpected provider failure.
    #[error("recognition internal error: {0}")]
    Internal(String),
}

impl AsrError {
    /// Whether the error is the result of the caller's own cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Why a final hypothesis closed its segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SegmentEnd {
    /// The caller ended it: the result of [`AsrStream::finalize`] or of a refine call.
    Requested,
    /// The recogniser ended the segment itself because it reached its maximum length. Only
    /// [`AsrStream::feed`] returns such a hypothesis. It is final for the audio before the cut;
    /// the audio after the cut is already the start of the next segment of the same stream
    /// (revisions restart at 1), so nothing is lost or repeated, and the caller does not call
    /// [`AsrStream::reset`] for it.
    MaxLength,
}

/// The recogniser's current reading of a segment.
#[derive(Clone, Debug, PartialEq)]
pub struct AsrHypothesis {
    /// The whole text so far.
    pub text: String,
    /// Length in bytes (on a char boundary) of the prefix of `text` that the
    /// recogniser will not revise any more.
    pub stable_prefix_len: usize,
    /// Increases by one per hypothesis of a segment; the final repeats or
    /// exceeds the last partial's revision.
    pub revision: u32,
    /// `true` for the result of [`AsrStream::finalize`], and for the hypothesis with which the
    /// recogniser ends a segment itself (see `ended_by`).
    pub is_final: bool,
    /// Why the segment ended; `None` on a partial hypothesis, `Some` on every final one.
    pub ended_by: Option<SegmentEnd>,
    /// The end of the audio this hypothesis covers.
    pub audio_end: MonoTime,
}

/// Context given when a stream is opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsrContext {
    /// Text of the previous clause, for models that use it.
    pub previous_text: Option<String>,
    /// Words the recogniser should favour.
    pub hotwords: Vec<String>,
}

/// One open recognition stream: audio goes in, hypotheses come out.
pub trait AsrStream: Send {
    /// Feed audio; compute happens on the caller's thread. Returns a partial
    /// hypothesis when the text changed, or, when the recogniser ends the
    /// segment itself at its maximum length, the final hypothesis of that
    /// segment (`is_final`, `ended_by == Some(SegmentEnd::MaxLength)`): the
    /// rest of the audio then belongs to the next segment of the stream.
    fn feed(&mut self, audio: &[f32], ts: MonoTime) -> Result<Option<AsrHypothesis>, AsrError>;

    /// End of a clause or segment: the final hypothesis. Returns
    /// [`AsrError::Cancelled`] when `cancel` fires first.
    fn finalize(&mut self, cancel: &CancellationToken) -> Result<AsrHypothesis, AsrError>;

    /// Start the next segment on the same stream.
    fn reset(&mut self);
}

/// A streaming recogniser that opens streams.
pub trait StreamingAsr: Send + Sync {
    /// What the provider is and supports.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Open a stream. Streams are independent: finalising or cancelling one
    /// does not affect another opened from the same provider.
    fn open(&self, lang: Lang, ctx: AsrContext) -> Result<Box<dyn AsrStream>, AsrError>;
}

/// The refine pass of two-pass recognition: a whole clause in, one result out.
pub trait RefineAsr: Send + Sync {
    /// What the provider is and supports (normally [`crate::StreamingKind::Segmented`]).
    fn descriptor(&self) -> ProviderDescriptor;

    /// Transcribe one clause of audio. Returns [`AsrError::Cancelled`] when
    /// `cancel` fires first; other calls are not affected.
    fn transcribe(
        &self,
        audio: &[f32],
        rate: SampleRate,
        lang: Lang,
        ctx: &AsrContext,
        cancel: &CancellationToken,
    ) -> Result<AsrHypothesis, AsrError>;
}
