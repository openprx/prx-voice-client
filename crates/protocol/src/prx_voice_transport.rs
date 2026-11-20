//! Persistent-connection transport for the prx-voice server.
//!
//! [`PrxVoiceTransport`] implements [`Transport`](crate::transport::Transport)
//! using a single long-lived WebSocket to `/api/v1/stream`. Sessions are
//! created and closed as lightweight JSON messages on the existing connection,
//! eliminating per-conversation handshake latency.
//!
//! # Session identifiers
//!
//! The engine owns its own session IDs; the server issues a `sid` in
//! `session_created`. The transport keeps exactly one mapping between the two
//! (the [`SessionTable`]): outgoing messages always carry the server `sid`,
//! inbound messages and audio are mapped back to the engine's session ID.
//! Messages that name an unknown `sid` are dropped. Messages that carry no
//! `sid` at all (the server omits it on `token`, `tts_start`, ...) are
//! attributed to the connection's only session, and dropped if there is not
//! exactly one.
//!
//! # Runtime ownership
//!
//! Each connection runs on its own OS thread, which builds and drops its own
//! single-threaded tokio runtime. The caller's thread (possibly a thread that
//! is inside another tokio runtime) never creates or drops a runtime, so
//! `connect` / `disconnect` are safe to call from async contexts.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc as tokio_mpsc;

use crate::transport::{Transport, TransportError, TransportState, TtsAudioChunk};
use crate::{Command, Event};

/// How long `create_session` waits for the server's `session_created`.
const CREATE_SESSION_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound for establishing the WebSocket.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Client ping interval.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// A connection that has been silent this long is considered dead.
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(30);
/// Sample rate assumed until the server announces one in `tts_start`.
const DEFAULT_TTS_SAMPLE_RATE: u32 = 16_000;
/// Length of the hyphenated session UUID that prefixes binary frames.
const FRAME_PREFIX_LEN: usize = 36;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for connecting to a prx-voice server.
#[derive(Debug, Clone)]
pub struct PrxVoiceConfig {
    /// Base URL of the prx-voice server (e.g., `"http://localhost:3000"`).
    pub base_url: String,
    /// Language for sessions (e.g., `"zh-CN"`, `"en-US"`).
    pub language: String,
    /// ASR provider preference (e.g., `"sherpa"`, `"deepgram"`, `"mock"`).
    pub asr_provider: String,
    /// Agent provider preference (e.g., `"ollama"`, `"openai"`, `"mock"`).
    pub agent_provider: String,
    /// TTS provider preference (e.g., `"sherpa"`, `"azure"`, `"mock"`).
    pub tts_provider: String,
    /// Channel type identifier.
    pub channel: String,
    /// Client ID for connection deduplication (set once, reused across reconnects).
    pub client_id: String,
}

impl Default for PrxVoiceConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:3000".into(),
            language: "zh-CN".into(),
            asr_provider: "sherpa".into(),
            agent_provider: "ollama".into(),
            tts_provider: "sherpa".into(),
            channel: "app_sdk".into(),
            client_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Outgoing WebSocket message wrapper
// ---------------------------------------------------------------------------

/// Outgoing message types for the WebSocket writer.
enum WsOutgoing {
    /// A text (JSON) message.
    Text(String),
    /// A binary message (36-byte UUID hex + PCM16 audio data).
    Binary(Vec<u8>),
}

// ---------------------------------------------------------------------------
// Session mapping
// ---------------------------------------------------------------------------

/// Parse the UUID out of a server `sid` (`"sess-{uuid}"` or a bare UUID).
fn parse_server_sid(sid: &str) -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(sid.strip_prefix("sess-").unwrap_or(sid)).ok()
}

/// One session as the transport sees it.
struct SessionEntry {
    /// The `sid` the server issued, sent verbatim on every outgoing message.
    server_sid: String,
    /// The UUID inside `server_sid`, used as the binary frame prefix.
    server_uuid: uuid::Uuid,
    /// Set by an interrupt, cleared by the next `tts_start`: audio of the
    /// interrupted reply that is still in flight is dropped.
    interrupted: bool,
}

/// The single mapping between engine session IDs and server `sid`s.
#[derive(Default)]
struct SessionTable {
    by_engine: HashMap<uuid::Uuid, SessionEntry>,
    server_to_engine: HashMap<uuid::Uuid, uuid::Uuid>,
}

impl SessionTable {
    /// Bind `engine_id` to the server's `sid`. Returns `false` if the `sid`
    /// is not a valid server session identifier.
    fn bind(&mut self, engine_id: uuid::Uuid, server_sid: &str) -> bool {
        let Some(server_uuid) = parse_server_sid(server_sid) else {
            return false;
        };
        self.unbind(&engine_id);
        self.server_to_engine.insert(server_uuid, engine_id);
        self.by_engine.insert(
            engine_id,
            SessionEntry {
                server_sid: server_sid.to_owned(),
                server_uuid,
                interrupted: false,
            },
        );
        true
    }

    fn unbind(&mut self, engine_id: &uuid::Uuid) {
        if let Some(entry) = self.by_engine.remove(engine_id) {
            self.server_to_engine.remove(&entry.server_uuid);
        }
    }

