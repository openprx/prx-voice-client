//! Standard span and field name conventions for structured tracing.
//!
//! Using consistent names across the codebase enables powerful log queries
//! and makes trace output immediately understandable.

/// Standard span names for consistent tracing across the application.
///
/// Each constant follows the `<subsystem>.<action>` naming convention.
pub mod span_names {
    /// Top-level span covering the entire lifetime of a session.
    pub const SESSION_LIFECYCLE: &str = "session.lifecycle";
    /// Span around a single state-machine transition.
    pub const SESSION_TRANSITION: &str = "session.transition";
    /// Span for processing a single inbound command.
    pub const COMMAND_PROCESS: &str = "command.process";
    /// Span for sending data through the transport layer.
    pub const TRANSPORT_SEND: &str = "transport.send";
    /// Span for receiving data from the transport layer.
    pub const TRANSPORT_RECV: &str = "transport.recv";
    /// Span for the initial transport connection handshake.
    pub const TRANSPORT_CONNECT: &str = "transport.connect";
    /// Span for a transport reconnection attempt.
    pub const TRANSPORT_RECONNECT: &str = "transport.reconnect";
    /// Span for processing an inbound audio frame.
    pub const AUDIO_INPUT: &str = "audio.input";
    /// Span for emitting an outbound audio frame.
    pub const AUDIO_OUTPUT: &str = "audio.output";
    /// Span for a voice-activity detection evaluation.
    pub const AUDIO_VAD: &str = "audio.vad";
    /// Span for a storage read operation.
    pub const STORAGE_READ: &str = "storage.read";
    /// Span for a storage write operation.
    pub const STORAGE_WRITE: &str = "storage.write";
    /// Span for loading configuration from disk.
    pub const CONFIG_LOAD: &str = "config.load";
    /// Span for persisting configuration to disk.
    pub const CONFIG_SAVE: &str = "config.save";
    /// Span for a foreign-function interface call (UniFFI / JNI).
    pub const FFI_CALL: &str = "ffi.call";
    /// Span for exporting a diagnostics bundle.
    pub const DIAGNOSTICS_EXPORT: &str = "diagnostics.export";
}

/// Standard field names for structured logging.
///
/// Using these constants as tracing span/event field keys ensures uniform
/// naming so that log aggregation and alerting rules work reliably.
pub mod field_names {
    /// Unique identifier of the session (UUID string).
    pub const SESSION_ID: &str = "session.id";
    /// Current state of the session state machine.
    pub const SESSION_STATE: &str = "session.state";
    /// The command variant being processed.
    pub const COMMAND_TYPE: &str = "command.type";
    /// The event variant being emitted or received.
    pub const EVENT_TYPE: &str = "event.type";
    /// Machine-readable error code.
    pub const ERROR_CODE: &str = "error.code";
    /// Whether the error is recoverable (`true`) or fatal (`false`).
    pub const ERROR_RECOVERABLE: &str = "error.recoverable";
    /// Current state of the transport layer.
    pub const TRANSPORT_STATE: &str = "transport.state";
    /// Audio sample rate in Hz.
    pub const AUDIO_SAMPLE_RATE: &str = "audio.sample_rate";
    /// Number of audio channels.
    pub const AUDIO_CHANNELS: &str = "audio.channels";
    /// Duration of an operation in milliseconds.
    pub const DURATION_MS: &str = "duration_ms";
    /// Attempt number (e.g. for retries).
    pub const ATTEMPT: &str = "attempt";
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_names_are_not_empty() {
        let names = [
            span_names::SESSION_LIFECYCLE,
            span_names::SESSION_TRANSITION,
            span_names::COMMAND_PROCESS,
            span_names::TRANSPORT_SEND,
            span_names::TRANSPORT_RECV,
            span_names::TRANSPORT_CONNECT,
            span_names::TRANSPORT_RECONNECT,
            span_names::AUDIO_INPUT,
            span_names::AUDIO_OUTPUT,
            span_names::AUDIO_VAD,
            span_names::STORAGE_READ,
            span_names::STORAGE_WRITE,
            span_names::CONFIG_LOAD,
            span_names::CONFIG_SAVE,
            span_names::FFI_CALL,
            span_names::DIAGNOSTICS_EXPORT,
        ];
        for name in &names {
            assert!(!name.is_empty(), "span name must not be empty");
            assert!(
                name.contains('.'),
                "span name '{name}' should follow subsystem.action convention"
            );
        }
    }

    #[test]
    fn test_field_names_are_not_empty() {
        let names = [
            field_names::SESSION_ID,
            field_names::SESSION_STATE,
            field_names::COMMAND_TYPE,
            field_names::EVENT_TYPE,
            field_names::ERROR_CODE,
            field_names::ERROR_RECOVERABLE,
            field_names::TRANSPORT_STATE,
            field_names::AUDIO_SAMPLE_RATE,
            field_names::AUDIO_CHANNELS,
            field_names::DURATION_MS,
            field_names::ATTEMPT,
        ];
        for name in &names {
            assert!(!name.is_empty(), "field name must not be empty");
        }
    }
}
