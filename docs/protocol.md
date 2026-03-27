# Protocol Specification

This document defines the command/event protocol used for communication between the UI layer and the backend engine in PRX Voice Client.

## Contract Version

**Current version: 1**

The contract version is incremented when breaking changes are introduced to the command or event schema. Clients should check the version returned by initialization to ensure compatibility.

```rust
ContractVersion { version: 1 }
```

## Commands

Commands flow from the UI (or FFI caller) to the engine. Each command is a variant of the `Command` enum.

| Command | Parameters | Description |
|---------|-----------|-------------|
| `InitializeApp` | -- | Initialize all application subsystems. |
| `ShutdownApp` | -- | Gracefully shut down the application. |
| `CreateSession` | -- | Create a new voice session. Returns a session ID via event. |
| `StartSession` | `session_id: UUID` | Start an existing session. Transitions Idle -> Starting -> Listening. |
| `StopSession` | `session_id: UUID` | Stop a session. Transitions to Completed. |
| `InterruptPlayback` | `session_id: UUID` | Interrupt assistant audio playback. Only effective during AssistantSpeaking state. |
| `PauseSession` | `session_id: UUID` | Pause a running session. |
| `ResumeSession` | `session_id: UUID` | Resume a paused session. |
| `SwitchInputDevice` | `device_id: String` | Switch the active audio input device. |
| `SwitchOutputDevice` | `device_id: String` | Switch the active audio output device. |
| `RefreshDeviceList` | -- | Request an updated list of available audio devices. |
| `ExportDiagnostics` | -- | Export a diagnostics bundle to the filesystem. |
| `AcknowledgeError` | `error_id: String` | Dismiss an error by its identifier. |

### Command JSON Format

When sent over the FFI boundary or WebSocket transport, commands are serialized as JSON:

```json
{ "CreateSession": null }
```

```json
{ "StartSession": { "session_id": "01914a3b-7c0d-7f8a-b3e4-a1b2c3d4e5f6" } }
```

```json
{ "SwitchInputDevice": { "device_id": "built-in-mic-01" } }
```

## Events

Events flow from the engine to the UI. Each event is a variant of the `Event` enum.

| Event | Fields | Description |
|-------|--------|-------------|
| `AppReady` | -- | Application initialization completed successfully. |
| `SessionCreated` | `session_id: UUID` | A new session was created. |
| `SessionStarted` | `session_id: UUID` | A session has started listening. |
| `SessionStateChanged` | `session_id: UUID`, `from: String`, `to: String` | A session transitioned between states. |
| `InterimTranscriptProduced` | `session_id: UUID`, `text: String` | A partial (non-final) transcript fragment. |
| `FinalTranscriptProduced` | `session_id: UUID`, `text: String` | A finalized transcript of user speech. |
| `AssistantReplyStarted` | `session_id: UUID` | The assistant began generating a reply. |
| `AssistantReplyTextDelta` | `session_id: UUID`, `delta: String` | An incremental text chunk from the assistant reply. |
| `PlaybackCompleted` | `session_id: UUID` | Assistant audio playback finished. |
| `PlaybackInterrupted` | `session_id: UUID` | User interrupted assistant playback. |
| `DeviceListUpdated` | `devices: Vec<DeviceInfo>` | The available audio device list was refreshed. |
| `ErrorOccurred` | `code: String`, `message: String`, `recoverable: bool` | An error occurred in the backend. |
| `DiagnosticsExported` | `path: String` | A diagnostics bundle was written to the filesystem. |

### Event JSON Format

```json
{ "AppReady": null }
```

```json
{
  "SessionStateChanged": {
    "session_id": "01914a3b-7c0d-7f8a-b3e4-a1b2c3d4e5f6",
    "from": "Starting",
    "to": "Listening"
  }
}
```

```json
{
  "ErrorOccurred": {
    "code": "audio.device_lost",
    "message": "Input device disconnected",
    "recoverable": true
  }
}
```

### DeviceInfo Structure

```json
{
  "id": "built-in-mic-01",
  "name": "Built-in Microphone",
  "kind": "Input",
  "is_default": true
}
```

`kind` is one of: `"Input"`, `"Output"`.

## Error Codes

Error codes follow a `domain.code` format.

| Domain | Code | Description |
|--------|------|-------------|
| `session` | `command_failed` | A command could not be processed. |
| `session` | `timeout` | Session was idle for too long. |
| `session` | `failed` | Session terminated with an unrecoverable error. |
| `audio` | `device_lost` | Audio device was disconnected. |
| `audio` | `format_mismatch` | Audio format is incompatible. |
| `audio` | `buffer_overflow` | Audio buffer overflow occurred. |
| `audio` | `initialization_failed` | Audio subsystem failed to initialize. |
| `protocol` | `serialization_error` | JSON serialization/deserialization failed. |
| `protocol` | `connection_failed` | Transport connection could not be established. |
| `protocol` | `disconnected` | Transport was unexpectedly disconnected. |
| `protocol` | `send_failed` | Sending a message over the transport failed. |
| `protocol` | `timeout` | Transport operation timed out. |
| `network` | `reconnecting` | Reconnection attempt in progress. |
| `network` | `max_reconnect_attempts_exceeded` | All reconnection attempts exhausted. |
| `config` | `parse_error` | Configuration file could not be parsed. |
| `config` | `validation_error` | Configuration values are invalid. |
| `storage` | `database_error` | SQLite operation failed. |
| `security` | `credential_not_found` | Requested credential does not exist. |
| `ffi` | `not_initialized` | FFI layer used before initialization. |
| `ffi` | `invalid_argument` | Invalid argument passed across FFI boundary. |

## Transport

### MockGateway

Development and testing transport that simulates a backend locally.

- Commands are logged and translated into response events.
- Simulates multi-turn conversation flows: user speech recognition, assistant replies, and playback.
- Audio data triggers transcript events after configurable frame thresholds.
- No network connection required.

### WsTransport

Production WebSocket transport for real backend communication.

- Connects to a WebSocket server at a configurable URL.
- Commands are serialized as JSON text frames.
- Audio data is sent as binary frames with a session ID prefix.
- Events are received as JSON text frames and deserialized.
- Runs a background reader/writer task pair for non-blocking I/O.

### Message Format

| Direction | Format | Content |
|-----------|--------|---------|
| Command (text) | JSON | Serialized `Command` enum |
| Audio (binary) | Raw bytes | Session ID (16 bytes UUID) + PCM audio data |
| Event (text) | JSON | Serialized `Event` enum |

### FFI Transport (Mobile)

On mobile platforms, the C API wraps the same `Engine` + `Transport` stack:

- `prxvc_initialize()` creates the engine with a `MockGateway` (or `WsTransport`).
- `prxvc_poll_events()` returns a JSON array of `FfiEvent` objects.
- Each `FfiEvent` contains `event_type`, `payload_json`, and `timestamp`.

```json
[
  {
    "event_type": "session_created",
    "payload_json": "{\"SessionCreated\":{\"session_id\":\"...\"}}",
    "timestamp": "2026-03-27T10:30:00Z"
  }
]
```

## Session State Transitions

The engine drives session state transitions based on events received from the transport:

| Event | Triggered Transitions |
|-------|-----------------------|
| `FinalTranscriptProduced` | Listening -> UserSpeaking -> UserSpeechEnded -> Recognizing |
| `AssistantReplyStarted` | Recognizing -> Thinking -> AssistantSpeaking |
| `PlaybackCompleted` | AssistantSpeaking -> Listening |

These transitions are best-effort: if the session is not in the expected starting state, the transition is skipped with a warning log.
