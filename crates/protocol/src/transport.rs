//! Transport abstraction for backend communication.
//!
//! Defines the [`Transport`] trait that all backend transports must implement,
//! along with error and state types.

use std::any::Any;

use crate::{Command, Event};

/// Transport-level errors.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The transport failed to establish a connection.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    /// The transport was unexpectedly disconnected.
    #[error("disconnected: {0}")]
    Disconnected(String),
    /// Sending a message over the transport failed.
    #[error("send failed: {0}")]
    SendFailed(String),
    /// Receiving a message over the transport failed.
    #[error("receive failed: {0}")]
    ReceiveFailed(String),
    /// The operation timed out after the given number of milliseconds.
    #[error("timeout after {0}ms")]
    Timeout(u64),
}

/// Represents the current state of a transport connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    /// Not connected to any backend.
    Disconnected,
    /// Connection attempt in progress.
    Connecting,
    /// Successfully connected and ready.
    Connected,
    /// Lost connection, attempting to reconnect.
    Reconnecting,
    /// Terminal failure state.
    Failed,
}

/// Async transport trait for communication with a backend.
///
/// Implementors provide the mechanism to send commands, receive events, and
/// stream audio data to and from a backend service.
pub trait Transport: Send + Sync {
    /// Connect to the backend.
    fn connect(&mut self) -> Result<(), TransportError>;

    /// Disconnect from the backend.
    fn disconnect(&mut self) -> Result<(), TransportError>;

    /// Send a command to the backend.
    fn send_command(&mut self, command: &Command) -> Result<(), TransportError>;

    /// Send raw audio frame data for a given session.
    fn send_audio_data(&mut self, session_id: &str, data: &[u8]) -> Result<(), TransportError>;

    /// Poll for the next event (non-blocking, returns `None` if no event available).
    fn poll_event(&mut self) -> Result<Option<Event>, TransportError>;

    /// Get the current transport state.
    fn state(&self) -> TransportState;

    /// Downcast to a concrete type for testing purposes.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
