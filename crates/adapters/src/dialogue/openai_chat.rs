//! `OpenAiChatDialogue`: the assistant's conversational backend over an OpenAI-compatible
//! `/v1/chat/completions` endpoint, for example `llama-server`.
//!
//! The endpoint is stateless, so the adapter keeps each conversation's history (`History`, bounded
//! by turns and tokens) and sends it with every request, behind the system prompt of a spoken
//! assistant. A turn is stored when its reply completes; a reply that is cancelled or dropped
//! keeps the part that was delivered, so the model knows what the user heard. The HTTP and SSE
//! client is the translator's (`chat_http`); cancelling a stream aborts the request task, which
//! drops the connection.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_media::Lang;
use prxvc_ports::{
    CancellationToken, ConversationId, DialogueBackend, DialogueDelta, DialogueError,
    DialogueMessage, DialogueRequest, DialogueRole, DialogueStream, Egress, ProviderDescriptor,
    StreamingKind,
};

use super::config::DialogueConfig;
use super::history::History;
use super::prompt::request_body;
use crate::chat_http::{ChatClient, ChatRequest, Chunk, HttpFailure, Received, is_loopback};
use crate::mt::assess::estimate_tokens;
use crate::mt::config::ConfigError;
use crate::mt::output::OutputFilter;

struct Inner {
    cfg: DialogueConfig,
    client: ChatClient,
    descriptor: ProviderDescriptor,
    conversations: Mutex<HashMap<ConversationId, History>>,
}

impl Inner {
    /// Store a finished or interrupted turn: the new messages and the reply that was delivered.
    fn commit(&self, conversation: &ConversationId, new: &[DialogueMessage], reply: &str) {
        let reply = reply.trim();
        if reply.is_empty() {
            return;
        }
        let mut conversations = self.conversations.lock();
        let history = conversations.entry(conversation.clone()).or_default();
        history.push(new.iter().cloned());
        history.push([DialogueMessage {
            role: DialogueRole::Assistant,
            text: reply.to_owned(),
        }]);
        history.trim(self.cfg.history, 0, 0);
    }
}

/// The dialogue backend for OpenAI-compatible chat endpoints.
#[derive(Clone)]
pub struct OpenAiChatDialogue {
    inner: Arc<Inner>,
}

impl fmt::Debug for OpenAiChatDialogue {
    /// Shows the endpoint and model; never the API key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatDialogue")
            .field("provider", &self.inner.cfg.provider_id)
            .field("base_url", &self.inner.cfg.base_url)
            .field("model", &self.inner.cfg.model)
            .field("has_api_key", &self.inner.cfg.api_key.is_some())
            .finish()
    }
}