    fn get(&self, engine_id: &uuid::Uuid) -> Option<&SessionEntry> {
        self.by_engine.get(engine_id)
    }

    fn get_mut(&mut self, engine_id: &uuid::Uuid) -> Option<&mut SessionEntry> {
        self.by_engine.get_mut(engine_id)
    }

    fn engine_for_server_uuid(&self, server_uuid: &uuid::Uuid) -> Option<uuid::Uuid> {
        self.server_to_engine.get(server_uuid).copied()
    }

    /// The engine session of a connection that has exactly one session.
    fn sole_session(&self) -> Option<uuid::Uuid> {
        if self.by_engine.len() == 1 {
            self.by_engine.keys().next().copied()
        } else {
            None
        }
    }
}

/// A `create_session` request waiting for its `session_created`.
struct PendingCreate {
    engine_id: uuid::Uuid,
    reply: std_mpsc::Sender<Result<String, String>>,
}

/// State shared between the transport and its connection thread.
struct ConnShared {
    /// Cleared by the connection thread when the socket is gone.
    alive: AtomicBool,
    sessions: parking_lot::Mutex<SessionTable>,
    /// Pending creates keyed by `req_id`.
    pending_creates: parking_lot::Mutex<HashMap<String, PendingCreate>>,
}

// ---------------------------------------------------------------------------
// Server message mapping
// ---------------------------------------------------------------------------

