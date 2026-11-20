//! Machine translation port: a clause in, translated text out as token deltas.

use prxvc_media::{ClauseId, Lang};
use tokio_util::sync::CancellationToken;

use crate::descriptor::ProviderDescriptor;

/// An MT failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MtError {
    /// The request was cancelled through its token.
    #[error("translation cancelled")]
    Cancelled,
    /// The provider did not answer in time.
    #[error("translation timed out")]
    Timeout,
    /// The provider cannot be reached or has stopped working.
    #[error("translation unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The provider refused the request (an HTTP-style status).
    #[error("translation rejected with status {status}")]
    Rejected {
        /// The status the provider answered with.
        status: u16,
    },
    /// An unexpected provider failure.
    #[error("translation internal error: {0}")]
    Internal(String),
}

impl MtError {
    /// Whether the error is the result of the caller's own cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// One translation request: one committed source clause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtRequest {
    /// The clause being translated.
    pub clause: ClauseId,
    /// Language of `text`.
    pub source: Lang,
    /// Language to translate into.
    pub target: Lang,
    /// The clause text.
    pub text: String,
    /// Previous clauses as `(source, target)` pairs, oldest first.
    pub context: Vec<(String, String)>,
    /// Required term translations as `(source, target)` pairs.
    pub glossary: Vec<(String, String)>,
}

/// One step of a translation stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MtDelta {
    /// More translated text, to append.
    Text(String),
    /// The translation is complete; token usage as reported by the provider.
    Done {
        /// Tokens of the prompt.
        prompt_tokens: u32,
        /// Tokens of the translation.
        completion_tokens: u32,
    },
}

/// The stream of one translation request; it is that request's own handle.
pub trait MtStream: Send {
    /// Block until the next delta. `None` after [`MtDelta::Done`] or an
    /// error. When `cancel` fires the next result is
    /// `Some(Err(MtError::Cancelled))` and the provider-side work is abandoned.
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<MtDelta, MtError>>;
}

/// A translation provider.
pub trait Translator: Send + Sync {
    /// What the provider is and supports.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Start a request. Requests are independent: cancelling one stream
    /// leaves every other stream of the provider running.
    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError>;
}
