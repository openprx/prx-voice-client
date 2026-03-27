//! FFI boundary layer for the prx-voice-client system.
//!
//! Provides JSON-based entry points intended for UniFFI integration with
//! iOS and Android host applications. All functions accept and return JSON
//! strings to minimise the surface area of the foreign-function interface.
//!
//! The module is backed by a real [`Engine`] instance that processes commands
//! through the session state machine and mock transport.

pub mod c_api;

use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use prxvc_core::PrxvcError;
use prxvc_core::bus::event_channel;
use prxvc_core::engine::Engine;
use prxvc_protocol::mock::MockGateway;
use prxvc_protocol::{Command, ContractVersion, Event};
use prxvc_session::SessionId;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors originating from the FFI boundary.
#[derive(Debug, thiserror::Error)]
pub enum FfiError {
    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    SerializationError(String),

    /// An argument passed across the FFI boundary was invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// An error propagated from a core subsystem.
    #[error("core error: {0}")]
    CoreError(String),

    /// The FFI layer has not been initialized.
    #[error("FFI layer not initialized")]
    NotInitialized,
}

impl From<PrxvcError> for FfiError {
    fn from(err: PrxvcError) -> Self {
        FfiError::CoreError(err.to_string())
    }
}

/// Convenience alias for FFI results.
pub type FfiResult<T> = Result<T, FfiError>;

// ---------------------------------------------------------------------------
// Data transfer types
// ---------------------------------------------------------------------------

/// Snapshot of a session's state, serializable across the FFI boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiSessionState {
    /// Unique session identifier.
    pub session_id: String,
    /// Current session state label.
    pub state: String,
    /// Number of conversational turns completed.
    pub turn_count: u64,
    /// ISO-8601 timestamp when the session was created.
    pub created_at: String,
}

/// An event payload suitable for passing across the FFI boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiEvent {
    /// Event type discriminator (e.g. `"session_created"`).
    pub event_type: String,
    /// JSON-encoded event payload.
    pub payload_json: String,
    /// ISO-8601 timestamp of the event.
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Global engine state
// ---------------------------------------------------------------------------

/// Handle wrapping the engine instance for FFI use.
struct EngineHandle {
    engine: Engine,
}

/// Global engine instance (initialized via [`ffi_initialize`]).
static ENGINE: LazyLock<Mutex<Option<EngineHandle>>> = LazyLock::new(|| Mutex::new(None));

