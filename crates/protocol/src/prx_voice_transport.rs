//! Persistent-connection transport for the prx-voice server.
//!
//! [`PrxVoiceTransport`] implements [`Transport`](crate::transport::Transport)
//! using a single long-lived WebSocket to `/api/v1/stream`. Sessions are
//! created and closed as lightweight JSON messages on the existing connection,
//! eliminating per-conversation handshake latency.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use futures::{SinkExt, StreamExt};

use crate::transport::{Transport, TransportError, TransportState};
use crate::{Command, Event};

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
// Per-session client-side state
// ---------------------------------------------------------------------------

/// Tracks a session on the client side.
struct ClientSession {
    /// Server-assigned session ID string (e.g., "sess-{uuid}").
    server_sid: String,
    /// Raw UUID hex (36 chars) for binary frame prefix.
    uuid_hex: String,
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
// Transport
// ---------------------------------------------------------------------------

/// Transport that maintains a persistent WebSocket to a prx-voice server.
///
/// Lifecycle:
/// 1. `connect()` — Establish persistent WebSocket to `/api/v1/stream`,
///    start heartbeat and reader tasks.
/// 2. `send_command(CreateSession)` — Send JSON over existing WS.
/// 3. `send_command(StartSession)` — Send `audio_start` over existing WS.
/// 4. `send_audio_data()` — Binary frames with 36-byte UUID prefix.
/// 5. `send_command(StopSession)` — Send `close_session`, WS stays open.
/// 6. `disconnect()` — Tear down the persistent connection.
pub struct PrxVoiceTransport {
    /// Server configuration.
    config: PrxVoiceConfig,
    /// Current connection state.
    state: TransportState,
    /// Pending events waiting to be polled.
    event_queue: VecDeque<Event>,
    /// History of all commands sent through this transport.
    command_log: Vec<Command>,

    // -- Connection layer (app-lifetime) --
    /// Owned tokio runtime.
    _owned_runtime: Option<tokio::runtime::Runtime>,
    /// Handle to the tokio runtime.
    runtime_handle: Option<tokio::runtime::Handle>,
    /// Channel sender for outgoing WebSocket messages.
    ws_writer: Option<std::sync::mpsc::Sender<WsOutgoing>>,
    /// Event receiver channel (from WebSocket reader task).
    event_receiver: Option<parking_lot::Mutex<std::sync::mpsc::Receiver<Event>>>,
    /// TTS audio receiver (binary PCM16 frames from server).
    audio_receiver: Option<parking_lot::Mutex<std::sync::mpsc::Receiver<Vec<u8>>>>,
    /// Timestamp of last pong received from server.
    last_pong: Arc<parking_lot::Mutex<Instant>>,

