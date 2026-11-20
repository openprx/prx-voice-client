//! What the adapter tells its owner about one finished request: the typed
//! failure (the port's `MtError` is coarser) and the output-quality flags that
//! the pipeline records as `mt_suspect` (pipeline-design PD-2.6.3, 5.4).

use std::time::Duration;

use prxvc_media::ClauseId;
use prxvc_ports::MtError;

/// Why a request failed, in the adapter's own terms.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MtFailure {
    /// The TCP connection was not established in time.
    #[error("connect timeout")]
    ConnectTimeout,
    /// The connection could not be made (refused, unreachable, TLS).
    #[error("cannot connect: {0}")]
    ConnectFailed(String),
    /// No first token arrived within the first-token timeout.
    #[error("no first token within the timeout")]
    FirstTokenTimeout,
    /// The stream went silent between tokens for longer than the idle timeout.
    #[error("stream stalled: no data within the idle timeout")]
    IdleTimeout,
    /// The server answered with an HTTP error status.
    #[error("HTTP status {0}")]
    HttpStatus(u16),
    /// The server reported an error inside the stream.
    #[error("error event in the stream")]
    ServerEvent(Option<u16>),
    /// The connection ended or broke before the translation was complete.
    #[error("stream ended early: {0}")]
    StreamBroken(String),
    /// The model produced no translation text.
    #[error("empty translation")]
    EmptyOutput,
    /// The request was cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
}

impl MtFailure {
    /// The port-level error for this failure.
    ///
    /// The port has one `Timeout`; it is used for the first-token timeout.
    /// A stall between tokens and a connect timeout are `Unavailable` with a
    /// distinct reason. HTTP 4xx is `Rejected`; 5xx is `Unavailable` (the
    /// server is loading its model, out of slots or broken).
    pub fn to_port_error(&self) -> MtError {
        match self {
            Self::FirstTokenTimeout => MtError::Timeout,
            Self::Cancelled => MtError::Cancelled,
            Self::HttpStatus(status) if (400..500).contains(status) => {
                MtError::Rejected { status: *status }
            }
            Self::ServerEvent(Some(status)) if (400..500).contains(status) => {
                MtError::Rejected { status: *status }
            }
            Self::EmptyOutput => MtError::Internal(self.to_string()),
            other => MtError::Unavailable {
                reason: other.to_string(),
            },
        }
    }
}

/// A way in which the output does not look like a translation. Detected and
/// reported; the text is never altered because of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suspect {
    /// No translation text at all.
    Empty,
    /// The output is mostly in the wrong script for the target language.
    WrongLanguage,
    /// The output is much longer than a translation of the input can be.
    TooLong,
    /// The server stopped at `max_tokens` (`finish_reason: length`).
    Truncated,
    /// The output reads as a refusal or an assistant apology.
    Refusal,
    /// The output reads as an answer or a chat opener, not a translation.
    AnswerLike,
    /// A thinking tag is still in the output.
    ThinkResidue,
    /// The model looped: the output was one short unit repeated many times in a row, or cycled
    /// with a short period. The request was aborted as soon as the guard saw it.
    Repetition,
}

/// A harmless wrapper that was removed from the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stripped {
    /// A leading thinking block.
    Thinking,
    /// A leading `Translation:` style label.
    Preamble,
    /// A pair of quotes around the whole output.
    QuoteWrapper,
}

/// The outcome of one request, delivered to the [`MtObserver`] when its stream
/// ends for any reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtReport {
    /// The clause that was translated.
    pub clause: ClauseId,
    /// The failure, when the request did not complete.
    pub failure: Option<MtFailure>,
    /// Output-quality flags; `mt_suspect` in the trace is `!suspects.is_empty()`.
    pub suspects: Vec<Suspect>,
    /// Wrappers removed from the output.
    pub stripped: Vec<Stripped>,
    /// The cleaned translation as delivered (the harness may record it; the
    /// product never logs it).
    pub text: String,
    /// Time from sending the request to the first content token.
    pub first_token: Option<Duration>,
    /// Time from sending the request to the end of the stream.
    pub total: Duration,
    /// Prompt tokens as reported by the server (0 when not reported).
    pub prompt_tokens: u32,
    /// Completion tokens as reported, or the number of content deltas.
    pub completion_tokens: u32,
    /// Data lines of the stream that were not valid JSON and were dropped.
    pub malformed_lines: u32,
}

impl MtReport {
    /// Whether the trace counts this request as `mt_suspect`.
    pub fn is_suspect(&self) -> bool {
        !self.suspects.is_empty()
    }
}

/// Receives one report per request. Called on the thread that drives the
/// stream, once, when the stream ends; it must not block.
pub trait MtObserver: Send + Sync {
    /// A request ended.
    fn on_report(&self, report: &MtReport);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_errors_follow_the_mapping_in_the_docs() {
        assert_eq!(
            MtFailure::FirstTokenTimeout.to_port_error(),
            MtError::Timeout
        );
        assert_eq!(MtFailure::Cancelled.to_port_error(), MtError::Cancelled);
        assert_eq!(
            MtFailure::HttpStatus(401).to_port_error(),
            MtError::Rejected { status: 401 }
        );
        assert!(matches!(
            MtFailure::HttpStatus(503).to_port_error(),
            MtError::Unavailable { .. }
        ));
        let idle = MtFailure::IdleTimeout.to_port_error();
        let connect = MtFailure::ConnectTimeout.to_port_error();
        assert_ne!(
            idle, connect,
            "stall and connect timeout stay distinguishable"
        );
        assert!(matches!(
            MtFailure::EmptyOutput.to_port_error(),
            MtError::Internal(_)
        ));
    }
}
