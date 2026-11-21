//! Dialogue port: a conversational backend that answers a spoken assistant's turns with streamed text.
//!
//! The backend owns the conversation. The caller names it with a [`ConversationId`] and sends only
//! the new messages of the turn; what came before (history, system prompt, tools, memory) is the
//! backend's business. That is what lets a stateless chat-completions endpoint (which keeps a
//! bounded history on the client side) and a stateful agent sit behind the same port.

use std::fmt;

use prxvc_media::Lang;
use tokio_util::sync::CancellationToken;

use crate::descriptor::ProviderDescriptor;

/// Names one conversation (one assistant session) towards the backend.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConversationId(String);

impl ConversationId {
    /// An id from its text.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConversationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A dialogue failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DialogueError {
    /// The request was cancelled through its token.
    #[error("dialogue cancelled")]
    Cancelled,
    /// The backend did not answer in time (the phase is not known).
    #[error("dialogue timed out")]
    Timeout,
    /// The backend cannot be reached or has stopped working: a server that does not answer, a
    /// connection that broke, a server that is still loading its model.
    #[error("dialogue backend unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The backend refused the request (an HTTP-style status).
    #[error("dialogue backend rejected the request with status {status}")]
    Rejected {
        /// The status the backend answered with.
        status: u16,
    },
    /// The backend answered with no text at all.
    #[error("dialogue backend produced no text")]
    EmptyReply,
    /// The backend is known to the application but cannot serve yet (its interface does not exist).
    #[error("dialogue backend `{backend}` is not available yet")]
    NotAvailableYet {
        /// The backend's name.
        backend: String,
    },
    /// An unexpected backend failure.
    #[error("dialogue internal error: {0}")]
    Internal(String),
}

impl DialogueError {
    /// Whether the error is the result of the caller's own cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Who wrote a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogueRole {
    /// The person talking to the assistant.
    User,
    /// The assistant.
    Assistant,
}

/// One message of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueMessage {
    /// Who wrote it.
    pub role: DialogueRole,
    /// What it says.
    pub text: String,
}

impl DialogueMessage {
    /// A message of the user.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: DialogueRole::User,
            text: text.into(),
        }
    }
}

/// One request: the new messages of a turn in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueRequest {
    /// The conversation the messages belong to.
    pub conversation: ConversationId,
    /// The new messages of this turn, oldest first (normally the user's recognised speech).
    pub messages: Vec<DialogueMessage>,
    /// The language the user spoke; the reply is in it.
    pub lang: Lang,
}

/// One step of a dialogue stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogueDelta {
    /// More reply text, to append.
    Text(String),
    /// The reply is complete; token usage as reported by the backend (zero when it reports none).
    Done {
        /// Tokens of the prompt.
        prompt_tokens: u32,
        /// Tokens of the reply.
        completion_tokens: u32,
    },
}

/// The stream of one dialogue request; it is that request's own handle.
pub trait DialogueStream: Send {
    /// Block until the next delta. `None` after [`DialogueDelta::Done`] or an error. When `cancel`
    /// fires the next result is `Some(Err(DialogueError::Cancelled))` and the backend-side work is
    /// abandoned. Dropping the stream abandons the request as well.
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<DialogueDelta, DialogueError>>;
}

/// A conversational backend.
pub trait DialogueBackend: Send + Sync {
    /// What the backend is and supports.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Send the new messages of a turn. Requests are independent: cancelling one stream leaves
    /// every other stream of the backend running. The text the stream delivers is kept in the
    /// conversation when the stream completes; a stream that is cancelled or dropped keeps what it
    /// had delivered by then (the reply the user heard part of).
    fn respond(&self, req: DialogueRequest) -> Result<Box<dyn DialogueStream>, DialogueError>;

    /// The conversation is over: the backend may forget it.
    fn end_conversation(&self, conversation: &ConversationId);
}