/// Lock the global engine and return a guard, or error if not initialized.
fn with_engine<F, T>(f: F) -> Result<T, FfiError>
where
    F: FnOnce(&mut Engine) -> Result<T, FfiError>,
{
    let mut guard = ENGINE.lock();
    let handle = guard.as_mut().ok_or(FfiError::NotInitialized)?;
    f(&mut handle.engine)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a UUID from a string session id.
fn parse_session_id(session_id: &str) -> Result<uuid::Uuid, FfiError> {
    uuid::Uuid::parse_str(session_id)
        .map_err(|e| FfiError::InvalidArgument(format!("invalid session_id: {e}")))
}

/// Serialize a value to JSON, wrapping errors in [`FfiError`].
fn to_json<T: Serialize>(value: &T) -> Result<String, FfiError> {
    serde_json::to_string(value)
        .map_err(|e| FfiError::SerializationError(format!("JSON serialization failed: {e}")))
}

/// Convert a protocol [`Event`] into an [`FfiEvent`].
fn event_to_ffi(event: &Event) -> FfiEvent {
    let event_type = match event {
        Event::AppReady => "app_ready",
        Event::SessionCreated { .. } => "session_created",
        Event::SessionStarted { .. } => "session_started",
        Event::SessionStateChanged { .. } => "session_state_changed",
        Event::InterimTranscriptProduced { .. } => "interim_transcript",
        Event::FinalTranscriptProduced { .. } => "final_transcript",
        Event::AssistantReplyStarted { .. } => "assistant_reply_started",
        Event::AssistantReplyTextDelta { .. } => "assistant_reply_delta",
        Event::PlaybackCompleted { .. } => "playback_completed",
        Event::PlaybackInterrupted { .. } => "playback_interrupted",
        Event::DeviceListUpdated { .. } => "device_list_updated",
        Event::ErrorOccurred { .. } => "error_occurred",
        Event::DiagnosticsExported { .. } => "diagnostics_exported",
    };

    let payload_json = serde_json::to_string(event).unwrap_or_default();

    FfiEvent {
        event_type: event_type.to_owned(),
        payload_json,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }
}

// ---------------------------------------------------------------------------
// FFI entry points
// ---------------------------------------------------------------------------

/// Initialize the prx-voice-client subsystems.
///
/// Accepts a JSON configuration string and returns a JSON status object.
/// An empty or invalid config string causes default configuration to be used.
/// Creates a [`MockGateway`] transport, an event channel, and an [`Engine`],
/// stores the engine in the global [`ENGINE`] handle.
///
/// Returns `{"status": "initialized", "contract_version": <N>}` on success.
pub fn ffi_initialize(config_json: &str) -> Result<String, FfiError> {
    // Parse config JSON (or use defaults if empty/invalid).
    let _config: serde_json::Value = if config_json.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(config_json).unwrap_or_else(|_| {
            tracing::warn!("invalid config JSON, using defaults");
            serde_json::Value::Object(serde_json::Map::new())
        })
    };

    // Create transport, event channel, and engine.
    let transport = Box::new(MockGateway::new());
    let (event_tx, _event_rx) = event_channel(256);
    let mut engine = Engine::new(transport, event_tx);

    // Initialize engine (connects transport, polls for AppReady).
    engine.initialize().map_err(FfiError::from)?;

    // Store in global handle.
    let mut guard = ENGINE.lock();
    *guard = Some(EngineHandle { engine });

    let response = serde_json::json!({
        "status": "initialized",
        "contract_version": ContractVersion::CURRENT.version,
    });
    to_json(&response)
}

/// Shut down the prx-voice-client subsystems.
///
/// Calls [`Engine::shutdown`] on the global engine and removes the handle.
pub fn ffi_shutdown() -> Result<(), FfiError> {
    let mut guard = ENGINE.lock();
    if let Some(mut handle) = guard.take() {
        handle.engine.shutdown().map_err(FfiError::from)?;
    }
    Ok(())
}

/// Create a new voice session.
///
/// Accepts a JSON configuration string (currently unused beyond validation)
/// and returns a JSON object containing the newly allocated session identifier.
pub fn ffi_create_session(config_json: &str) -> Result<String, FfiError> {
    // Validate JSON if non-empty.
    if !config_json.is_empty() {
        let _value: serde_json::Value = serde_json::from_str(config_json)
            .map_err(|e| FfiError::InvalidArgument(format!("invalid session config JSON: {e}")))?;
    }

    with_engine(|engine| {
        engine
            .process_command(Command::CreateSession)
            .map_err(FfiError::from)?;

        let session = engine
            .active_session()
            .ok_or(FfiError::CoreError("no active session after create".into()))?;

        let response = serde_json::json!({ "session_id": session.id.0.to_string() });
        to_json(&response)
    })
}

/// Start a session by its identifier.
///
/// Parses the UUID from `session_id`, transitions the session through
/// Starting -> Listening, and forwards to the transport.
///
/// Returns `{"status": "started", "session_id": "<uuid>"}` on success.
pub fn ffi_start_session(session_id: &str) -> Result<String, FfiError> {
    let uuid = parse_session_id(session_id)?;

    with_engine(|engine| {
        engine
            .process_command(Command::StartSession { session_id: uuid })
            .map_err(FfiError::from)?;

        let response = serde_json::json!({
            "status": "started",
            "session_id": session_id,
        });
        to_json(&response)
    })
}

