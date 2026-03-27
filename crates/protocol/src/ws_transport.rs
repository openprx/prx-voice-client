//! WebSocket-based transport for real network communication.
//!
//! Provides [`WsTransport`], a [`Transport`](crate::transport::Transport)
//! implementation that connects to a WebSocket server and exchanges
//! JSON-serialized commands and events.

use std::collections::VecDeque;

use futures::{SinkExt, StreamExt};

use crate::transport::{Transport, TransportError, TransportState};
use crate::{Command, Event};

/// Handle for sending messages to the background WebSocket writer task.
struct WsWriter {
    /// Channel sender for outgoing text messages.
    tx: std::sync::mpsc::Sender<WsOutgoing>,
}

/// Outgoing message types for the WebSocket writer.
enum WsOutgoing {
    /// A text (JSON) message.
    Text(String),
    /// A binary message (session_id prefix + audio data).
    Binary(Vec<u8>),
}

/// WebSocket-based transport for real network communication.
///
/// Commands are serialized as JSON and sent over the WebSocket connection.
/// Events are received as JSON messages and deserialized. The transport
/// bridges the synchronous [`Transport`] trait with an internal tokio
/// runtime that manages the asynchronous WebSocket connection.
///
/// # Reconnection
///
/// The transport supports reconnection with exponential backoff via
/// [`attempt_reconnect`](Self::attempt_reconnect). The maximum number
/// of attempts and base delay are configurable.
pub struct WsTransport {
    /// WebSocket server URL.
    url: String,
    /// Current connection state.
    state: TransportState,
    /// Pending events waiting to be polled.
    event_queue: VecDeque<Event>,
    /// History of all commands sent through this transport.
    command_log: Vec<Command>,
    /// Current reconnection attempt counter.
    reconnect_attempts: u32,
    /// Maximum reconnection attempts before entering the `Failed` state.
    max_reconnect_attempts: u32,
    /// Base delay in milliseconds for reconnection backoff.
    reconnect_delay_ms: u64,
    /// Internal tokio runtime for async WebSocket operations.
    runtime: Option<tokio::runtime::Runtime>,
    /// Writer handle for sending messages to the background task.
    ws_writer: Option<WsWriter>,
    /// Receiver end of the event channel from the background reader task.
    /// Wrapped in `parking_lot::Mutex` to satisfy the `Sync` bound on `Transport`.
    event_receiver: Option<parking_lot::Mutex<std::sync::mpsc::Receiver<Event>>>,
}

impl WsTransport {
    /// Create a new `WsTransport` targeting the given WebSocket URL.
    ///
    /// The transport starts in the [`Disconnected`](TransportState::Disconnected)
    /// state. Call [`connect`](Transport::connect) to establish the connection.
    ///
    /// Uses default reconnection settings: 5 max attempts, 1000ms base delay.
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            state: TransportState::Disconnected,
            event_queue: VecDeque::new(),
            command_log: Vec::new(),
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
            reconnect_delay_ms: 1000,
            runtime: None,
            ws_writer: None,
            event_receiver: None,
        }
    }

    /// Create a new `WsTransport` with custom reconnection configuration.
    ///
    /// # Arguments
    ///
    /// * `url` - WebSocket server URL (e.g. `"ws://localhost:8080"`)
    /// * `max_attempts` - Maximum reconnection attempts before failure
    /// * `delay_ms` - Base delay in milliseconds for exponential backoff
    pub fn with_config(url: &str, max_attempts: u32, delay_ms: u64) -> Self {
        Self {
            url: url.to_owned(),
            state: TransportState::Disconnected,
            event_queue: VecDeque::new(),
            command_log: Vec::new(),
            reconnect_attempts: 0,
            max_reconnect_attempts: max_attempts,
            reconnect_delay_ms: delay_ms,
            runtime: None,
            ws_writer: None,
            event_receiver: None,
        }
    }

    /// Attempt to reconnect with exponential backoff.
    ///
    /// Each call increments the reconnect counter and waits for an
    /// exponentially increasing delay before trying to connect again.
    /// If the maximum number of attempts is exceeded, the transport
    /// transitions to the [`Failed`](TransportState::Failed) state.
    pub fn attempt_reconnect(&mut self) -> Result<(), TransportError> {
        if self.reconnect_attempts >= self.max_reconnect_attempts {
            self.state = TransportState::Failed;
            return Err(TransportError::ConnectionFailed(format!(
                "max reconnect attempts ({}) exceeded",
                self.max_reconnect_attempts
            )));
        }

        self.state = TransportState::Reconnecting;
        self.reconnect_attempts += 1;

        let delay = self.reconnect_delay_ms * 2u64.pow(self.reconnect_attempts.min(5));
        std::thread::sleep(std::time::Duration::from_millis(delay));

        match self.connect() {
            Ok(()) => {
                self.reconnect_attempts = 0;
                self.state = TransportState::Connected;
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    "reconnect attempt {} failed: {}",
                    self.reconnect_attempts,
                    e
                );
                Err(e)
            }
        }
    }

    /// Reset the reconnection counter to zero.
    pub fn reset_reconnect_counter(&mut self) {
        self.reconnect_attempts = 0;
    }

    /// Get the current reconnection attempt count.
    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts
    }

    /// Get the URL this transport is configured to connect to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Return a reference to all commands sent through this transport.
    pub fn command_log(&self) -> &[Command] {
        &self.command_log
    }
}

