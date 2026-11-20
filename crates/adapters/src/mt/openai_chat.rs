//! `OpenAiChatMt`: streaming translation through an OpenAI-compatible
//! `/v1/chat/completions` endpoint (PD-2.6.1), for example `llama-server`.
//!
//! The port is synchronous, the HTTP client is not. The adapter owns a small
//! tokio runtime; every request is one task on it that reads the response,
//! parses the SSE stream and sends the pieces over a channel. The request's
//! stream object receives them with a blocking call that also watches the
//! caller's cancellation token. Cancelling aborts the task, which drops the
//! response and with it the connection, so the server sees the client go away.
//! Requests share nothing but the connection pool, so two directions run
//! independently.
//!
//! Failures keep their kind: [`MtFailure`] has one variant for each of the
//! three timeouts and for each HTTP and stream failure; the port's `MtError`
//! is the coarser view (see `MtFailure::to_port_error`). The typed failure and
//! the output-quality flags reach the owner through an optional [`MtObserver`].

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_ports::{
    CancellationToken, Egress, MtDelta, MtError, MtRequest, MtStream, ProviderDescriptor,
    StreamingKind, Translator,
};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::redirect::Policy;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::assess::{AssessInput, assess};
use super::config::{ConfigError, OpenAiChatConfig, Timeouts};
use super::output::OutputFilter;
use super::repetition::RepetitionGuard;
use super::report::{MtFailure, MtObserver, MtReport, Stripped, Suspect};
use super::sse::{SseEvent, SseParser};

/// Capacity of the channel between a request's task and its stream.
const CHANNEL_CAPACITY: usize = 256;

struct Inner {
    cfg: OpenAiChatConfig,
    url: reqwest::Url,
    client: reqwest::Client,
    descriptor: ProviderDescriptor,
    observer: Option<Arc<dyn MtObserver>>,
    // Declared last so the runtime is dropped after everything that uses it.
    runtime: tokio::runtime::Runtime,
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

        let mut client = reqwest::Client::builder()
            .connect_timeout(cfg.timeouts.connect)
            .redirect(Policy::none())
            .tcp_nodelay(true)
            .user_agent("prx-voice-client");
        if local {
            // A loopback server is never behind the system proxy.
            client = client.no_proxy();
        }
        let client = client
            .build()
            .map_err(|e| ConfigError::Client(e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mt-http")
            .enable_all()
            .build()
            .map_err(|e| ConfigError::Client(e.to_string()))?;

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
                runtime,
            }),
        })
    }
}

fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.eq_ignore_ascii_case("localhost")
        || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
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
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let started = Instant::now();
        let task = inner.runtime.spawn(run_request(
            inner.client.clone(),
            inner.url.clone(),
            inner.cfg.api_key.as_ref().map(|k| k.expose().to_owned()),
            body,
            inner.cfg.timeouts,
            tx,
            started,
        ));
        tracing::debug!(clause = ?req.clause, model = %inner.cfg.model, "translation request sent");
        Ok(Box::new(ChatStream {
            inner: Arc::clone(&self.inner),
            req,
            rx,
            task: Some(task),
            filter: OutputFilter::new(),
            delivered: String::new(),
            state: State::Streaming,
            started,
            first_token: None,
            deltas: 0,
            guard: RepetitionGuard::new(),
            suspect: None,
        }))
    }
}

/// A piece of a response, as the request's task sends it.
enum Chunk {
    Text {
        text: String,
        at: Instant,
    },
    Done {
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
        finish_reason: Option<String>,
        malformed: u32,
        at: Instant,
    },
}

type Msg = Result<Chunk, MtFailure>;

/// The task of one request.
async fn run_request(
    client: reqwest::Client,
    url: reqwest::Url,
    api_key: Option<String>,
    body: Value,
    timeouts: Timeouts,
    tx: mpsc::Sender<Msg>,
    started: Instant,
) {
    if let Err(failure) = drive(&client, url, api_key, &body, timeouts, &tx, started).await {
        // The stream may already be gone (cancelled or dropped): then nobody
        // is waiting for the failure.
        let _ = tx.send(Err(failure)).await;
    }
}