/// Stop a session by its identifier.
///
/// Transitions the session to Completed and clears the active session.
///
/// Returns `{"status": "stopped", "session_id": "<uuid>"}` on success.
pub fn ffi_stop_session(session_id: &str) -> Result<String, FfiError> {
    let uuid = parse_session_id(session_id)?;

    with_engine(|engine| {
        engine
            .process_command(Command::StopSession { session_id: uuid })
            .map_err(FfiError::from)?;

        let response = serde_json::json!({
            "status": "stopped",
            "session_id": session_id,
        });
        to_json(&response)
    })
}

/// Interrupt assistant playback in a session.
///
/// Only takes effect when the session is in the AssistantSpeaking state.
///
/// Returns `{"status": "interrupted", "session_id": "<uuid>"}` on success.
pub fn ffi_interrupt(session_id: &str) -> Result<String, FfiError> {
    let uuid = parse_session_id(session_id)?;

    with_engine(|engine| {
        engine
            .process_command(Command::InterruptPlayback { session_id: uuid })
            .map_err(FfiError::from)?;

        let response = serde_json::json!({
            "status": "interrupted",
            "session_id": session_id,
        });
        to_json(&response)
    })
}

/// Dispatch a command to the backend.
///
/// Accepts a JSON-encoded [`Command`] and processes it through the engine.
/// Returns `{"status": "ok"}` on success.
pub fn ffi_send_command(command_json: &str) -> Result<String, FfiError> {
    let command: Command = serde_json::from_str(command_json)
        .map_err(|e| FfiError::InvalidArgument(format!("invalid command JSON: {e}")))?;

    with_engine(|engine| {
        engine.process_command(command).map_err(FfiError::from)?;

        let response = serde_json::json!({ "status": "ok" });
        to_json(&response)
    })
}

/// Retrieve the current state of a session.
///
/// Returns a JSON-serialized [`FfiSessionState`].
pub fn ffi_get_session_state(session_id: &str) -> Result<String, FfiError> {
    if session_id.is_empty() {
        return Err(FfiError::InvalidArgument(
            "session_id must not be empty".to_owned(),
        ));
    }

    let uuid = parse_session_id(session_id)?;
    let sid = SessionId(uuid);

    with_engine(|engine| {
        let session = engine
            .session_manager()
            .get_session(&sid)
            .ok_or_else(|| FfiError::InvalidArgument(format!("session not found: {session_id}")))?;

        let state = FfiSessionState {
            session_id: session.id.0.to_string(),
            state: session.state.to_string(),
            turn_count: session.turn_count,
            created_at: session.created_at.to_rfc3339(),
        };

        to_json(&state)
    })
}

/// Poll the transport for pending events.
///
/// Calls [`Engine::poll_transport_events`] and converts the resulting
/// events to a JSON array of [`FfiEvent`] objects.
pub fn ffi_poll_events() -> Result<String, FfiError> {
    with_engine(|engine| {
        let events = engine.poll_transport_events().map_err(FfiError::from)?;

        let ffi_events: Vec<FfiEvent> = events.iter().map(event_to_ffi).collect();
        to_json(&ffi_events)
    })
}