/// Map a prx-voice server JSON message to our [`Event`] enum.
///
/// Session ID is set to `Uuid::nil()` — the caller fills in the correct value.
pub(crate) fn map_server_message(
    msg_type: &str,
    json: &serde_json::Value,
) -> Option<Event> {
    match msg_type {
        "status" => {
            let status = json["status"].as_str()?;
            let to = match status {
                "recording" => "UserSpeaking",
                "thinking" => "Thinking",
                "listening" => "Listening",
                "interrupted" => "Interrupted",
                "closed" => "Completed",
                other => other,
            };
            Some(Event::SessionStateChanged {
                session_id: uuid::Uuid::nil(),
                from: String::new(),
                to: to.to_string(),
            })
        }
        "transcript" => {
            let text = json["text"].as_str()?.to_string();
            Some(Event::InterimTranscriptProduced {
                session_id: uuid::Uuid::nil(),
                text,
            })
        }
        "token" => {
            let token = json["token"].as_str()?.to_string();
            Some(Event::AssistantReplyTextDelta {
                session_id: uuid::Uuid::nil(),
                delta: token,
            })
        }
        "response" => {
            let agent_text = json["agent_text"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            Some(Event::AssistantReplyTextDelta {
                session_id: uuid::Uuid::nil(),
                delta: agent_text,
            })
        }
        "tts_start" => Some(Event::AssistantReplyStarted {
            session_id: uuid::Uuid::nil(),
        }),
        "tts_end" => Some(Event::PlaybackCompleted {
            session_id: uuid::Uuid::nil(),
        }),
        "error" | "session_error" => {
            let message = json["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string();
            Some(Event::ErrorOccurred {
                code: "server.error".to_owned(),
                message,
                recoverable: true,
            })
        }
        // Silently ignored message types.
        "no_speech" | "heartbeat" | "connected" | "ping" | "pong" => None,
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Inbound handling (runs on the connection thread)
// ---------------------------------------------------------------------------

/// Message types that belong to a session.
fn is_session_scoped(msg_type: &str) -> bool {
    matches!(
        msg_type,
        "status" | "transcript" | "token" | "response" | "tts_start" | "tts_end" | "no_speech"
    )
}

/// Decodes server messages into events and audio chunks.
struct Inbound {
    shared: Arc<ConnShared>,
    event_tx: std_mpsc::Sender<Event>,
    audio_tx: std_mpsc::Sender<TtsAudioChunk>,
    out_tx: tokio_mpsc::UnboundedSender<WsOutgoing>,
    /// Rate announced by the latest `tts_start` on this connection.
    tts_rate: u32,
}

impl Inbound {
    /// Handle a text frame. Returns `false` when the consumer is gone.
    fn on_text(&mut self, text: &str) -> bool {
        let json: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse server message as JSON: {e}");
                return self
                    .event_tx
                    .send(Event::ErrorOccurred {
                        code: "protocol.parse_error".to_owned(),
                        message: format!("failed to parse server message: {e}"),
                        recoverable: true,
                    })
                    .is_ok();
            }
        };
        let msg_type = json["type"].as_str().unwrap_or_default();

        match msg_type {
            "pong" | "connected" | "session_closed" | "heartbeat" => return true,
            "ping" => {
                let pong = serde_json::json!({
                    "type": "pong",
                    "ts": chrono::Utc::now().to_rfc3339(),
                });
                let _ = self.out_tx.send(WsOutgoing::Text(pong.to_string()));
                return true;
            }
            "session_created" => return self.on_session_created(&json),
            "session_error" => {
                if self.fail_pending_create(&json) {
                    return true;
                }
            }
            _ => {}
        }

        // Resolve which engine session (if any) the message belongs to.
        let engine_id = if is_session_scoped(msg_type) {
            match self.resolve_session(&json) {
                Some(id) => Some(id),
                None => {
                    tracing::debug!(msg_type, "dropping message for unknown or ambiguous session");
                    return true;
                }
            }
        } else {
            None
        };

        if msg_type == "tts_start" {
            self.tts_rate = json["sample_rate"]
                .as_u64()
                .and_then(|r| u32::try_from(r).ok())
                .filter(|r| *r > 0)
                .unwrap_or(DEFAULT_TTS_SAMPLE_RATE);
            if let Some(id) = engine_id {
                if let Some(entry) = self.shared.sessions.lock().get_mut(&id) {
                    entry.interrupted = false;
                }
            }
        }

        match map_server_message(msg_type, &json) {
            Some(mut event) => {
                if let Some(id) = engine_id {
                    patch_event_session_id(&mut event, id);
                }
                self.event_tx.send(event).is_ok()
            }
            None => true,
        }
    }

    /// Handle a binary frame: `[36-byte session UUID][PCM16]`.
    fn on_binary(&mut self, data: &[u8]) -> bool {
        if data.len() <= FRAME_PREFIX_LEN {
            tracing::debug!(len = data.len(), "dropping binary frame without payload");
            return true;
        }
        let (prefix, pcm) = data.split_at(FRAME_PREFIX_LEN);
        let server_uuid = std::str::from_utf8(prefix)
            .ok()
            .and_then(|s| uuid::Uuid::parse_str(s).ok());
        let Some(server_uuid) = server_uuid else {
            tracing::debug!("dropping binary frame with malformed session prefix");
            return true;
        };
        let engine_id = {
            let sessions = self.shared.sessions.lock();
            match sessions.engine_for_server_uuid(&server_uuid) {
                Some(id) if sessions.get(&id).is_some_and(|e| !e.interrupted) => id,
                _ => return true,
            }
        };
        self.audio_tx
            .send(TtsAudioChunk {
                session_id: engine_id,
                sample_rate: self.tts_rate,
                pcm16: pcm.to_vec(),
            })
            .is_ok()
    }

    fn resolve_session(&self, json: &serde_json::Value) -> Option<uuid::Uuid> {
        let sessions = self.shared.sessions.lock();
        match json["sid"].as_str() {
            Some(sid) => parse_server_sid(sid).and_then(|u| sessions.engine_for_server_uuid(&u)),
            None => sessions.sole_session(),
        }
    }

    /// Register the session the server created for a pending request.
    fn on_session_created(&mut self, json: &serde_json::Value) -> bool {
        let req_id = json["req_id"].as_str().unwrap_or_default();
        let sid = json["sid"].as_str().unwrap_or_default();
        let Some(pending) = self.shared.pending_creates.lock().remove(req_id) else {
            tracing::warn!(req_id, sid, "session_created for an unknown request");
            return true;
        };
        let result = if self.shared.sessions.lock().bind(pending.engine_id, sid) {
            Ok(sid.to_owned())
        } else {
            Err(format!("server returned an invalid session id '{sid}'"))
        };
        let _ = pending.reply.send(result);
        true
    }

    /// Fail the pending create a `session_error` refers to, if any.
    fn fail_pending_create(&mut self, json: &serde_json::Value) -> bool {
        let req_id = json["req_id"].as_str().unwrap_or_default();
        let Some(pending) = self.shared.pending_creates.lock().remove(req_id) else {
            return false;
        };
        let message = json["message"].as_str().unwrap_or("session creation failed");
        let _ = pending.reply.send(Err(message.to_owned()));
        true
    }
}

/// Body of the connection thread: connect, then pump the socket until it
/// closes or `shutdown` fires.
#[allow(clippy::too_many_arguments)]
async fn run_connection(
    ws_url: String,
    mut inbound: Inbound,
    mut out_rx: tokio_mpsc::UnboundedReceiver<WsOutgoing>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ready_tx: std_mpsc::Sender<Result<(), TransportError>>,
) {
    use tokio_tungstenite::tungstenite::Message;

    let connect = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(&ws_url));
    let mut ws = match connect.await {
        Ok(Ok((ws, _response))) => ws,
        Ok(Err(e)) => {
            let _ = ready_tx.send(Err(TransportError::ConnectionFailed(format!(
                "WebSocket connection to {ws_url} failed: {e}"
            ))));
            return;
        }
        Err(_) => {
            let _ = ready_tx.send(Err(TransportError::ConnectionFailed(format!(
                "WebSocket connection to {ws_url} timed out"
            ))));
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));
    drop(ready_tx);

    let shared = Arc::clone(&inbound.shared);
    let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
    ticker.tick().await; // the first tick completes immediately
    let mut last_inbound = Instant::now();
    let mut close_reason: Option<(&'static str, String)> = None;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                // Either a shutdown request or the transport was dropped.
                let _ = changed;
                break;
            }
            msg = ws.next() => {
                last_inbound = Instant::now();
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if !inbound.on_text(&text) { break; }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if !inbound.on_binary(&data) { break; }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        close_reason = Some((
                            "transport.closed",
                            "WebSocket connection closed by server".to_owned(),
                        ));
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::error!("WebSocket read error: {e}");
                        close_reason = Some((
                            "transport.read_error",
                            format!("WebSocket read error: {e}"),
                        ));
                        break;
                    }
                }
            }
            out = out_rx.recv() => {
                let Some(out) = out else { break };
                let ws_msg = match out {
                    WsOutgoing::Text(text) => Message::Text(text.into()),
                    WsOutgoing::Binary(data) => Message::Binary(data.into()),
                };
                if let Err(e) = ws.send(ws_msg).await {
                    tracing::error!("WebSocket write error: {e}");
                    close_reason = Some((
                        "transport.write_error",
                        format!("WebSocket write error: {e}"),
                    ));
                    break;
                }
            }
            _ = ticker.tick() => {
                if last_inbound.elapsed() > LIVENESS_TIMEOUT {
                    close_reason = Some((
                        "transport.heartbeat_timeout",
                        "no traffic from server".to_owned(),
                    ));
                    break;
                }
                let ping = serde_json::json!({
                    "type": "ping",
                    "ts": chrono::Utc::now().to_rfc3339(),
                });
                if let Err(e) = ws.send(Message::Text(ping.to_string().into())).await {
                    close_reason = Some((
                        "transport.write_error",
                        format!("WebSocket write error: {e}"),
                    ));
                    break;
                }
            }
        }
    }

    // Publish the loss before anything else so `state()` flips promptly.
    shared.alive.store(false, Ordering::Release);
    // Wake any `create_session` still waiting on this connection.
    shared.pending_creates.lock().clear();
    if let Some((code, message)) = close_reason {
        let _ = inbound.event_tx.send(Event::ErrorOccurred {
            code: code.to_owned(),
            message,
            recoverable: true,
        });
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), ws.close(None)).await;
}

