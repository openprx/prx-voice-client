//! `OpenAiChatMt`: streaming translation through an OpenAI-compatible
//! `/v1/chat/completions` endpoint (PD-2.6.1), for example `llama-server`.
//!
//! The port is synchronous, the HTTP client is not: the shared `chat_http` client owns a small
//! tokio runtime and runs every request as one task that reads the response and parses the SSE
//! stream; the request's stream object receives the pieces with a blocking call that also watches
//! the caller's cancellation token. Cancelling aborts the task, which drops the response and with
//! it the connection, so the server sees the client go away. Requests share nothing but the
//! connection pool, so two directions run independently.
//!
//! Failures keep their kind: [`MtFailure`] has one variant for each of the
//! three timeouts and for each HTTP and stream failure; the port's `MtError`
//! is the coarser view (see `MtFailure::to_port_error`). The typed failure and
//! the output-quality flags reach the owner through an optional [`MtObserver`].

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_ports::{
    CancellationToken, Egress, MtDelta, MtError, MtFlag, MtRequest, MtStream, ProviderDescriptor,
    StreamingKind, Translator,
};
use super::assess::{AssessInput, assess};
use super::config::{ConfigError, OpenAiChatConfig};
use super::output::OutputFilter;
use super::repetition::RepetitionGuard;
use super::report::{MtFailure, MtObserver, MtReport, Stripped, Suspect};
use crate::chat_http::{ChatClient, ChatRequest, Chunk, Received, is_loopback};

struct Inner {
    cfg: OpenAiChatConfig,
    url: reqwest::Url,
    client: ChatClient,
    descriptor: ProviderDescriptor,
    observer: Option<Arc<dyn MtObserver>>,
}

/// The translation provider for OpenAI-compatible chat endpoints.
#[derive(Clone)]
pub struct OpenAiChatMt {
    inner: Arc<Inner>,
}

impl fmt::Debug for OpenAiChatMt {
    /// Shows the endpoint and model; never the API key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatMt")
            .field("provider", &self.inner.cfg.provider_id)
            .field("endpoint", &self.inner.url.as_str())
            .field("model", &self.inner.cfg.model)
            .field("has_api_key", &self.inner.cfg.api_key.is_some())
            .finish()
    }
}

impl OpenAiChatMt {
    /// A translator for `cfg`.
    pub fn new(cfg: OpenAiChatConfig) -> Result<Self, ConfigError> {
        Self::build(cfg, None)
    }

    /// A translator that reports every finished request to `observer`.
    pub fn with_observer(
        cfg: OpenAiChatConfig,
        observer: Arc<dyn MtObserver>,
    ) -> Result<Self, ConfigError> {
        Self::build(cfg, Some(observer))
    }

    fn build(
        cfg: OpenAiChatConfig,
        observer: Option<Arc<dyn MtObserver>>,
    ) -> Result<Self, ConfigError> {
        cfg.validate()?;
        let url = cfg.endpoint_url()?;
        let host = url.host_str().unwrap_or_default().to_owned();
        let local = is_loopback(&host);
        let client = ChatClient::new(
            url.clone(),
            cfg.api_key.as_ref().map(|k| k.expose().to_owned()),
            cfg.timeouts,
            "mt-http",
        )?;

        let egress = if local {
            Egress::Local
        } else {
            Egress::Text { host }
        };
        let descriptor =
            ProviderDescriptor::new(&cfg.provider_id, &cfg.model, StreamingKind::Native)
                .with_languages(cfg.languages.iter().copied())
                .with_egress(egress);
        Ok(Self {
            inner: Arc::new(Inner {
                cfg,
                url,
                client,
                descriptor,
                observer,
            }),
        })
    }
}

impl Translator for OpenAiChatMt {
    fn descriptor(&self) -> ProviderDescriptor {
        self.inner.descriptor.clone()
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        let inner = &self.inner;
        inner
            .descriptor
            .require_pair(req.source, req.target)
            .map_err(|e| MtError::Unavailable {
                reason: e.to_string(),
            })?;
        if req.source == req.target {
            return Err(MtError::Internal(
                "source and target language are the same".into(),
            ));
        }
        if req.text.trim().is_empty() {
            return Err(MtError::Internal("the source clause is empty".into()));
        }
        let body = inner.cfg.request_body(&req);
        let request = inner.client.start(body);
        let started = request.started();
        tracing::debug!(clause = ?req.clause, model = %inner.cfg.model, "translation request sent");
        Ok(Box::new(ChatStream {
            inner: Arc::clone(&self.inner),
            req,
            request,
            filter: OutputFilter::new(),
            delivered: String::new(),
            state: State::Streaming,
            started,
            first_token: None,
            deltas: 0,
            guard: RepetitionGuard::new(),
            suspect: None,
            flags: Vec::new(),
        }))
    }
}

enum State {
    Streaming,
    /// The cleaned tail was returned; `Done` follows.
    DonePending(MtDelta),
    Ended,
}

/// The stream of one request: its own handle.
struct ChatStream {
    inner: Arc<Inner>,
    req: MtRequest,
    request: ChatRequest,
    filter: OutputFilter,
    delivered: String,
    state: State,
    started: Instant,
    first_token: Option<Duration>,
    deltas: u32,
    /// Watches the text as it arrives for a model that loops.
    guard: RepetitionGuard,
    /// Why the request was flagged, once it was (see [`MtStream::suspect`]).
    suspect: Option<String>,
    /// The quality flags of the finished output (see [`MtStream::flags`]).
    flags: Vec<MtFlag>,
}

impl MtStream for ChatStream {
    fn suspect(&self) -> Option<String> {
        self.suspect.clone()
    }