async fn drive(
    client: &reqwest::Client,
    url: reqwest::Url,
    api_key: Option<String>,
    body: &Value,
    timeouts: Timeouts,
    tx: &mpsc::Sender<Msg>,
    started: Instant,
) -> Result<(), MtFailure> {
    let deadline = tokio::time::Instant::from_std(started + timeouts.first_token);
    let mut request = client
        .post(url)
        .header(ACCEPT, "text/event-stream")
        .json(body);
    if let Some(key) = api_key {
        request = request.header(AUTHORIZATION, format!("Bearer {key}"));
    }
    let mut response = match tokio::time::timeout_at(deadline, request.send()).await {
        Err(_) => return Err(MtFailure::FirstTokenTimeout),
        Ok(Err(e)) => return Err(classify_send_error(e)),
        Ok(Ok(r)) => r,
    };
    if !response.status().is_success() {
        return Err(MtFailure::HttpStatus(response.status().as_u16()));
    }

    let mut parser = SseParser::new();
    let mut first_token_seen = false;
    let mut deltas = 0u32;
    let mut malformed = 0u32;
    let mut usage: Option<(u32, u32)> = None;
    let mut finish_reason: Option<String> = None;
    loop {
        let read = if first_token_seen {
            timeout(timeouts.idle, response.chunk()).await
        } else {
            tokio::time::timeout_at(deadline, response.chunk()).await
        };
        let bytes = match read {
            Err(_) if first_token_seen => return Err(MtFailure::IdleTimeout),
            Err(_) => return Err(MtFailure::FirstTokenTimeout),
            Ok(Err(e)) => return Err(MtFailure::StreamBroken(e.without_url().to_string())),
            Ok(Ok(bytes)) => bytes,
        };
        let (events, eof) = match bytes {
            Some(b) => (parser.push(&b), false),
            None => (parser.finish(), true),
        };
        for event in events {
            match event {
                SseEvent::Content(text) => {
                    first_token_seen = true;
                    deltas = deltas.saturating_add(1);
                    let msg = Chunk::Text {
                        text,
                        at: Instant::now(),
                    };
                    if tx.send(Ok(msg)).await.is_err() {
                        return Ok(());
                    }
                }
                SseEvent::Finish(reason) => finish_reason = Some(reason),
                SseEvent::Usage {
                    prompt_tokens,
                    completion_tokens,
                } => usage = Some((prompt_tokens, completion_tokens)),
                SseEvent::Malformed => malformed = malformed.saturating_add(1),
                SseEvent::ServerError { code } => return Err(MtFailure::ServerEvent(code)),
                SseEvent::Done => return finish(tx, usage, deltas, finish_reason, malformed).await,
            }
        }
        if eof {
            // Some servers close the stream without `[DONE]`. A stop reason
            // says the model finished; without one the connection broke.
            return if finish_reason.is_some() {
                finish(tx, usage, deltas, finish_reason, malformed).await
            } else {
                Err(MtFailure::StreamBroken(
                    "the connection closed before the end of the translation".into(),
                ))
            };
        }
    }
}

async fn finish(
    tx: &mpsc::Sender<Msg>,
    usage: Option<(u32, u32)>,
    deltas: u32,
    finish_reason: Option<String>,
    malformed: u32,
) -> Result<(), MtFailure> {
    let done = Chunk::Done {
        prompt_tokens: usage.map(|(p, _)| p),
        completion_tokens: usage.map(|(_, c)| c).or(Some(deltas)),
        finish_reason,
        malformed,
        at: Instant::now(),
    };
    // A closed channel means the stream was dropped: nothing left to tell.
    let _ = tx.send(Ok(done)).await;
    Ok(())
}

fn classify_send_error(e: reqwest::Error) -> MtFailure {
    if e.is_connect() && e.is_timeout() {
        MtFailure::ConnectTimeout
    } else if e.is_connect() {
        MtFailure::ConnectFailed("connection refused or host unreachable".into())
    } else if e.is_timeout() {
        MtFailure::FirstTokenTimeout
    } else {
        MtFailure::ConnectFailed(e.without_url().to_string())
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
    rx: mpsc::Receiver<Msg>,
    task: Option<JoinHandle<()>>,
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
}

impl MtStream for ChatStream {
    fn suspect(&self) -> Option<String> {
        self.suspect.clone()
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
            let received = self.inner.runtime.block_on(async {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => None,
                    msg = self.rx.recv() => Some(msg),
                }
            });
            match received {
                None => return Some(Err(self.fail(MtFailure::Cancelled))),
                Some(None) => {
                    let failure =
                        MtFailure::StreamBroken("the request task ended unexpectedly".into());
                    return Some(Err(self.fail(failure)));
                }
                Some(Some(Err(failure))) => return Some(Err(self.fail(failure))),
                Some(Some(Ok(Chunk::Text { text, at }))) => {
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
                Some(Some(Ok(Chunk::Done {
                    prompt_tokens,
                    completion_tokens,
                    finish_reason,
                    malformed,
                    at,
                }))) => {
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
        self.task = None;
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
        self.stop_task();
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
        self.stop_task();
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

    /// Abort the request task: its response and connection are dropped.
    fn stop_task(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.rx.close();
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
        self.stop_task();
    }
}
