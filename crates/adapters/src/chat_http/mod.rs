//! The HTTP and SSE client of OpenAI-compatible `/v1/chat/completions` endpoints, shared by the
//! streaming translator (`mt::openai_chat`) and the assistant's dialogue backend
//! (`dialogue::openai_chat`).
//!
//! The ports are synchronous, the HTTP client is not. A `ChatClient` owns a small tokio runtime;
//! every request is one task on it that reads the response, parses the SSE stream (`sse`) and sends
//! the pieces over a channel. The request's `ChatRequest` receives them with a blocking call that
//! also watches the caller's cancellation token. Stopping a request aborts the task, which drops the
//! response and with it the connection, so the server sees the client go away. Requests share
//! nothing but the connection pool.
//!
//! Failures keep their kind ([`HttpFailure`]); each user maps them to its own port error.

pub mod sse;

use std::net::IpAddr;
use std::time::Instant;

use prxvc_ports::CancellationToken;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::redirect::Policy;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::mt::config::{ConfigError, Timeouts};
use sse::{SseEvent, SseParser};

/// Capacity of the channel between a request's task and its stream.
const CHANNEL_CAPACITY: usize = 256;

/// Why a request failed, at the transport level.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HttpFailure {
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
    /// The connection ended or broke before the answer was complete.
    #[error("stream ended early: {0}")]
    StreamBroken(String),
    /// The request was cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
}

/// A piece of a response, as the request's task sends it.
pub(crate) enum Chunk {
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

type Msg = Result<Chunk, HttpFailure>;

/// What waiting for the next piece of a response ended with.
pub(crate) enum Received {
    /// A piece.
    Chunk(Chunk),
    /// The request failed.
    Failed(HttpFailure),
}

/// Body fields the adapters own: an extra parameter may not override them.
pub(crate) const RESERVED_PARAMS: [&str; 4] = ["model", "messages", "stream", "stream_options"];

/// The chat completions URL for `base_url`: `{base}/v1/chat/completions`, tolerating a base that
/// already ends in `/v1` and trailing slashes.
pub(crate) fn endpoint_url(base_url: &str) -> Result<reqwest::Url, ConfigError> {
    let base = base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    let url = reqwest::Url::parse(&format!("{base}/v1/chat/completions"))
        .map_err(|e| ConfigError::InvalidBaseUrl(e.to_string()))?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Ok(url),
        _ => Err(ConfigError::InvalidBaseUrl(format!(
            "`{base_url}` is not an http(s) URL"
        ))),
    }
}

/// Whether `host` is this computer.
pub(crate) fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.eq_ignore_ascii_case("localhost")
        || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The HTTP client and runtime of one endpoint.
pub(crate) struct ChatClient {
    client: reqwest::Client,
    url: reqwest::Url,
    api_key: Option<String>,
    timeouts: Timeouts,
    // Declared last so the runtime is dropped after everything that uses it.
    runtime: tokio::runtime::Runtime,
}

impl ChatClient {
    /// A client for `url`. A loopback server is never reached through the system proxy.
    pub(crate) fn new(
        url: reqwest::Url,
        api_key: Option<String>,
        timeouts: Timeouts,
        thread_name: &str,
    ) -> Result<Self, ConfigError> {
        let local = is_loopback(url.host_str().unwrap_or_default());
        let mut client = reqwest::Client::builder()
            .connect_timeout(timeouts.connect)
            .redirect(Policy::none())
            .tcp_nodelay(true)
            .user_agent("prx-voice-client");
        if local {
            client = client.no_proxy();
        }
        let client = client
            .build()
            .map_err(|e| ConfigError::Client(e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name(thread_name)
            .enable_all()
            .build()
            .map_err(|e| ConfigError::Client(e.to_string()))?;
        Ok(Self {
            client,
            url,
            api_key,
            timeouts,
            runtime,
        })
    }

    /// Send `body` and start reading the response.
    pub(crate) fn start(&self, body: Value) -> ChatRequest {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let started = Instant::now();
        let task = self.runtime.spawn(run_request(
            self.client.clone(),
            self.url.clone(),
            self.api_key.clone(),
            body,
            self.timeouts,
            tx,
            started,
        ));
        ChatRequest {
            rx,
            task: Some(task),
            started,
        }
    }

    /// Block on `future` on the client's runtime.
    pub(crate) fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }
}

/// One request in flight: the receiving end of its task.
pub(crate) struct ChatRequest {
    rx: mpsc::Receiver<Msg>,
    task: Option<JoinHandle<()>>,
    started: Instant,
}

impl ChatRequest {
    /// When the request was sent.
    pub(crate) fn started(&self) -> Instant {
        self.started
    }