    // -- Session layer --
    /// Active sessions: local UUID → client session state.
    active_sessions: HashMap<uuid::Uuid, ClientSession>,
    /// Pending create requests: req_id → local UUID.
    /// Shared with the reader task so it can correlate session_created responses.
    pending_creates: Arc<parking_lot::Mutex<HashMap<String, uuid::Uuid>>>,
    /// The most recently created session ID (for backward compat with Engine).
    current_session_id: Option<uuid::Uuid>,

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
            _owned_runtime: None,
            runtime_handle: None,
            ws_writer: None,
            event_receiver: None,
            audio_receiver: None,
            last_pong: Arc::new(parking_lot::Mutex::new(Instant::now())),
            active_sessions: HashMap::new(),
            pending_creates: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            current_session_id: None,
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
        }
    }

    /// Get the current transport state.
    pub fn transport_state(&self) -> &TransportState {
        &self.state
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

    /// Get the current (most recent) session ID, if any.
    pub fn current_session_id(&self) -> Option<&str> {
        // Return the server-format sid for the current session.
        self.current_session_id.and_then(|uuid| {
            self.active_sessions
                .get(&uuid)
                .map(|s| s.server_sid.as_str())
        })
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

    /// Handle the `CreateSession` command — send JSON over existing WS and
    /// synchronously wait for the `session_created` response (up to 5s).
    ///
    /// This must be synchronous because the Engine calls `StartSession`
    /// immediately after `CreateSession`, and `StartSession` needs the
    /// session to already be registered in `active_sessions`.
    fn handle_create_session(&mut self) -> Result<(), TransportError> {
        let req_id = uuid::Uuid::new_v4().to_string();
        let local_uuid = uuid::Uuid::new_v4();

        // Insert into the shared pending_creates map so the reader task
        // can correlate the session_created response by req_id.
        self.pending_creates.lock().insert(req_id.clone(), local_uuid);

        let msg = serde_json::json!({
            "type": "create_session",
            "req_id": req_id,
            "channel": self.config.channel,
            "language": self.config.language,
            "asr_providers": [self.config.asr_provider],
            "agent_providers": [self.config.agent_provider],
            "tts_providers": [self.config.tts_provider],
        });

        self.send_ws_text(&msg.to_string())?;
        tracing::info!(%req_id, "create_session sent over persistent WS");

        // Synchronously wait for the session_created response from the reader task.
        // The reader sends SessionCreated events; we poll until we find ours.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let rx_mutex = self.event_receiver.as_ref().ok_or_else(|| {
            TransportError::ReceiveFailed("no event receiver".into())
        })?;

        loop {
            if std::time::Instant::now() > deadline {
                return Err(TransportError::Timeout(5000));
            }

            let rx = rx_mutex.lock();
            match rx.try_recv() {
                Ok(Event::SessionCreated { session_id }) => {
                    drop(rx); // Release lock before mutating self.
                    let server_sid = format!("sess-{session_id}");
                    let uuid_hex = session_id.to_string();
                    tracing::info!(%session_id, %server_sid, "session created on persistent WS");
                    self.active_sessions.insert(session_id, ClientSession {
                        server_sid,
                        uuid_hex,
                    });
                    self.current_session_id = Some(session_id);
                    self.event_queue.push_back(Event::SessionCreated { session_id });
                    return Ok(());
                }
                Ok(Event::ErrorOccurred { code, message, recoverable }) => {
                    drop(rx);
                    tracing::error!(%code, %message, "session creation failed on server");
                    self.event_queue.push_back(Event::ErrorOccurred {
                        code: code.clone(),
                        message: message.clone(),
                        recoverable,
                    });
                    return Err(TransportError::ReceiveFailed(
                        format!("session creation failed: {message}"),
                    ));
                }
                Ok(other_event) => {
                    drop(rx);
                    // Queue non-create events for later poll_event calls.
                    self.event_queue.push_back(other_event);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    drop(rx);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(TransportError::ReceiveFailed(
                        "event channel disconnected while waiting for session_created".into(),
                    ));
                }
            }
        }
    }

    /// Resolve a session UUID to a ClientSession.
    ///
    /// The Engine's session UUID (from SessionManager) may differ from the
    /// transport's session UUID (from the server). When an exact match is not
    /// found, fall back to the most recently created session.
    fn resolve_session(&self, session_id: &uuid::Uuid) -> Option<&ClientSession> {
        self.active_sessions
            .get(session_id)
            .or_else(|| {
                // Fallback: the Engine may use a different UUID than the server assigned.
                self.current_session_id
                    .as_ref()
                    .and_then(|id| self.active_sessions.get(id))
            })
    }

    /// Handle the `StartSession` command — send `audio_start`.
    fn handle_start_session(
        &mut self,
        session_id: &uuid::Uuid,
    ) -> Result<(), TransportError> {
        let session = self
            .resolve_session(session_id)
            .ok_or_else(|| {
                TransportError::SendFailed("session not found — create a session first".into())
            })?;

        let msg = serde_json::json!({
            "type": "audio_start",
            "sid": session.server_sid,
        });
        self.send_ws_text(&msg.to_string())?;

        self.event_queue
            .push_back(Event::SessionStarted { session_id: *session_id });
        Ok(())
    }

    /// Handle the `StopSession` command — close session, keep WS alive.
    fn handle_stop_session(
        &mut self,
        session_id: &uuid::Uuid,
    ) -> Result<(), TransportError> {
        let server_sid = self
            .resolve_session(session_id)
            .map(|s| s.server_sid.clone())
            .unwrap_or_default();

        if !server_sid.is_empty() {
            let msg = serde_json::json!({
                "type": "close_session",
                "sid": server_sid,
                "reason": "user_ended",
            });
            let _ = self.send_ws_text(&msg.to_string());
        }

        // Clean up local session state — but keep the WebSocket open.
        self.active_sessions.remove(session_id);
        if self.current_session_id == Some(*session_id) {
            self.current_session_id = None;
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
        let server_sid = self
            .resolve_session(session_id)
            .map(|s| s.server_sid.clone())
            .unwrap_or_default();

        if !server_sid.is_empty() {
            let msg = serde_json::json!({
                "type": "interrupt",
                "sid": server_sid,
            });
            self.send_ws_text(&msg.to_string())?;
        }

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
        let server_sid = self
            .resolve_session(session_id)
            .map(|s| s.server_sid.clone())
            .unwrap_or_default();

        if server_sid.is_empty() {
            return Err(TransportError::SendFailed(
                "no active session for SendText".into(),
            ));
        }

        let msg = serde_json::json!({
            "type": "text",
            "sid": server_sid,
            "text": text,
        });
        self.send_ws_text(&msg.to_string())?;
        tracing::info!(%server_sid, text_len = text.len(), "sent text to server");
        Ok(())
    }

    /// Send a text message over the WebSocket channel.
    fn send_ws_text(&self, text: &str) -> Result<(), TransportError> {
        if let Some(writer) = &self.ws_writer {
            writer
                .send(WsOutgoing::Text(text.to_string()))
                .map_err(|e| {
                    TransportError::SendFailed(format!("WebSocket writer channel closed: {e}"))
                })?;
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "no active WebSocket writer".to_owned(),
            ))
        }
    }
}

