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
    /// The provider did not answer in time (the phase is not known).
    #[error("translation timed out")]
    Timeout,
    /// The provider did not answer in time in a known phase of the request.
    #[error("translation timed out ({phase})")]
    TimedOut {
        /// Where the request was waiting.
        phase: MtPhase,
    },
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

    /// Whether the provider ran out of time, with or without a known phase.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout | Self::TimedOut { .. })
    }
}

/// The phase of a translation request in which a timeout happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum MtPhase {
    /// Connecting to the provider or sending the request.
    #[error("connect")]
    Connect,
    /// Waiting for the first translated token.
    #[error("first token")]
    FirstToken,
    /// The stream stalled after it had started.
    #[error("stream stalled")]
    Stream,
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
    /// A retry after a reply in the wrong language: the request carries a stricter instruction
    /// that the reply must be written in `target` and nothing else.
    pub strict_language: bool,
}

/// A way in which a finished translation does not look like a translation, reported next to the
/// text and never acted on by the adapter. The pipeline counts and shows each kind. Empty output
/// and a looping model are failures, not flags (see [`MtStream::suspect`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MtFlag {
    /// The output is mostly in the wrong script for the target language.
    WrongLanguage,
    /// The output is much longer than a translation of the input can be.
    TooLong,
    /// The provider stopped at its token limit.
    Truncated,
    /// The output reads as a refusal or an assistant apology.
    Refusal,
    /// The output reads as an answer or a chat opener, not a translation.
    AnswerLike,
    /// A thinking tag is still in the output.
    ThinkResidue,
}

impl MtFlag {
    /// Every kind, in a fixed order.
    pub const ALL: [Self; 6] = [
        Self::WrongLanguage,
        Self::TooLong,
        Self::Truncated,
        Self::Refusal,
        Self::AnswerLike,
        Self::ThinkResidue,
    ];

    /// A stable name for logs and counters.
    pub const fn name(self) -> &'static str {
        match self {
            Self::WrongLanguage => "wrong_language",
            Self::TooLong => "too_long",
            Self::Truncated => "truncated",
            Self::Refusal => "refusal",
            Self::AnswerLike => "answer_like",
            Self::ThinkResidue => "think_residue",
        }
    }
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

    /// Why the output so far does not look like a translation (empty, wrong
    /// language, far too long, a refusal or an answer), or `None`.
    ///
    /// The pipeline asks after every delta and once more after `Done`. Once an
    /// adapter has flagged a request it keeps returning the reason. A flagged
    /// request is never spoken: the stream still ends normally, and the
    /// pipeline applies the direction's failure policy. The default is "never
    /// flagged", so adapters that cannot assess their output compile unchanged.
    fn suspect(&self) -> Option<String> {
        None
    }

    /// The quality flags of the finished translation, empty until [`MtDelta::Done`] was
    /// delivered. Unlike [`MtStream::suspect`] these do not stop the clause: its text may already
    /// be speaking when they are known. The pipeline counts them and shows them in the captions.
    /// The default is "no flags".
    fn flags(&self) -> Vec<MtFlag> {
        Vec::new()
    }
}

/// A translation provider.
pub trait Translator: Send + Sync {
    /// What the provider is and supports.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Start a request. Requests are independent: cancelling one stream
    /// leaves every other stream of the provider running.
    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError>;
}