impl OpenAiChatDialogue {
    /// A backend for `cfg`.
    pub fn new(cfg: DialogueConfig) -> Result<Self, ConfigError> {
        cfg.validate()?;
        let url = crate::chat_http::endpoint_url(&cfg.base_url)?;
        let host = url.host_str().unwrap_or_default().to_owned();
        let egress = if is_loopback(&host) {
            Egress::Local
        } else {
            Egress::Text { host }
        };
        let client = ChatClient::new(
            url,
            cfg.api_key.as_ref().map(|k| k.expose().to_owned()),
            cfg.timeouts,
            "dialogue-http",
        )?;
        let descriptor =
            ProviderDescriptor::new(&cfg.provider_id, &cfg.model, StreamingKind::Native)
                .with_languages(cfg.languages.iter().copied())
                .with_egress(egress);
        Ok(Self {
            inner: Arc::new(Inner {
                cfg,
                client,
                descriptor,
                conversations: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// How many user turns the history of `conversation` holds (tests and diagnostics).
    pub fn history_turns(&self, conversation: &ConversationId) -> usize {
        self.inner
            .conversations
            .lock()
            .get(conversation)
            .map_or(0, History::turns)
    }
}

/// The port-level error for a transport failure: a first-token timeout is `Timeout`, a 4xx is
/// `Rejected`, everything else is `Unavailable` with the reason (a stalled stream, a refused
/// connection, a server that is loading its model).
pub(crate) fn port_error(failure: &HttpFailure) -> DialogueError {
    match failure {
        HttpFailure::FirstTokenTimeout => DialogueError::Timeout,
        HttpFailure::Cancelled => DialogueError::Cancelled,
        HttpFailure::HttpStatus(status) | HttpFailure::ServerEvent(Some(status))
            if (400..500).contains(status) =>
        {
            DialogueError::Rejected { status: *status }
        }
        other => DialogueError::Unavailable {
            reason: other.to_string(),
        },
    }
}

impl DialogueBackend for OpenAiChatDialogue {
    fn descriptor(&self) -> ProviderDescriptor {
        self.inner.descriptor.clone()
    }

    fn respond(&self, req: DialogueRequest) -> Result<Box<dyn DialogueStream>, DialogueError> {
        let inner = &self.inner;
        inner
            .descriptor
            .require_lang(req.lang)
            .map_err(|e| DialogueError::Unavailable {
                reason: e.to_string(),
            })?;
        let new_user: Vec<&DialogueMessage> = req
            .messages
            .iter()
            .filter(|m| m.role == DialogueRole::User)
            .collect();
        if new_user.iter().all(|m| m.text.trim().is_empty()) {
            return Err(DialogueError::Internal("the user message is empty".into()));
        }
        let pending_tokens: u32 = req.messages.iter().map(|m| estimate_tokens(&m.text)).sum();
        let body = {
            let mut conversations = inner.conversations.lock();
            let history = conversations.entry(req.conversation.clone()).or_default();
            history.trim(inner.cfg.history, new_user.len(), pending_tokens);
            request_body(&inner.cfg, req.lang, history.messages(), &req.messages)
        };
        let request = inner.client.start(body);
        tracing::debug!(conversation = %req.conversation, model = %inner.cfg.model, "dialogue request sent");
        Ok(Box::new(ChatReply {
            inner: Arc::clone(&self.inner),
            request,
            filter: OutputFilter::thinking_only(),
            conversation: req.conversation,
            new: req.messages,
            reply: String::new(),
            state: State::Streaming,
        }))
    }

    fn end_conversation(&self, conversation: &ConversationId) {
        self.inner.conversations.lock().remove(conversation);
    }
}

enum State {
    Streaming,
    /// The cleaned tail was returned; `Done` follows.
    DonePending(DialogueDelta),
    Ended,
}

/// The stream of one request: its own handle.
struct ChatReply {
    inner: Arc<Inner>,
    request: ChatRequest,
    filter: OutputFilter,
    conversation: ConversationId,
    new: Vec<DialogueMessage>,
    /// The reply text delivered so far.
    reply: String,
    state: State,
}

impl ChatReply {
    fn fail(&mut self, failure: &HttpFailure) -> DialogueError {
        self.request.stop();
        self.state = State::Ended;
        // What the user already got is part of the conversation.
        self.inner
            .commit(&self.conversation, &self.new, &self.reply);
        if !matches!(failure, HttpFailure::Cancelled) {
            tracing::warn!(conversation = %self.conversation, %failure, "dialogue request failed");
        }
        port_error(failure)
    }

    fn complete(
        &mut self,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> Result<DialogueDelta, DialogueError> {
        let end = self.filter.finish();
        self.reply.push_str(&end.tail);
        self.request.detach();
        if self.reply.trim().is_empty() {
            self.state = State::Ended;
            tracing::warn!(conversation = %self.conversation, "the dialogue backend produced no text");
            return Err(DialogueError::EmptyReply);
        }
        self.inner
            .commit(&self.conversation, &self.new, &self.reply);
        let done = DialogueDelta::Done {
            prompt_tokens: prompt_tokens.unwrap_or(0),
            completion_tokens: completion_tokens.unwrap_or(0),
        };
        if end.tail.is_empty() {
            self.state = State::Ended;
            Ok(done)
        } else {
            self.state = State::DonePending(done);
            Ok(DialogueDelta::Text(end.tail))
        }
    }
}

impl DialogueStream for ChatReply {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<DialogueDelta, DialogueError>> {
        loop {
            match std::mem::replace(&mut self.state, State::Streaming) {
                State::Ended => {
                    self.state = State::Ended;
                    return None;
                }
                State::DonePending(done) => {
                    self.state = State::Ended;
                    return Some(Ok(done));
                }
                State::Streaming => {}
            }
            if cancel.is_cancelled() {
                return Some(Err(self.fail(&HttpFailure::Cancelled)));
            }
            match self.request.recv(&self.inner.client, cancel) {
                Received::Failed(failure) => return Some(Err(self.fail(&failure))),
                Received::Chunk(Chunk::Text { text, .. }) => {
                    let clean = self.filter.push(&text);
                    if !clean.is_empty() {
                        self.reply.push_str(&clean);
                        return Some(Ok(DialogueDelta::Text(clean)));
                    }
                }
                Received::Chunk(Chunk::Done {
                    prompt_tokens,
                    completion_tokens,
                    ..
                }) => return Some(self.complete(prompt_tokens, completion_tokens)),
            }
        }
    }
}

impl Drop for ChatReply {
    /// Dropping a stream abandons its request, closes its connection and keeps what was delivered.
    fn drop(&mut self) {
        if !matches!(self.state, State::Ended) {
            self.inner
                .commit(&self.conversation, &self.new, &self.reply);
        }
        self.request.stop();
    }
}

/// The languages a conversation can be held in, for display.
pub fn languages(cfg: &DialogueConfig) -> &[Lang] {
    &cfg.languages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failures_map_to_typed_dialogue_errors() {
        assert_eq!(
            port_error(&HttpFailure::FirstTokenTimeout),
            DialogueError::Timeout
        );
        assert_eq!(
            port_error(&HttpFailure::Cancelled),
            DialogueError::Cancelled
        );
        assert_eq!(
            port_error(&HttpFailure::HttpStatus(401)),
            DialogueError::Rejected { status: 401 }
        );
        assert!(matches!(
            port_error(&HttpFailure::HttpStatus(503)),
            DialogueError::Unavailable { .. }
        ));
        assert!(matches!(
            port_error(&HttpFailure::ConnectFailed("refused".into())),
            DialogueError::Unavailable { .. }
        ));
        assert_ne!(
            port_error(&HttpFailure::IdleTimeout),
            port_error(&HttpFailure::ConnectTimeout),
            "a stall and a connect timeout stay distinguishable"
        );
    }
}