// ---------------------------------------------------------------------------
// Connection handle
// ---------------------------------------------------------------------------

/// Everything that belongs to one live connection.
struct Connection {
    shared: Arc<ConnShared>,
    outgoing: tokio_mpsc::UnboundedSender<WsOutgoing>,
    events: parking_lot::Mutex<std_mpsc::Receiver<Event>>,
    audio: parking_lot::Mutex<std_mpsc::Receiver<TtsAudioChunk>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

impl Connection {
    /// Stop the connection thread and wait for it. The thread builds and
    /// drops its own runtime, so this is safe from any context.
    fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Transport that maintains a persistent WebSocket to a prx-voice server.
///
/// Lifecycle:
/// 1. `connect()` — Establish persistent WebSocket to `/api/v1/stream` on a
///    dedicated thread; sessions that were open before a reconnect are
///    re-created on the new connection under the same engine IDs.
/// 2. `create_session(id)` — Send `create_session`, wait for `session_created`,
///    remember the server's `sid` for `id`.
/// 3. `send_command(StartSession)` — Send `audio_start` over existing WS.
/// 4. `send_audio_data()` — Binary frames with 36-byte UUID prefix.
/// 5. `send_command(StopSession)` — Send `close_session`, WS stays open.
/// 6. `disconnect()` — Tear down the persistent connection.
pub struct PrxVoiceTransport {
    /// Server configuration.
    config: PrxVoiceConfig,
    /// Current connection state (see [`Transport::state`] for the live value).
    state: TransportState,
    /// Pending events waiting to be polled.
    event_queue: VecDeque<Event>,
    /// History of all commands sent through this transport.
    command_log: Vec<Command>,
    /// The live connection, if any.
    conn: Option<Connection>,
    /// Engine sessions to re-create after a reconnect: id -> started.
    resumable: HashMap<uuid::Uuid, bool>,

    // -- Reconnection --
    reconnect_attempts: u32,
    max_reconnect_attempts: u32,
}

impl PrxVoiceTransport {
    /// Create a new `PrxVoiceTransport` with the given configuration.
    pub fn new(config: PrxVoiceConfig) -> Self {
        Self {
            config,
            state: TransportState::Disconnected,
            event_queue: VecDeque::new(),
            command_log: Vec::new(),
            conn: None,
            resumable: HashMap::new(),
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
        }
    }

    /// Get the current transport state.
    pub fn transport_state(&self) -> TransportState {
        self.state()
    }

    /// Get the current reconnection attempt count.
    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts
    }

    /// Get the maximum reconnection attempts.
    pub fn max_reconnect_attempts(&self) -> u32 {
        self.max_reconnect_attempts
    }

    /// Get the server configuration.
    pub fn config(&self) -> &PrxVoiceConfig {
        &self.config
    }

    /// The server `sid` the given engine session is mapped to, if any.
    pub fn server_sid(&self, session_id: &uuid::Uuid) -> Option<String> {
        let conn = self.conn.as_ref()?;
        conn.shared
            .sessions
            .lock()
            .get(session_id)
            .map(|e| e.server_sid.clone())
    }

    /// Return a reference to all commands sent through this transport.
    pub fn command_log(&self) -> &[Command] {
        &self.command_log
    }

    /// Convert an HTTP base URL to a WebSocket URL.
    fn to_ws_url(base_url: &str) -> String {
        if base_url.starts_with("https://") {
            format!("wss://{}", &base_url["https://".len()..])
        } else if base_url.starts_with("http://") {
            format!("ws://{}", &base_url["http://".len()..])
        } else {
            format!("ws://{base_url}")
        }
    }

    /// Stop and drop the current connection, if any.
    fn teardown_connection(&mut self) {
        if let Some(conn) = self.conn.take() {
            conn.shutdown();
        }
    }