    fn flags(&self) -> Vec<MtFlag> {
        self.flags.clone()
    }

    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<MtDelta, MtError>> {
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
                return Some(Err(self.fail(MtFailure::Cancelled)));
            }
            match self.request.recv(&self.inner.client, cancel) {
                Received::Failed(failure) => return Some(Err(self.fail(failure.into()))),
                Received::Chunk(Chunk::Text { text, at }) => {
                    self.deltas = self.deltas.saturating_add(1);
                    self.first_token
                        .get_or_insert_with(|| at.saturating_duration_since(self.started));
                    let looped = self.guard.push(&text);
                    let clean = self.filter.push(&text);
                    if !clean.is_empty() {
                        self.delivered.push_str(&clean);
                    }
                    if let Some(repetition) = looped {
                        return Some(Ok(self.abort_loop(repetition.reason(), clean)));
                    }
                    if !clean.is_empty() {
                        return Some(Ok(MtDelta::Text(clean)));
                    }
                }
                Received::Chunk(Chunk::Done {
                    prompt_tokens,
                    completion_tokens,
                    finish_reason,
                    malformed,
                    at,
                }) => {
                    return Some(self.complete(
                        prompt_tokens,
                        completion_tokens,
                        finish_reason,
                        malformed,
                        at,
                    ));
                }
            }
        }
    }
}

impl ChatStream {
    /// The stream ended normally: settle the filter, assess, report.
    fn complete(
        &mut self,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
        finish_reason: Option<String>,
        malformed: u32,
        at: Instant,
    ) -> Result<MtDelta, MtError> {
        let end = self.filter.finish();
        self.delivered.push_str(&end.tail);
        let suspects = assess(&AssessInput {
            source: &self.req.text,
            source_lang: self.req.source,
            target_lang: self.req.target,
            output: &end.text,
            finish_reason: finish_reason.as_deref(),
        });
        self.flags = suspects.iter().filter_map(|s| s.as_flag()).collect();
        let done = MtDelta::Done {
            prompt_tokens: prompt_tokens.unwrap_or(0),
            completion_tokens: completion_tokens.unwrap_or(self.deltas),
        };
        let failure = suspects
            .contains(&Suspect::Empty)
            .then_some(MtFailure::EmptyOutput);
        self.report(MtReportParts {
            failure: failure.clone(),
            suspects,
            stripped: end.stripped,
            text: end.text,
            total: at.saturating_duration_since(self.started),
            prompt_tokens: prompt_tokens.unwrap_or(0),
            completion_tokens: completion_tokens.unwrap_or(self.deltas),
            malformed_lines: malformed,
        });
        // The task ends by itself after `Done`; detach it so that the
        // connection can return to the pool.
        self.request.detach();
        if let Some(failure) = failure {
            self.state = State::Ended;
            return Err(failure.to_port_error());
        }
        if end.tail.is_empty() {
            self.state = State::Ended;
            Ok(done)
        } else {
            self.state = State::DonePending(done);
            Ok(MtDelta::Text(end.tail))
        }
    }

    /// The model loops: stop the request now, flag it with `reason` and end the stream normally.
    /// `last` is the clean text of the delta that showed the loop; it is delivered, the pipeline
    /// reads the flag right after it and drops the request without speaking it.
    fn abort_loop(&mut self, reason: String, last: String) -> MtDelta {
        self.request.stop();
        self.suspect = Some(reason);
        self.report(MtReportParts {
            failure: None,
            suspects: vec![Suspect::Repetition],
            stripped: Vec::new(),
            text: self.delivered.clone(),
            total: self.started.elapsed(),
            prompt_tokens: 0,
            completion_tokens: self.deltas,
            malformed_lines: 0,
        });
        let done = MtDelta::Done {
            prompt_tokens: 0,
            completion_tokens: self.deltas,
        };
        if last.is_empty() {
            self.state = State::Ended;
            done
        } else {
            self.state = State::DonePending(done);
            MtDelta::Text(last)
        }
    }

    /// End the stream with `failure`: stop the request, report, return the port error.
    fn fail(&mut self, failure: MtFailure) -> MtError {
        self.request.stop();
        self.state = State::Ended;
        self.report(MtReportParts {
            failure: Some(failure.clone()),
            suspects: Vec::new(),
            stripped: Vec::new(),
            text: self.delivered.clone(),
            total: self.started.elapsed(),
            prompt_tokens: 0,
            completion_tokens: self.deltas,
            malformed_lines: 0,
        });
        failure.to_port_error()
    }

    fn report(&self, parts: MtReportParts) {
        if !parts.suspects.is_empty() {
            // Kinds only: the text itself is never logged.
            tracing::warn!(clause = ?self.req.clause, suspects = ?parts.suspects, "translation looks suspect");
        }
        if let Some(failure) = &parts.failure
            && *failure != MtFailure::Cancelled
        {
            tracing::warn!(clause = ?self.req.clause, %failure, "translation failed");
        }
        if let Some(observer) = &self.inner.observer {
            observer.on_report(&MtReport {
                clause: self.req.clause,
                failure: parts.failure,
                suspects: parts.suspects,
                stripped: parts.stripped,
                text: parts.text,
                first_token: self.first_token,
                total: parts.total,
                prompt_tokens: parts.prompt_tokens,
                completion_tokens: parts.completion_tokens,
                malformed_lines: parts.malformed_lines,
            });
        }
    }
}

/// The fields of a report that differ between ending paths.
struct MtReportParts {
    failure: Option<MtFailure>,
    suspects: Vec<Suspect>,
    stripped: Vec<Stripped>,
    text: String,
    total: Duration,
    prompt_tokens: u32,
    completion_tokens: u32,
    malformed_lines: u32,
}

impl Drop for ChatStream {
    /// Dropping a stream abandons its request and closes its connection.
    fn drop(&mut self) {
        self.request.stop();
    }
}