/// Patch the `session_id` field in events that carry one.
fn patch_event_session_id(event: &mut Event, session_id: uuid::Uuid) {
    match event {
        Event::SessionCreated {
            session_id: sid,
        }
        | Event::SessionStarted {
            session_id: sid,
        }
        | Event::InterimTranscriptProduced {
            session_id: sid,
            ..
        }
        | Event::FinalTranscriptProduced {
            session_id: sid,
            ..
        }
        | Event::AssistantReplyStarted {
            session_id: sid,
        }
        | Event::AssistantReplyTextDelta {
            session_id: sid,
            ..
        }
        | Event::PlaybackCompleted {
            session_id: sid,
        }
        | Event::PlaybackInterrupted {
            session_id: sid,
        }
        | Event::SessionStateChanged {
            session_id: sid,
            ..
        } => {
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
        // Clean up any existing state.
        self.runtime_handle = None;
        self._owned_runtime = None;
        self.ws_writer = None;
        self.event_receiver = None;
        self.audio_receiver = None;

        self.state = TransportState::Connecting;

        // Validate base URL.
        let _parsed = url::Url::parse(&self.config.base_url).map_err(|e| {
            self.state = TransportState::Disconnected;
            TransportError::ConnectionFailed(format!(
                "invalid base URL '{}': {e}",
                self.config.base_url
            ))
        })?;

        // Create dedicated runtime.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("prxvc-transport")
            .build()
            .map_err(|e| {
                self.state = TransportState::Disconnected;
                TransportError::ConnectionFailed(format!("failed to create runtime: {e}"))
            })?;
        let handle = rt.handle().clone();
        self._owned_runtime = Some(rt);
        self.runtime_handle = Some(handle.clone());

        // Build WebSocket URL: /api/v1/stream
        let ws_base = Self::to_ws_url(&self.config.base_url);
        let ws_url = format!("{ws_base}/api/v1/stream");

        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (outgoing_tx, outgoing_rx) = std::sync::mpsc::channel::<WsOutgoing>();

        let last_pong = Arc::clone(&self.last_pong);
        // Share the same pending_creates map with the reader task.
        let pending_creates_ref = Arc::clone(&self.pending_creates);

        // Clone outgoing_tx before moving into the spawned thread — the
        // original stays here and is assigned to self.ws_writer.
        let outgoing_tx_for_ws = outgoing_tx.clone();

        // Connect WebSocket on a dedicated thread.
        let (ws_tx, ws_rx) = std::sync::mpsc::channel();
        let ws_handle = handle.clone();
        std::thread::spawn(move || {
            let result = ws_handle.block_on(async {
                use tokio_tungstenite::tungstenite::Message;

                let (ws_stream, _response) =
                    tokio_tungstenite::connect_async(&ws_url)
                        .await
                        .map_err(|e| {
                            TransportError::ConnectionFailed(format!(
                                "WebSocket connection to {ws_url} failed: {e}"
                            ))
                        })?;

                let (write_half, read_half) = ws_stream.split();

                // Spawn reader task.
                let event_tx_clone = event_tx.clone();
                let audio_tx_clone = audio_tx;
                let last_pong_clone = last_pong.clone();
                tokio::spawn(async move {
                    let mut read_half = read_half;
                    while let Some(msg_result) = read_half.next().await {
                        match msg_result {
                            Ok(Message::Text(text)) => {
                                let parsed: Result<serde_json::Value, _> =
                                    serde_json::from_str(&text);
                                match parsed {
                                    Ok(json) => {
                                        let msg_type =
                                            json["type"].as_str().unwrap_or_default().to_string();

                                        // Handle pong — update timestamp.
                                        if msg_type == "pong" {
                                            *last_pong_clone.lock() = Instant::now();
                                            continue;
                                        }

                                        // Handle ping from server — reply with pong.
                                        if msg_type == "ping" {
                                            // Pong is sent via the writer; we skip here.
                                            // The main loop handles sending pong.
                                            continue;
                                        }

                                        // Handle session_created — correlate with pending request.
                                        if msg_type == "session_created" {
                                            let req_id = json["req_id"]
                                                .as_str()
                                                .unwrap_or_default()
                                                .to_string();
                                            // Look up the local UUID for this req_id.
                                            let local_uuid = {
                                                pending_creates_ref.lock().remove(&req_id)
                                            };
                                            if let Some(local_uuid) = local_uuid {
                                                let event = Event::SessionCreated {
                                                    session_id: local_uuid,
                                                };
                                                if event_tx_clone.send(event).is_err() {
                                                    break;
                                                }
                                            }
                                            continue;
                                        }

                                        // Handle session_closed.
                                        if msg_type == "session_closed" {
                                            // Already handled by StopSession locally.
                                            continue;
                                        }

                                        // Map session-scoped messages.
                                        // Try to extract session UUID from "sid" field.
                                        let session_uuid = json["sid"]
                                            .as_str()
                                            .and_then(|s| {
                                                let hex = s.strip_prefix("sess-").unwrap_or(s);
                                                uuid::Uuid::parse_str(hex).ok()
                                            })
                                            .unwrap_or(uuid::Uuid::nil());

                                        if let Some(mut event) =
                                            map_server_message(&msg_type, &json)
                                        {
                                            patch_event_session_id(&mut event, session_uuid);
                                            if event_tx_clone.send(event).is_err() {
                                                break;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "failed to parse server message as JSON: {e}"
                                        );
                                        let err_event = Event::ErrorOccurred {
                                            code: "protocol.parse_error".to_owned(),
                                            message: format!(
                                                "failed to parse server message: {e}"
                                            ),
                                            recoverable: true,
                                        };
                                        if event_tx_clone.send(err_event).is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            Ok(Message::Binary(data)) => {
                                // Binary frames are TTS PCM16 audio.
                                if audio_tx_clone.send(data.to_vec()).is_err() {
                                    break;
                                }
                            }
                            Ok(Message::Close(_)) => {
                                let close_event = Event::ErrorOccurred {
                                    code: "transport.closed".to_owned(),
                                    message:
                                        "WebSocket connection closed by server".to_owned(),
                                    recoverable: true,
                                };
                                let _ = event_tx_clone.send(close_event);
                                break;
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::error!("WebSocket read error: {e}");
                                let err_event = Event::ErrorOccurred {
                                    code: "transport.read_error".to_owned(),
                                    message: format!("WebSocket read error: {e}"),
                                    recoverable: false,
                                };
                                let _ = event_tx_clone.send(err_event);
                                break;
                            }
                        }
                    }
                });

                // Spawn writer task.
                tokio::spawn(async move {
                    let mut write_half = write_half;
                    loop {
                        let msg = match outgoing_rx.try_recv() {
                            Ok(m) => m,
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                                continue;
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                break;
                            }
                        };

                        let ws_msg = match msg {
                            WsOutgoing::Text(text) => Message::Text(text.into()),
                            WsOutgoing::Binary(data) => Message::Binary(data.into()),
                        };

                        if let Err(e) = write_half.send(ws_msg).await {
                            tracing::error!("WebSocket write error: {e}");
                            break;
                        }
                    }
                });

                // Spawn heartbeat task (client sends ping every 10s).
                let ping_writer = event_tx.clone();
                let heartbeat_outgoing = outgoing_tx_for_ws.clone();
                tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10));
                    loop {
                        ticker.tick().await;
                        let ping = serde_json::json!({
                            "type": "ping",
                            "ts": chrono::Utc::now().to_rfc3339(),
                        });
                        if heartbeat_outgoing
                            .send(WsOutgoing::Text(ping.to_string()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    // If heartbeat can't send, connection is dead.
                    let _ = ping_writer.send(Event::ErrorOccurred {
                        code: "transport.heartbeat_failed".to_owned(),
                        message: "Heartbeat task stopped".to_owned(),
                        recoverable: true,
                    });
                });

                Ok(())
            });
            let _ = ws_tx.send(result);
        });

        let connect_result = ws_rx.recv()
            .map_err(|_| TransportError::ConnectionFailed("ws connect thread died".into()));
        match connect_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                self.state = TransportState::Disconnected;
                return Err(e);
            }
            Err(e) => {
                self.state = TransportState::Disconnected;
                return Err(e);
            }
        }

        self.ws_writer = Some(outgoing_tx);
        self.event_receiver = Some(parking_lot::Mutex::new(event_rx));
        self.audio_receiver = Some(parking_lot::Mutex::new(audio_rx));
        self.state = TransportState::Connected;
        self.reconnect_attempts = 0;

        // Store the pending_creates reference so reader can correlate.
        // (The reader already has a clone via pending_creates_ref.)

        // Also store the create_rx so poll_event can process correlations.
        // We'll drain it in poll_event.
        // Store it as a field... but we can't add new fields after construction.
        // Instead, we'll just check create_rx in each poll_event call.
        // Actually, let's just store it. We need a way to receive create correlations.
        // For simplicity, the reader task sends SessionCreated events directly,
        // and we handle the session registration in poll_event when we see SessionCreated.

        self.event_queue.push_back(Event::AppReady);

        tracing::info!("persistent WebSocket connected to /api/v1/stream");
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        self.ws_writer = None;
        self.event_receiver = None;
        self.audio_receiver = None;
        self.runtime_handle = None;
        self._owned_runtime = None;
        self.active_sessions.clear();
        self.pending_creates.lock().clear();
        self.current_session_id = None;
        self.state = TransportState::Disconnected;
        Ok(())
    }

    fn send_command(&mut self, command: &Command) -> Result<(), TransportError> {
        if self.state != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        self.command_log.push(command.clone());

        match command {
            Command::CreateSession => self.handle_create_session(),
            Command::StartSession { session_id } => self.handle_start_session(session_id),
            Command::StopSession { session_id } => self.handle_stop_session(session_id),
            Command::InterruptPlayback { session_id } => {
                self.handle_interrupt_playback(session_id)
            }
            Command::SendText { session_id, text } => {
                self.handle_send_text(session_id, text)
            }
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
        if self.state != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        // Find the session's UUID hex for the binary prefix.
        // session_id might be the Engine's UUID or server sid.
        let uuid_hex = if let Ok(uuid) = uuid::Uuid::parse_str(session_id) {
            if let Some(session) = self.resolve_session(&uuid) {
                session.uuid_hex.clone()
            } else {
                // Use the raw UUID hex as fallback.
                uuid.as_simple().to_string()
            }
        } else {
            // Try parsing as server sid.
            let hex = session_id.strip_prefix("sess-").unwrap_or(session_id);
            hex.replace('-', "")
        };

        // Build binary frame: [36-byte UUID hex][PCM16 data]
        // Note: UUID simple format is 32 chars, hyphenated is 36.
        // Server expects 36-byte (hyphenated) UUID hex.
        let uuid_str = if uuid_hex.len() == 32 {
            // Convert simple to hyphenated.
            format!(
                "{}-{}-{}-{}-{}",
                &uuid_hex[..8],
                &uuid_hex[8..12],
                &uuid_hex[12..16],
                &uuid_hex[16..20],
                &uuid_hex[20..32]
            )
        } else {
            uuid_hex
        };

        let mut payload = Vec::with_capacity(36 + data.len());
        payload.extend_from_slice(uuid_str.as_bytes());
        payload.extend_from_slice(data);

        if let Some(writer) = &self.ws_writer {
            writer
                .send(WsOutgoing::Binary(payload))
                .map_err(|e| {
                    TransportError::SendFailed(format!("WebSocket writer channel closed: {e}"))
                })?;
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "no active WebSocket writer — connect first".to_owned(),
            ))
        }
    }

    fn poll_event(&mut self) -> Result<Option<Event>, TransportError> {
        // First drain locally queued events.
        if let Some(event) = self.event_queue.pop_front() {
            return Ok(Some(event));
        }

        // Check the event channel from the WebSocket reader.
        if let Some(rx_mutex) = &self.event_receiver {
            let rx = rx_mutex.lock();
            match rx.try_recv() {
                Ok(event) => {
                    // SessionCreated is already handled synchronously in
                    // handle_create_session — no need to register here.

                    if let Event::ErrorOccurred {
                        ref code,
                        recoverable,
                        ..
                    } = event
                    {
                        if code == "transport.read_error" && !recoverable {
                            self.state = TransportState::Disconnected;
                        }
                    }
                    return Ok(Some(event));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.state == TransportState::Connected && self.ws_writer.is_some() {
                        tracing::warn!("WebSocket reader task disconnected");
                    }
                }
            }
        }

        // Drain TTS audio to avoid backpressure.
        if let Some(rx_mutex) = &self.audio_receiver {
            let rx = rx_mutex.lock();
            while rx.try_recv().is_ok() {}
        }

        Ok(None)
    }

    fn state(&self) -> TransportState {
        self.state.clone()
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
        assert!(transport.current_session_id().is_none());
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