    fn connection(&self) -> Result<&Connection, TransportError> {
        self.conn
            .as_ref()
            .ok_or_else(|| TransportError::SendFailed("no active WebSocket writer".to_owned()))
    }

    /// Send a text message over the WebSocket channel.
    fn send_ws_text(&self, text: String) -> Result<(), TransportError> {
        self.connection()?
            .outgoing
            .send(WsOutgoing::Text(text))
            .map_err(|e| {
                TransportError::SendFailed(format!("WebSocket writer channel closed: {e}"))
            })
    }

    /// Look up the server `sid` for an engine session or fail.
    fn require_server_sid(&self, session_id: &uuid::Uuid) -> Result<String, TransportError> {
        self.server_sid(session_id).ok_or_else(|| {
            TransportError::SendFailed(format!(
                "unknown session {session_id} — create a session first"
            ))
        })
    }

    /// Create a server session for `engine_id` and wait (up to 5 s) for the
    /// server's `session_created`.
    ///
    /// This is synchronous because the engine starts the session right after
    /// creating it, and every later message needs the server's `sid`.
    fn handle_create_session(&mut self, engine_id: uuid::Uuid) -> Result<(), TransportError> {
        let req_id = uuid::Uuid::new_v4().to_string();
        let (reply_tx, reply_rx) = std_mpsc::channel();
        let shared = Arc::clone(&self.connection()?.shared);
        shared.pending_creates.lock().insert(
            req_id.clone(),
            PendingCreate {
                engine_id,
                reply: reply_tx,
            },
        );

        let msg = serde_json::json!({
            "type": "create_session",
            "req_id": req_id,
            "channel": self.config.channel,
            "language": self.config.language,
            "asr_providers": [self.config.asr_provider],
            "agent_providers": [self.config.agent_provider],
            "tts_providers": [self.config.tts_provider],
        });
        if let Err(e) = self.send_ws_text(msg.to_string()) {
            shared.pending_creates.lock().remove(&req_id);
            return Err(e);
        }
        tracing::info!(%req_id, %engine_id, "create_session sent over persistent WS");

        match reply_rx.recv_timeout(CREATE_SESSION_TIMEOUT) {
            Ok(Ok(server_sid)) => {
                tracing::info!(%engine_id, %server_sid, "session created on persistent WS");
                self.resumable.entry(engine_id).or_insert(false);
                self.event_queue.push_back(Event::SessionCreated {
                    session_id: engine_id,
                });
                Ok(())
            }
            Ok(Err(message)) => {
                tracing::error!(%message, "session creation failed on server");
                Err(TransportError::ReceiveFailed(format!(
                    "session creation failed: {message}"
                )))
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                shared.pending_creates.lock().remove(&req_id);
                Err(TransportError::Timeout(
                    CREATE_SESSION_TIMEOUT.as_millis() as u64
                ))
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => Err(TransportError::Disconnected(
                "connection closed while waiting for session_created".into(),
            )),
        }
    }

    /// Handle the `StartSession` command — send `audio_start`.
    fn handle_start_session(&mut self, session_id: &uuid::Uuid) -> Result<(), TransportError> {
        let sid = self.require_server_sid(session_id)?;
        let msg = serde_json::json!({ "type": "audio_start", "sid": sid });
        self.send_ws_text(msg.to_string())?;
        self.resumable.insert(*session_id, true);
        self.event_queue.push_back(Event::SessionStarted {
            session_id: *session_id,
        });
        Ok(())
    }

    /// Handle the `StopSession` command — close session, keep WS alive.
    fn handle_stop_session(&mut self, session_id: &uuid::Uuid) -> Result<(), TransportError> {
        self.resumable.remove(session_id);
        if let Some(sid) = self.server_sid(session_id) {
            let msg = serde_json::json!({
                "type": "close_session",
                "sid": sid,
                "reason": "user_ended",
            });
            let _ = self.send_ws_text(msg.to_string());
        } else {
            tracing::debug!(%session_id, "StopSession for a session the server does not know");
        }

        // Clean up local session state — but keep the WebSocket open.
        if let Some(conn) = &self.conn {
            conn.shared.sessions.lock().unbind(session_id);
        }

        self.event_queue.push_back(Event::SessionStateChanged {
            session_id: *session_id,
            from: String::new(),
            to: "Completed".to_string(),
        });
        Ok(())
    }

    /// Handle the `InterruptPlayback` command.
    fn handle_interrupt_playback(
        &mut self,
        session_id: &uuid::Uuid,
    ) -> Result<(), TransportError> {
        let sid = self.require_server_sid(session_id)?;
        // Drop the rest of the interrupted reply that is still on the wire.
        if let Some(conn) = &self.conn {
            if let Some(entry) = conn.shared.sessions.lock().get_mut(session_id) {
                entry.interrupted = true;
            }
        }
        let msg = serde_json::json!({ "type": "interrupt", "sid": sid });
        self.send_ws_text(msg.to_string())?;
        self.event_queue.push_back(Event::PlaybackInterrupted {
            session_id: *session_id,
        });
        Ok(())
    }

    /// Handle the `SendText` command — send recognized text to the server.
    ///
    /// The server receives `{"type":"text","sid":"...","text":"..."}` and
    /// routes directly to the Agent pipeline, bypassing server-side ASR.
    fn handle_send_text(
        &mut self,
        session_id: &uuid::Uuid,
        text: &str,
    ) -> Result<(), TransportError> {
        let sid = self.require_server_sid(session_id)?;
        let msg = serde_json::json!({ "type": "text", "sid": sid, "text": text });
        self.send_ws_text(msg.to_string())?;
        tracing::info!(server_sid = %sid, text_len = text.len(), "sent text to server");
        Ok(())
    }

    /// Re-create sessions that were open before the connection was lost.
    fn resume_sessions(&mut self) {
        let sessions: Vec<(uuid::Uuid, bool)> =
            self.resumable.iter().map(|(k, v)| (*k, *v)).collect();
        for (engine_id, started) in sessions {
            match self.handle_create_session(engine_id) {
                Ok(()) => {
                    if started {
                        if let Err(e) = self.handle_start_session(&engine_id) {
                            tracing::warn!(%engine_id, "resume: audio_start failed: {e}");
                        }
                    }
                    // The engine already knows this session.
                    self.event_queue.retain(|e| {
                        !matches!(e, Event::SessionCreated { session_id }
                            if *session_id == engine_id)
                            && !matches!(e, Event::SessionStarted { session_id }
                                if *session_id == engine_id)
                    });
                }
                Err(e) => {
                    tracing::warn!(%engine_id, "resume: session could not be re-created: {e}");
                    self.resumable.remove(&engine_id);
                }
            }
        }
    }
}

impl Drop for PrxVoiceTransport {
    fn drop(&mut self) {
        self.teardown_connection();
    }
}

/// Patch the `session_id` field in events that carry one.
fn patch_event_session_id(event: &mut Event, session_id: uuid::Uuid) {
    match event {
        Event::SessionCreated { session_id: sid }
        | Event::SessionStarted { session_id: sid }
        | Event::InterimTranscriptProduced { session_id: sid, .. }
        | Event::FinalTranscriptProduced { session_id: sid, .. }
        | Event::AssistantReplyStarted { session_id: sid }
        | Event::AssistantReplyTextDelta { session_id: sid, .. }
        | Event::PlaybackCompleted { session_id: sid }
        | Event::PlaybackInterrupted { session_id: sid }
        | Event::SessionStateChanged { session_id: sid, .. } => {
            *sid = session_id;
        }
        Event::AppReady
        | Event::DeviceListUpdated { .. }
        | Event::ErrorOccurred { .. }
        | Event::DiagnosticsExported { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Transport trait implementation
// ---------------------------------------------------------------------------

impl Transport for PrxVoiceTransport {
    fn connect(&mut self) -> Result<(), TransportError> {
        // Clean up any existing connection (joins its thread).
        self.teardown_connection();
        self.state = TransportState::Connecting;

        // Validate base URL.
        if let Err(e) = url::Url::parse(&self.config.base_url) {
            self.state = TransportState::Disconnected;
            return Err(TransportError::ConnectionFailed(format!(
                "invalid base URL '{}': {e}",
                self.config.base_url
            )));
        }
        let ws_url = format!("{}/api/v1/stream", Self::to_ws_url(&self.config.base_url));

        let shared = Arc::new(ConnShared {
            alive: AtomicBool::new(true),
            sessions: parking_lot::Mutex::new(SessionTable::default()),
            pending_creates: parking_lot::Mutex::new(HashMap::new()),
        });
        let (event_tx, event_rx) = std_mpsc::channel::<Event>();
        let (audio_tx, audio_rx) = std_mpsc::channel::<TtsAudioChunk>();
        let (out_tx, out_rx) = tokio_mpsc::unbounded_channel::<WsOutgoing>();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<(), TransportError>>();

        let inbound = Inbound {
            shared: Arc::clone(&shared),
            event_tx,
            audio_tx,
            out_tx: out_tx.clone(),
            tts_rate: DEFAULT_TTS_SAMPLE_RATE,
        };

        // The connection owns a runtime on its own thread: no runtime is ever
        // created or dropped on the caller's thread, which may be inside one.
        let thread = std::thread::Builder::new()
            .name("prxvc-transport".to_owned())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(TransportError::ConnectionFailed(format!(
                            "failed to create runtime: {e}"
                        ))));
                        return;
                    }
                };
                rt.block_on(run_connection(ws_url, inbound, out_rx, shutdown_rx, ready_tx));
            })
            .map_err(|e| {
                self.state = TransportState::Disconnected;
                TransportError::ConnectionFailed(format!("failed to spawn transport thread: {e}"))
            })?;