    /// Wait for the next piece. Cancelling ends the wait with [`HttpFailure::Cancelled`].
    pub(crate) fn recv(&mut self, client: &ChatClient, cancel: &CancellationToken) -> Received {
        let received = client.block_on(async {
            tokio::select! {
                biased;
                () = cancel.cancelled() => None,
                msg = self.rx.recv() => Some(msg),
            }
        });
        match received {
            None => Received::Failed(HttpFailure::Cancelled),
            Some(None) => Received::Failed(HttpFailure::StreamBroken(
                "the request task ended unexpectedly".into(),
            )),
            Some(Some(Err(failure))) => Received::Failed(failure),
            Some(Some(Ok(chunk))) => Received::Chunk(chunk),
        }
    }

    /// The stream ended normally: let the task finish by itself so the connection can return to
    /// the pool.
    pub(crate) fn detach(&mut self) {
        self.task = None;
    }

    /// Abort the request task: its response and connection are dropped.
    pub(crate) fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.rx.close();
    }
}

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
) -> Result<(), HttpFailure> {
    let deadline = tokio::time::Instant::from_std(started + timeouts.first_token);
    let mut request = client
        .post(url)
        .header(ACCEPT, "text/event-stream")
        .json(body);
    if let Some(key) = api_key {
        request = request.header(AUTHORIZATION, format!("Bearer {key}"));
    }
    let mut response = match tokio::time::timeout_at(deadline, request.send()).await {
        Err(_) => return Err(HttpFailure::FirstTokenTimeout),
        Ok(Err(e)) => return Err(classify_send_error(e)),
        Ok(Ok(r)) => r,
    };
    if !response.status().is_success() {
        return Err(HttpFailure::HttpStatus(response.status().as_u16()));
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
            Err(_) if first_token_seen => return Err(HttpFailure::IdleTimeout),
            Err(_) => return Err(HttpFailure::FirstTokenTimeout),
            Ok(Err(e)) => return Err(HttpFailure::StreamBroken(e.without_url().to_string())),
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
                SseEvent::ServerError { code } => return Err(HttpFailure::ServerEvent(code)),
                SseEvent::Done => return finish(tx, usage, deltas, finish_reason, malformed).await,
            }
        }
        if eof {
            // Some servers close the stream without `[DONE]`. A stop reason
            // says the model finished; without one the connection broke.
            return if finish_reason.is_some() {
                finish(tx, usage, deltas, finish_reason, malformed).await
            } else {
                Err(HttpFailure::StreamBroken(
                    "the connection closed before the end of the answer".into(),
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
) -> Result<(), HttpFailure> {
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

fn classify_send_error(e: reqwest::Error) -> HttpFailure {
    if e.is_connect() && e.is_timeout() {
        HttpFailure::ConnectTimeout
    } else if e.is_connect() {
        HttpFailure::ConnectFailed("connection refused or host unreachable".into())
    } else if e.is_timeout() {
        HttpFailure::FirstTokenTimeout
    } else {
        HttpFailure::ConnectFailed(e.without_url().to_string())
    }
}