impl Transport for WsTransport {
    fn connect(&mut self) -> Result<(), TransportError> {
        // Clean up any existing connection.
        self.runtime = None;
        self.ws_writer = None;
        self.event_receiver = None;

        self.state = TransportState::Connecting;

        // Validate URL before attempting connection.
        let parsed_url = url::Url::parse(&self.url).map_err(|e| {
            self.state = TransportState::Disconnected;
            TransportError::ConnectionFailed(format!("invalid URL '{}': {}", self.url, e))
        })?;

        // Create a new tokio runtime for background tasks.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .map_err(|e| {
                self.state = TransportState::Disconnected;
                TransportError::ConnectionFailed(format!("failed to create runtime: {e}"))
            })?;

        // Channels for bridging async <-> sync:
        // event_tx/event_rx: background reader -> poll_event()
        // outgoing_tx/outgoing_rx: send_command() -> background writer
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        let (outgoing_tx, outgoing_rx) = std::sync::mpsc::channel::<WsOutgoing>();

        // Attempt the WebSocket connection synchronously (block on it).
        let connect_result: Result<(), TransportError> = rt.block_on(async {
            use tokio_tungstenite::tungstenite::Message;

            let (ws_stream, _response) = tokio_tungstenite::connect_async(parsed_url.as_str())
                .await
                .map_err(|e| {
                    TransportError::ConnectionFailed(format!("WebSocket connection failed: {e}"))
                })?;

            let (write_half, read_half) = ws_stream.split();

            // Spawn the reader task.
            let event_tx_clone = event_tx.clone();
            tokio::spawn(async move {
                let mut read_half = read_half;
                while let Some(msg_result) = read_half.next().await {
                    match msg_result {
                        Ok(Message::Text(text)) => {
                            match serde_json::from_str::<Event>(&text) {
                                Ok(event) => {
                                    if event_tx_clone.send(event).is_err() {
                                        // Receiver dropped; stop reading.
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("failed to deserialize event: {e}");
                                    let err_event = Event::ErrorOccurred {
                                        code: "protocol.deserialize_error".to_owned(),
                                        message: format!("failed to deserialize event: {e}"),
                                        recoverable: true,
                                    };
                                    if event_tx_clone.send(err_event).is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            let close_event = Event::ErrorOccurred {
                                code: "transport.closed".to_owned(),
                                message: "WebSocket connection closed by server".to_owned(),
                                recoverable: true,
                            };
                            let _ = event_tx_clone.send(close_event);
                            break;
                        }
                        Ok(_) => {
                            // Ignore ping/pong/binary messages from server.
                        }
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

            // Spawn the writer task.
            tokio::spawn(async move {
                let mut write_half = write_half;
                // Read outgoing messages from the sync channel.
                // We use a blocking recv in a spawn_blocking-friendly way:
                // wrap in a tokio blocking task.
                loop {
                    let msg = {
                        let rx = &outgoing_rx;
                        // Non-blocking check first, then short sleep to avoid busy-wait.
                        match rx.try_recv() {
                            Ok(m) => m,
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                                continue;
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                break;
                            }
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

            Ok(())
        });

        if let Err(e) = connect_result {
            self.state = TransportState::Disconnected;
            return Err(e);
        }

        self.runtime = Some(rt);
        self.ws_writer = Some(WsWriter { tx: outgoing_tx });
        self.event_receiver = Some(parking_lot::Mutex::new(event_rx));
        self.state = TransportState::Connected;

        // Push AppReady event so callers know the connection is established.
        self.event_queue.push_back(Event::AppReady);

        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        // Drop the writer first to signal the background tasks.
        self.ws_writer = None;
        self.event_receiver = None;
        // Dropping the runtime stops all background tasks.
        self.runtime = None;
        self.state = TransportState::Disconnected;
        Ok(())
    }

    fn send_command(&mut self, command: &Command) -> Result<(), TransportError> {
        if self.state != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        let json = serde_json::to_string(command)
            .map_err(|e| TransportError::SendFailed(format!("failed to serialize command: {e}")))?;

        if let Some(writer) = &self.ws_writer {
            writer.tx.send(WsOutgoing::Text(json)).map_err(|e| {
                self.state = TransportState::Disconnected;
                TransportError::SendFailed(format!("writer channel closed: {e}"))
            })?;
        } else {
            return Err(TransportError::SendFailed(
                "no active WebSocket writer".to_owned(),
            ));
        }

        self.command_log.push(command.clone());
        Ok(())
    }

    fn send_audio_data(&mut self, session_id: &str, data: &[u8]) -> Result<(), TransportError> {
        if self.state != TransportState::Connected {
            return Err(TransportError::SendFailed(
                "transport is not connected".to_owned(),
            ));
        }

        // Binary frame format: session_id bytes (UTF-8) + 0x00 separator + audio data.
        let mut payload = Vec::with_capacity(session_id.len() + 1 + data.len());
        payload.extend_from_slice(session_id.as_bytes());
        payload.push(0x00);
        payload.extend_from_slice(data);

        if let Some(writer) = &self.ws_writer {
            writer.tx.send(WsOutgoing::Binary(payload)).map_err(|e| {
                self.state = TransportState::Disconnected;
                TransportError::SendFailed(format!("writer channel closed: {e}"))
            })?;
        } else {
            return Err(TransportError::SendFailed(
                "no active WebSocket writer".to_owned(),
            ));
        }

        Ok(())
    }

    fn poll_event(&mut self) -> Result<Option<Event>, TransportError> {
        // First drain any locally queued events (e.g. AppReady).
        if let Some(event) = self.event_queue.pop_front() {
            return Ok(Some(event));
        }

        // Then try the channel from the background reader.
        if let Some(rx_mutex) = &self.event_receiver {
            let rx = rx_mutex.lock();
            match rx.try_recv() {
                Ok(event) => {
                    // If we get a transport error event, update state accordingly.
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
                    Ok(Some(event))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // The background reader task has stopped.
                    if self.state == TransportState::Connected {
                        self.state = TransportState::Disconnected;
                    }
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
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
    fn test_ws_transport_new_is_disconnected() {
        let transport = WsTransport::new("ws://localhost:9999");
        assert_eq!(transport.state(), TransportState::Disconnected);
        assert_eq!(transport.url(), "ws://localhost:9999");
        assert_eq!(transport.reconnect_attempts(), 0);
        assert!(transport.command_log().is_empty());
    }

    #[test]
    fn test_ws_transport_with_config() {
        let transport = WsTransport::with_config("ws://example.com/ws", 10, 500);
        assert_eq!(transport.state(), TransportState::Disconnected);
        assert_eq!(transport.url(), "ws://example.com/ws");
        assert_eq!(transport.max_reconnect_attempts, 10);
        assert_eq!(transport.reconnect_delay_ms, 500);
    }

    #[test]
    fn test_ws_transport_connect_fails_with_invalid_url() {
        let mut transport = WsTransport::new("not-a-valid-url");
        let result = transport.connect();
        assert!(result.is_err());
        assert_eq!(transport.state(), TransportState::Disconnected);
    }

    #[test]
    fn test_ws_transport_connect_fails_with_unreachable_server() {
        // Port 1 is almost certainly not running a WebSocket server.
        let mut transport = WsTransport::new("ws://127.0.0.1:1/ws");
        let result = transport.connect();
        assert!(result.is_err());
        // State should not remain as Connecting after a failed attempt.
        assert_ne!(transport.state(), TransportState::Connecting);
    }

    #[test]
    fn test_ws_transport_send_command_fails_when_disconnected() {
        let mut transport = WsTransport::new("ws://localhost:9999");
        let result = transport.send_command(&Command::InitializeApp);
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_transport_send_audio_fails_when_disconnected() {
        let mut transport = WsTransport::new("ws://localhost:9999");
        let result = transport.send_audio_data("some-session", &[0u8; 16]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_transport_poll_event_returns_none_when_disconnected() {
        let mut transport = WsTransport::new("ws://localhost:9999");
        let result = transport.poll_event().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_ws_transport_reconnect_increments_counter() {
        // Use an unreachable address so connect always fails.
        let mut transport = WsTransport::with_config("ws://127.0.0.1:1/ws", 3, 1);
        assert_eq!(transport.reconnect_attempts(), 0);

        // First attempt should fail but increment counter.
        let _ = transport.attempt_reconnect();
        assert_eq!(transport.reconnect_attempts(), 1);

        let _ = transport.attempt_reconnect();
        assert_eq!(transport.reconnect_attempts(), 2);
    }

    #[test]
    fn test_ws_transport_max_reconnect_attempts() {
        let mut transport = WsTransport::with_config("ws://127.0.0.1:1/ws", 2, 1);

        // Exhaust reconnection attempts.
        let _ = transport.attempt_reconnect();
        let _ = transport.attempt_reconnect();

        // Next attempt should fail with max exceeded.
        let result = transport.attempt_reconnect();
        assert!(result.is_err());
        assert_eq!(transport.state(), TransportState::Failed);
    }

    #[test]
    fn test_ws_transport_state_transitions() {
        let mut transport = WsTransport::new("ws://127.0.0.1:1/ws");

        // Initial state.
        assert_eq!(transport.state(), TransportState::Disconnected);

        // Failed connect should leave us not in Connecting.
        let _ = transport.connect();
        assert_ne!(transport.state(), TransportState::Connecting);

        // Disconnect should bring us back to Disconnected.
        let _ = transport.disconnect();
        assert_eq!(transport.state(), TransportState::Disconnected);
    }

    #[test]
    fn test_ws_transport_disconnect_is_idempotent() {
        let mut transport = WsTransport::new("ws://localhost:9999");
        // Disconnect when already disconnected should succeed.
        let result = transport.disconnect();
        assert!(result.is_ok());
        assert_eq!(transport.state(), TransportState::Disconnected);

        // Double disconnect should also succeed.
        let result = transport.disconnect();
        assert!(result.is_ok());
        assert_eq!(transport.state(), TransportState::Disconnected);
    }

    #[test]
    fn test_ws_transport_reset_reconnect_counter() {
        let mut transport = WsTransport::with_config("ws://127.0.0.1:1/ws", 5, 1);
        let _ = transport.attempt_reconnect();
        assert!(transport.reconnect_attempts() > 0);

        transport.reset_reconnect_counter();
        assert_eq!(transport.reconnect_attempts(), 0);
    }
}