        let ready = ready_rx
            .recv_timeout(CONNECT_TIMEOUT + Duration::from_secs(2))
            .unwrap_or_else(|_| {
                Err(TransportError::ConnectionFailed(
                    "transport thread did not report a connection result".into(),
                ))
            });
        let conn = Connection {
            shared,
            outgoing: out_tx,
            events: parking_lot::Mutex::new(event_rx),
            audio: parking_lot::Mutex::new(audio_rx),
            shutdown: shutdown_tx,
            thread: Some(thread),
        };
        if let Err(e) = ready {
            conn.shutdown();
            self.state = TransportState::Disconnected;
            return Err(e);
        }

        self.conn = Some(conn);
        self.state = TransportState::Connected;
        self.reconnect_attempts = 0;
        self.resume_sessions();
        self.event_queue.push_back(Event::AppReady);

        tracing::info!("persistent WebSocket connected to /api/v1/stream");
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        self.teardown_connection();
        self.state = TransportState::Disconnected;
        Ok(())
    }

    fn create_session(&mut self, session_id: uuid::Uuid) -> Result<(), TransportError> {
        if self.state() != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }
        self.command_log.push(Command::CreateSession);
        self.handle_create_session(session_id)
    }

    fn send_command(&mut self, command: &Command) -> Result<(), TransportError> {
        if self.state() != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        self.command_log.push(command.clone());

        match command {
            // Without an engine-supplied ID the session is keyed by a fresh
            // one, reported through `SessionCreated`.
            Command::CreateSession => self.handle_create_session(uuid::Uuid::new_v4()),
            Command::StartSession { session_id } => self.handle_start_session(session_id),
            Command::StopSession { session_id } => self.handle_stop_session(session_id),
            Command::InterruptPlayback { session_id } => {
                self.handle_interrupt_playback(session_id)
            }
            Command::SendText { session_id, text } => self.handle_send_text(session_id, text),
            Command::RefreshDeviceList => {
                self.event_queue
                    .push_back(Event::DeviceListUpdated { devices: vec![] });
                Ok(())
            }
            Command::ExportDiagnostics => {
                self.event_queue.push_back(Event::DiagnosticsExported {
                    path: String::new(),
                });
                Ok(())
            }
            other => {
                tracing::debug!("PrxVoiceTransport ignoring command: {other:?}");
                Ok(())
            }
        }
    }

    fn send_audio_data(&mut self, session_id: &str, data: &[u8]) -> Result<(), TransportError> {
        if self.state() != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        let engine_id = uuid::Uuid::parse_str(session_id).map_err(|_| {
            TransportError::SendFailed(format!("'{session_id}' is not an engine session id"))
        })?;
        let server_uuid = {
            let conn = self.connection()?;
            let sessions = conn.shared.sessions.lock();
            sessions
                .get(&engine_id)
                .map(|e| e.server_uuid)
                .ok_or_else(|| {
                    TransportError::SendFailed(format!("unknown session {engine_id}"))
                })?
        };

        // Binary frame: [36-byte hyphenated session UUID][PCM16 data]
        let mut payload = Vec::with_capacity(FRAME_PREFIX_LEN + data.len());
        payload.extend_from_slice(server_uuid.hyphenated().to_string().as_bytes());
        payload.extend_from_slice(data);

        self.connection()?
            .outgoing
            .send(WsOutgoing::Binary(payload))
            .map_err(|e| {
                TransportError::SendFailed(format!("WebSocket writer channel closed: {e}"))
            })
    }

    fn poll_event(&mut self) -> Result<Option<Event>, TransportError> {
        // First drain locally queued events.
        if let Some(event) = self.event_queue.pop_front() {
            return Ok(Some(event));
        }

        let Some(conn) = &self.conn else {
            return Ok(None);
        };
        let polled = conn.events.lock().try_recv();
        let alive = conn.shared.alive.load(Ordering::Acquire);
        match polled {
            Ok(event) => return Ok(Some(event)),
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => {}
        }
        if !alive && self.state == TransportState::Connected {
            tracing::warn!("WebSocket connection lost");
            self.state = TransportState::Disconnected;
        }
        Ok(None)
    }

    fn poll_audio(&mut self) -> Option<TtsAudioChunk> {
        self.conn.as_ref()?.audio.lock().try_recv().ok()
    }

    fn state(&self) -> TransportState {
        match (&self.state, &self.conn) {
            // A closed socket is a disconnection even if nobody polled yet.
            (TransportState::Connected, Some(conn))
                if !conn.shared.alive.load(Ordering::Acquire) =>
            {
                TransportState::Disconnected
            }
            _ => self.state.clone(),
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prx_voice_config_default() {
        let cfg = PrxVoiceConfig::default();
        assert_eq!(cfg.base_url, "http://localhost:3000");
        assert_eq!(cfg.language, "zh-CN");
        assert_eq!(cfg.asr_provider, "sherpa");
        assert_eq!(cfg.agent_provider, "ollama");
        assert_eq!(cfg.tts_provider, "sherpa");
        assert_eq!(cfg.channel, "app_sdk");
        assert!(!cfg.client_id.is_empty());
    }

    #[test]
    fn test_prx_voice_transport_new_is_disconnected() {
        let transport = PrxVoiceTransport::new(PrxVoiceConfig::default());
        assert_eq!(transport.state(), TransportState::Disconnected);
        assert!(transport.server_sid(&uuid::Uuid::nil()).is_none());
        assert!(transport.command_log().is_empty());
    }

    #[test]
    fn test_prx_voice_transport_connect_to_unreachable() {
        // Connect to an unreachable port (not 3000 which may have a real server).
        let cfg = PrxVoiceConfig {
            base_url: "http://127.0.0.1:19999".into(),
            ..PrxVoiceConfig::default()
        };
        let mut transport = PrxVoiceTransport::new(cfg);
        let result = transport.connect();
        assert!(result.is_err(), "expected connect to fail on unreachable port");
        assert_eq!(transport.state(), TransportState::Disconnected);
    }

    #[test]
    fn test_prx_voice_transport_connect_fails_invalid_url() {
        let cfg = PrxVoiceConfig {
            base_url: "not-a-valid-url".into(),
            ..PrxVoiceConfig::default()
        };
        let mut transport = PrxVoiceTransport::new(cfg);
        let result = transport.connect();
        assert!(result.is_err());
        assert_eq!(transport.state(), TransportState::Disconnected);
    }

    #[test]
    fn test_prx_voice_transport_disconnect_is_idempotent() {
        let mut transport = PrxVoiceTransport::new(PrxVoiceConfig::default());
        let result = transport.disconnect();
        assert!(result.is_ok());
        assert_eq!(transport.state(), TransportState::Disconnected);

        let result = transport.disconnect();
        assert!(result.is_ok());
    }

    #[test]
    fn test_prx_voice_transport_send_command_fails_when_disconnected() {
        let mut transport = PrxVoiceTransport::new(PrxVoiceConfig::default());
        let result = transport.send_command(&Command::CreateSession);
        assert!(result.is_err());
    }

    #[test]
    fn test_prx_voice_transport_send_audio_fails_when_disconnected() {
        let mut transport = PrxVoiceTransport::new(PrxVoiceConfig::default());
        let result = transport.send_audio_data("some-session", &[0u8; 16]);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_server_status_messages() {
        let cases = vec![
            ("recording", "UserSpeaking"),
            ("thinking", "Thinking"),
            ("listening", "Listening"),
            ("interrupted", "Interrupted"),
            ("closed", "Completed"),
        ];

        for (status, expected_to) in cases {
            let json = serde_json::json!({"type": "status", "status": status});
            let event = map_server_message("status", &json);
            assert!(event.is_some(), "expected event for status={status}");
            if let Some(Event::SessionStateChanged { to, .. }) = event {
                assert_eq!(to, expected_to, "status={status}");
            } else {
                panic!("expected SessionStateChanged for status={status}");
            }
        }
    }

    #[test]
    fn test_map_server_transcript() {
        let json = serde_json::json!({"type": "transcript", "text": "hello world"});
        let event = map_server_message("transcript", &json);
        assert!(matches!(
            event,
            Some(Event::InterimTranscriptProduced { text, .. }) if text == "hello world"
        ));
    }

    #[test]
    fn test_map_server_token() {
        let json = serde_json::json!({"type": "token", "token": "word", "cumulative": "full word"});
        let event = map_server_message("token", &json);
        assert!(matches!(
            event,
            Some(Event::AssistantReplyTextDelta { delta, .. }) if delta == "word"
        ));
    }

    #[test]
    fn test_map_server_error() {
        let json = serde_json::json!({"type": "error", "message": "something broke"});
        let event = map_server_message("error", &json);
        assert!(matches!(
            event,
            Some(Event::ErrorOccurred { code, message, recoverable: true })
                if code == "server.error" && message == "something broke"
        ));
    }

    #[test]
    fn test_map_server_tts_start_end() {
        let json_start = serde_json::json!({"type": "tts_start", "sample_rate": 16000});
        let event = map_server_message("tts_start", &json_start);
        assert!(matches!(event, Some(Event::AssistantReplyStarted { .. })));

        let json_end = serde_json::json!({"type": "tts_end"});
        let event = map_server_message("tts_end", &json_end);
        assert!(matches!(event, Some(Event::PlaybackCompleted { .. })));
    }

    #[test]
    fn test_map_server_ignored_messages() {
        let json = serde_json::json!({"type": "heartbeat"});
        assert!(map_server_message("heartbeat", &json).is_none());

        let json = serde_json::json!({"type": "no_speech"});
        assert!(map_server_message("no_speech", &json).is_none());

        let json = serde_json::json!({"type": "unknown_type"});
        assert!(map_server_message("unknown_type", &json).is_none());
    }

    #[test]
    fn test_map_server_response() {
        let json = serde_json::json!({
            "type": "response",
            "user_text": "hi",
            "agent_text": "hello there"
        });
        let event = map_server_message("response", &json);
        assert!(matches!(
            event,
            Some(Event::AssistantReplyTextDelta { delta, .. }) if delta == "hello there"
        ));
    }

    #[test]
    fn test_to_ws_url() {
        assert_eq!(
            PrxVoiceTransport::to_ws_url("http://localhost:3000"),
            "ws://localhost:3000"
        );
        assert_eq!(
            PrxVoiceTransport::to_ws_url("https://example.com"),
            "wss://example.com"
        );
        assert_eq!(
            PrxVoiceTransport::to_ws_url("plain-host:8080"),
            "ws://plain-host:8080"
        );
    }

    #[test]
    fn test_patch_event_session_id() {
        let sid = uuid::Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
        let mut event = Event::InterimTranscriptProduced {
            session_id: uuid::Uuid::nil(),
            text: "test".into(),
        };
        patch_event_session_id(&mut event, sid);
        if let Event::InterimTranscriptProduced { session_id, .. } = event {
            assert_eq!(session_id, sid);
        }
    }
}