/// List available audio devices.
///
/// Sends a [`Command::RefreshDeviceList`] through the engine and returns
/// the resulting device list as JSON. If no device events are produced,
/// an empty array is returned.
pub fn ffi_list_devices() -> Result<String, FfiError> {
    with_engine(|engine| {
        engine
            .process_command(Command::RefreshDeviceList)
            .map_err(FfiError::from)?;

        // Poll for DeviceListUpdated event.
        let events = engine.poll_transport_events().map_err(FfiError::from)?;
        for event in &events {
            if let Event::DeviceListUpdated { devices } = event {
                return to_json(devices);
            }
        }

        // No device event found; return empty list.
        to_json(&serde_json::json!([]))
    })
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for downstream crates.
pub mod prelude {
    pub use crate::{FfiError, FfiEvent, FfiResult, FfiSessionState};
    pub use crate::{
        ffi_create_session, ffi_get_session_state, ffi_initialize, ffi_interrupt, ffi_list_devices,
        ffi_poll_events, ffi_send_command, ffi_shutdown, ffi_start_session, ffi_stop_session,
    };
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

/// Shared test lock for all test modules that access the global ENGINE.
/// Must be used by every test that calls FFI functions to prevent parallel
/// interference.
#[cfg(test)]
static TEST_LOCK: LazyLock<parking_lot::Mutex<()>> = LazyLock::new(|| parking_lot::Mutex::new(()));

/// Helper: acquire the test lock, shutdown any previous engine, and initialize fresh.
#[cfg(test)]
fn init_engine() -> parking_lot::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock();
    let _ = ffi_shutdown();
    ffi_initialize("{}").expect("ffi_initialize failed");
    guard
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_initialize_and_shutdown() {
        let _guard = TEST_LOCK.lock();
        let _ = ffi_shutdown();

        let result = ffi_initialize("{}");
        assert!(result.is_ok(), "initialize should succeed");

        let json: serde_json::Value =
            serde_json::from_str(&result.unwrap()).expect("response should be valid JSON");
        assert_eq!(json["status"], "initialized");
        assert_eq!(json["contract_version"], 1);

        let shutdown = ffi_shutdown();
        assert!(shutdown.is_ok(), "shutdown should succeed");
    }

    #[test]
    fn test_ffi_create_and_start_session() {
        let _guard = init_engine();

        let create_result = ffi_create_session("{}").expect("create_session should succeed");
        let create_json: serde_json::Value =
            serde_json::from_str(&create_result).expect("response should be valid JSON");

        let session_id = create_json["session_id"]
            .as_str()
            .expect("session_id should be present");
        assert!(!session_id.is_empty());

        // Verify the UUID is valid.
        uuid::Uuid::parse_str(session_id).expect("session_id should be a valid UUID");

        let start_result = ffi_start_session(session_id).expect("start_session should succeed");
        let start_json: serde_json::Value =
            serde_json::from_str(&start_result).expect("response should be valid JSON");
        assert_eq!(start_json["status"], "started");

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_get_session_state() {
        let _guard = init_engine();

        let create_result = ffi_create_session("").expect("create_session should succeed");
        let create_json: serde_json::Value =
            serde_json::from_str(&create_result).expect("valid JSON");

        let session_id = create_json["session_id"]
            .as_str()
            .expect("session_id present");

        let state_result =
            ffi_get_session_state(session_id).expect("get_session_state should succeed");
        let state_json: serde_json::Value =
            serde_json::from_str(&state_result).expect("valid JSON");

        assert_eq!(state_json["session_id"], session_id);
        assert_eq!(state_json["state"], "Idle");
        assert_eq!(state_json["turn_count"], 0);
        assert!(!state_json["created_at"].as_str().unwrap_or("").is_empty());

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_poll_events_after_create() {
        let _guard = init_engine();

        let _create = ffi_create_session("").expect("create_session should succeed");

        // Poll events -- the engine drains transport events during process_command,
        // so poll_events may return an empty array. Verify we get valid JSON.
        let poll_result = ffi_poll_events().expect("poll_events should succeed");
        let _events: Vec<serde_json::Value> =
            serde_json::from_str(&poll_result).expect("valid JSON array");

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_stop_session() {
        let _guard = init_engine();

        let create_result = ffi_create_session("").expect("create");
        let create_json: serde_json::Value =
            serde_json::from_str(&create_result).expect("valid JSON");
        let session_id = create_json["session_id"].as_str().expect("session_id");

        ffi_start_session(session_id).expect("start");

        let stop_result = ffi_stop_session(session_id).expect("stop should succeed");
        let stop_json: serde_json::Value = serde_json::from_str(&stop_result).expect("valid JSON");
        assert_eq!(stop_json["status"], "stopped");

        // After stop, session should be in Completed state.
        let state_result = ffi_get_session_state(session_id).expect("get_state");
        let state_json: serde_json::Value =
            serde_json::from_str(&state_result).expect("valid JSON");
        assert_eq!(state_json["state"], "Completed");

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_interrupt() {
        let _guard = init_engine();

        let create_result = ffi_create_session("").expect("create");
        let create_json: serde_json::Value =
            serde_json::from_str(&create_result).expect("valid JSON");
        let session_id = create_json["session_id"].as_str().expect("session_id");

        ffi_start_session(session_id).expect("start");

        // Interrupt while Listening -- this should succeed silently (no-op).
        let interrupt_result = ffi_interrupt(session_id).expect("interrupt should succeed");
        let interrupt_json: serde_json::Value =
            serde_json::from_str(&interrupt_result).expect("valid JSON");
        assert_eq!(interrupt_json["status"], "interrupted");

        // Session should still be Listening since interrupt is a no-op when
        // not in AssistantSpeaking state.
        let state_result = ffi_get_session_state(session_id).expect("get_state");
        let state_json: serde_json::Value =
            serde_json::from_str(&state_result).expect("valid JSON");
        assert_eq!(state_json["state"], "Listening");

        let _ = ffi_shutdown();
    }
}

/// FFI contract tests verifying JSON response shapes.
///
/// These tests validate that every FFI function returns well-formed JSON
/// with the expected keys and value types. They are serialized via
/// [`TEST_LOCK`] because they share the global [`ENGINE`].
#[cfg(test)]
mod contract_tests {
    use super::*;

    // Uses the shared TEST_LOCK and init_engine() from the parent module.

    #[test]
    fn test_ffi_initialize_returns_valid_json() {
        let _guard = TEST_LOCK.lock();
        let _ = ffi_shutdown();

        let result = ffi_initialize("{}").expect("init should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("response should be valid JSON");

        assert!(
            parsed.get("status").is_some(),
            "response must have 'status' field"
        );
        assert_eq!(parsed["status"], "initialized");
        assert!(
            parsed.get("contract_version").is_some(),
            "response must have 'contract_version' field"
        );

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_create_session_returns_session_id() {
        let _guard = init_engine();

        let result = ffi_create_session("{}").expect("create should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("response should be valid JSON");

        let session_id = parsed["session_id"]
            .as_str()
            .expect("session_id should be a string");

        // Verify it is a valid UUID.
        uuid::Uuid::parse_str(session_id).expect("session_id should be a valid UUID");

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_get_state_returns_expected_fields() {
        let _guard = init_engine();

        let create_result = ffi_create_session("").expect("create");
        let create_json: serde_json::Value =
            serde_json::from_str(&create_result).expect("valid JSON");
        let session_id = create_json["session_id"].as_str().expect("session_id");

        let state_result = ffi_get_session_state(session_id).expect("get_state should succeed");
        let state_json: serde_json::Value =
            serde_json::from_str(&state_result).expect("response should be valid JSON");

        // Verify all expected keys are present.
        for key in &["session_id", "state", "turn_count", "created_at"] {
            assert!(
                state_json.get(*key).is_some(),
                "response must have '{}' field, got: {}",
                key,
                state_result,
            );
        }

        // Verify types
        assert!(state_json["session_id"].is_string());
        assert!(state_json["state"].is_string());
        assert!(state_json["turn_count"].is_number());
        assert!(state_json["created_at"].is_string());

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_poll_events_returns_array() {
        let _guard = init_engine();

        let result = ffi_poll_events().expect("poll_events should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("response should be valid JSON");

        assert!(
            parsed.is_array(),
            "poll_events must return a JSON array, got: {}",
            result,
        );

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_invalid_session_id_returns_error() {
        let _guard = init_engine();

        let result = ffi_start_session("not-a-valid-uuid");
        assert!(
            result.is_err(),
            "start_session with garbage UUID should return error"
        );

        let result = ffi_stop_session("zzz-garbage");
        assert!(
            result.is_err(),
            "stop_session with garbage UUID should return error"
        );

        let result = ffi_get_session_state("totally-invalid");
        assert!(
            result.is_err(),
            "get_session_state with garbage UUID should return error"
        );

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_command_before_init_returns_error() {
        let _guard = TEST_LOCK.lock();
        // Ensure no engine is initialized.
        let _ = ffi_shutdown();

        let result = ffi_create_session("{}");
        assert!(
            result.is_err(),
            "create_session before init should return NotInitialized error"
        );

        let result = ffi_poll_events();
        assert!(
            result.is_err(),
            "poll_events before init should return NotInitialized error"
        );
    }

    #[test]
    fn test_ffi_double_initialize() {
        let _guard = TEST_LOCK.lock();
        let _ = ffi_shutdown();

        let first = ffi_initialize("{}");
        assert!(first.is_ok(), "first init should succeed");

        let second = ffi_initialize("{}");
        assert!(second.is_ok(), "second init (re-init) should succeed");

        // Verify the engine is functional after re-init.
        let create = ffi_create_session("{}");
        assert!(
            create.is_ok(),
            "create_session after re-init should succeed"
        );

        let _ = ffi_shutdown();
    }

    #[test]
    fn test_ffi_full_lifecycle_json_contracts() {
        let _guard = init_engine();

        // 1. Create session — verify shape
        let create_result = ffi_create_session("{}").expect("create");
        let create_json: serde_json::Value =
            serde_json::from_str(&create_result).expect("valid JSON");
        assert!(create_json.get("session_id").is_some());
        let session_id = create_json["session_id"]
            .as_str()
            .expect("session_id string");
        uuid::Uuid::parse_str(session_id).expect("valid UUID");

        // 2. Start session — verify shape
        let start_result = ffi_start_session(session_id).expect("start");
        let start_json: serde_json::Value =
            serde_json::from_str(&start_result).expect("valid JSON");
        assert_eq!(start_json["status"], "started");
        assert_eq!(start_json["session_id"], session_id);

        // 3. Get state — verify shape
        let state_result = ffi_get_session_state(session_id).expect("get_state");
        let state_json: serde_json::Value =
            serde_json::from_str(&state_result).expect("valid JSON");
        assert_eq!(state_json["session_id"], session_id);
        assert_eq!(state_json["state"], "Listening");
        assert_eq!(state_json["turn_count"], 0);
        assert!(state_json["created_at"].is_string());

        // 4. Poll events — verify array shape
        let poll_result = ffi_poll_events().expect("poll_events");
        let poll_json: serde_json::Value = serde_json::from_str(&poll_result).expect("valid JSON");
        assert!(poll_json.is_array());

        // 5. Interrupt — verify shape
        let interrupt_result = ffi_interrupt(session_id).expect("interrupt");
        let interrupt_json: serde_json::Value =
            serde_json::from_str(&interrupt_result).expect("valid JSON");
        assert_eq!(interrupt_json["status"], "interrupted");
        assert_eq!(interrupt_json["session_id"], session_id);

        // 6. Stop session — verify shape
        let stop_result = ffi_stop_session(session_id).expect("stop");
        let stop_json: serde_json::Value = serde_json::from_str(&stop_result).expect("valid JSON");
        assert_eq!(stop_json["status"], "stopped");
        assert_eq!(stop_json["session_id"], session_id);

        // 7. Get state after stop — verify Completed
        let final_state = ffi_get_session_state(session_id).expect("get_state after stop");
        let final_json: serde_json::Value = serde_json::from_str(&final_state).expect("valid JSON");
        assert_eq!(final_json["state"], "Completed");

        let _ = ffi_shutdown();
    }
}
