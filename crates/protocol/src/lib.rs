//! Command/event contract definitions for the prx-voice-client system.
//!
//! This crate defines the shared protocol layer: all commands the UI can send
//! and all events the backend can emit, plus supporting types for device info,
//! error codes, and contract versioning.

#[cfg(any(test, feature = "test-double"))]
pub mod mock;
pub mod transport;

use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Contract version
// ---------------------------------------------------------------------------

/// Protocol contract version, used for forward-compatibility checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractVersion {
    /// The numeric version of this contract.
    pub version: u32,
}

impl ContractVersion {
    /// Current protocol contract version.
    pub const CURRENT: ContractVersion = ContractVersion { version: 1 };
}

// ---------------------------------------------------------------------------
// Device types
// ---------------------------------------------------------------------------

/// Kind of audio device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    /// Audio input (microphone).
    Input,
    /// Audio output (speaker / headphone).
    Output,
}

/// Metadata describing an audio device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Platform-specific device identifier.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Whether this device is an input or output.
    pub kind: DeviceKind,
    /// Whether the OS reports this device as the default for its kind.
    pub is_default: bool,
}

// ---------------------------------------------------------------------------
// Error code
// ---------------------------------------------------------------------------

/// Structured error code with domain prefix (e.g. `"audio.device_lost"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorCode {
    /// Error domain (e.g. `"audio"`, `"session"`, `"protocol"`).
    pub domain: String,
    /// Specific error code within the domain (e.g. `"device_lost"`).
    pub code: String,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.domain, self.code)
    }
}

// ---------------------------------------------------------------------------
// Commands (UI -> backend)
// ---------------------------------------------------------------------------

/// Commands that can be dispatched from the UI layer to the backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Initialize the application subsystems.
    InitializeApp,
    /// Gracefully shut down the application.
    ShutdownApp,
    /// Create a new voice session (returns a session id via event).
    CreateSession,
    /// Start an existing session.
    StartSession {
        /// The session to start.
        session_id: uuid::Uuid,
    },
    /// Stop an existing session.
    StopSession {
        /// The session to stop.
        session_id: uuid::Uuid,
    },
    /// Interrupt the assistant playback in a session.
    InterruptPlayback {
        /// The target session.
        session_id: uuid::Uuid,
    },
    /// Pause a running session.
    PauseSession {
        /// The target session.
        session_id: uuid::Uuid,
    },
    /// Resume a paused session.
    ResumeSession {
        /// The target session.
        session_id: uuid::Uuid,
    },
    /// Switch the active audio input device.
    SwitchInputDevice {
        /// Platform device identifier.
        device_id: String,
    },
    /// Switch the active audio output device.
    SwitchOutputDevice {
        /// Platform device identifier.
        device_id: String,
    },
    /// Refresh the list of available audio devices.
    RefreshDeviceList,
    /// Export a diagnostics bundle.
    ExportDiagnostics,
    /// Acknowledge (dismiss) an error by its id.
    AcknowledgeError {
        /// The identifier of the error to acknowledge.
        error_id: String,
    },
    /// Send recognized text to the server (from local ASR).
    ///
    /// This skips server-side ASR and goes directly to the Agent pipeline.
    SendText {
        /// The target session.
        session_id: uuid::Uuid,
        /// The recognized text to send.
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Events (backend -> UI)
// ---------------------------------------------------------------------------

/// Events emitted by the backend for consumption by the UI layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// Application initialization completed successfully.
    AppReady,
    /// A new session was created.
    SessionCreated {
        /// The newly created session id.
        session_id: uuid::Uuid,
    },
    /// A session has started listening.
    SessionStarted {
        /// The started session id.
        session_id: uuid::Uuid,
    },
    /// A session transitioned between states.
    SessionStateChanged {
        /// The session whose state changed.
        session_id: uuid::Uuid,
        /// The state the session was in before the transition.
        from: String,
        /// The state the session transitioned to.
        to: String,
    },
    /// An interim (non-final) transcript fragment was produced.
    InterimTranscriptProduced {
        /// The session that produced the transcript.
        session_id: uuid::Uuid,
        /// The interim transcript text.
        text: String,
    },
    /// A final transcript was produced.
    FinalTranscriptProduced {
        /// The session that produced the transcript.
        session_id: uuid::Uuid,
        /// The finalized transcript text.
        text: String,
    },
    /// The assistant began producing a reply.
    AssistantReplyStarted {
        /// The session the reply belongs to.
        session_id: uuid::Uuid,
    },
    /// A streaming text delta from the assistant reply.
    AssistantReplyTextDelta {
        /// The session the delta belongs to.
        session_id: uuid::Uuid,
        /// The incremental text chunk.
        delta: String,
    },
    /// Audio playback of the assistant reply completed.
    PlaybackCompleted {
        /// The session whose playback finished.
        session_id: uuid::Uuid,
    },
    /// Audio playback was interrupted by the user.
    PlaybackInterrupted {
        /// The session whose playback was interrupted.
        session_id: uuid::Uuid,
    },
    /// The available device list was refreshed.
    DeviceListUpdated {
        /// Current list of available devices.
        devices: Vec<DeviceInfo>,
    },
    /// An error occurred in the backend.
    ErrorOccurred {
        /// Machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
        /// Whether the system can recover automatically.
        recoverable: bool,
    },
    /// A diagnostics bundle was exported.
    DiagnosticsExported {
        /// File-system path to the exported bundle.
        path: String,
    },
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for downstream crates.
pub mod prelude {
    #[cfg(any(test, feature = "test-double"))]
    pub use crate::mock::MockGateway;
    pub use crate::transport::{Transport, TransportError, TransportState, TtsAudioChunk};
    pub use crate::{Command, ContractVersion, DeviceInfo, DeviceKind, ErrorCode, Event};
}
