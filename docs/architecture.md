# Architecture Overview

## Design Principles

1. **Session-centric** -- The core object is the `Session`, not individual messages. All state, turns, and transcripts are scoped to a session.
2. **Real-time priority** -- Audio capture and playback happen on dedicated threads with minimal latency. The UI never blocks audio.
3. **Observable** -- Every state change emits an event. The debug page and diagnostics bundle expose full system introspection.
4. **Transport-agnostic** -- The protocol layer defines abstract commands and events. Transports (mock, WebSocket) are interchangeable.
5. **Platform-native shells** -- Each platform (desktop, iOS, Android) has a thin native shell. Shared logic lives in Rust crates.
6. **Graceful degradation** -- Missing permissions, unavailable storage, or failed hotkey registration degrade features without crashing.

## System Architecture

```
+---------------------------------------------------------------+
|                      Platform Shell                            |
|  +------------------+  +----------------+  +----------------+ |
|  | Desktop (egui)   |  | iOS (SwiftUI)  |  | Android (Comp) | |
|  +--------+---------+  +-------+--------+  +-------+--------+ |
|           |                     |                    |          |
+-----------+---------------------+--------------------+----------+
            |                     |                    |
            v                     v                    v
+---------------------------------------------------------------+
|                     FFI / Bridge Layer                         |
|  Desktop: direct Rust call  |  Mobile: C API (prxvc-ffi)      |
+-------------------------------+-------------------------------+
                                |
                                v
+---------------------------------------------------------------+
|                      Rust Core Engine                          |
|  +----------+  +-----------+  +----------+  +-----------+     |
|  | session  |  | protocol  |  | audio    |  | config    |     |
|  +----------+  +-----------+  +----------+  +-----------+     |
|  +----------+  +-----------+  +----------+  +-----------+     |
|  | core     |  | storage   |  | observe  |  | security  |     |
|  +----------+  +-----------+  +----------+  +-----------+     |
|  +----------+  +-----------+  +----------+                    |
|  | i18n     |  | testkit   |  | adapters |                    |
|  +----------+  +-----------+  +----------+                    |
+---------------------------------------------------------------+
                                |
                                v
+---------------------------------------------------------------+
|                     Transport Adapters                         |
|  +------------------+  +------------------------------------+ |
|  | MockGateway      |  | WsTransport (WebSocket JSON)       | |
|  | (dev/test)       |  | (production)                       | |
|  +------------------+  +------------------------------------+ |
+---------------------------------------------------------------+
```

## Crate Map

| Crate | Package Name | Purpose | Key Types |
|-------|-------------|---------|-----------|
| `crates/core` | `prxvc-core` | App context, lifecycle, engine orchestration | `AppContext`, `AppState`, `Engine`, `PrxvcError` |
| `crates/session` | `prxvc-session` | Session + interpreting state machines, turns, driver | `Session`, `SessionState`, `InterpretSession`, `InterpretDriver`, `Turn` |
| `crates/audio` | `prxvc-audio` | Audio I/O, per-app capture, resampling, VAD, routing | `AudioFrame`, `AudioPipeline`, `CoreAudioTapSource`, `MonoResampler16k`, `AppAudioSource` |
| `crates/protocol` | `prxvc-protocol` | Command/event contracts, transport trait | `Command`, `Event`, `Transport`, `ContractVersion` |
| `crates/config` | `prxvc-config` | Configuration model, TOML persistence | `AppConfig`, `ConfigManager`, `AppConfigManager` |
| `crates/storage` | `prxvc-storage` | Local SQLite session persistence | `StorageManager`, `SessionRecord` |
| `crates/observe` | `prxvc-observe` | Tracing, metrics, diagnostics export | `DiagnosticsBundle`, `SystemInfo` |
| `crates/security` | `prxvc-security` | Credential store, data sanitization | `SecurityError`, `CredentialStore`, `Sanitizer` |
| `crates/ffi` | `prxvc-ffi` | FFI boundary for iOS/Android | `ffi_initialize`, `ffi_create_session`, `FfiEvent` |
| `crates/desktop-ui` | `prxvc-desktop-ui` | egui view layer, shared UI state | `DesktopUiState`, `SharedAppState`, `SessionViewModel` |
| `crates/i18n` | `prxvc-i18n` | Internationalization (EN, ZH-CN) | `Locale`, `t()`, `t_with()` |
| `crates/adapters` | `prxvc-adapters` | MT/TTS adapters (translation, speech synthesis) | `Translator`, `Synthesizer`, `OpenAiTranslator`, `SineToneSynthesizer` |
| `crates/testkit` | `prxvc-testkit` | Test utilities and mocks | `MockAudioSource`, test factory functions |
| `apps/desktop` | `prxvc-desktop` | Desktop binary (egui + tokio) | `PrxVoiceApp`, `main()` |

## Data Flow

```
User Action (click / hotkey / tray)
    |
    v
UI Thread: push command string to SharedAppState
    |
    v
Engine Thread: take_commands() -> parse -> Command enum
    |
    v
Engine::process_command()
    |-- Update SessionManager (state machine transition)
    |-- Forward to Transport (MockGateway / WsTransport)
    |-- Broadcast resulting Event via EventBroadcaster
    |
    v
Engine Thread: poll_transport_events()
    |-- Receive events from Transport
    |-- Drive session state machine for transcript/playback events
    |-- Broadcast all events
    |
    v
SharedAppState: update session state, transcript, history
    |
    v
UI Thread: sync_from_shared() -> render
```

## State Machine

The session state machine enforces all valid transitions. Any invalid transition returns an error without mutating state.

```
                         +--------+
                         |  Idle  |
                         +---+----+
                             |
                      [CreateSession]
                             |
                             v
                       +-----------+
                       | Starting  |---> Failed
                       +-----+-----+
                             |
                             v
              +----------> +----------+
              |            | Listening |<--------+--------+--------+
              |            +-----+----+          |        |        |
              |                  |          [playback] [recover] [interrupt]
              |           [voice detected]       |        |        |
              |                  v               |        |        |
              |          +--------------+        |        |        |
              |          | UserSpeaking |        |        |        |
              |          +------+-------+        |        |        |
              |                 |                 |        |        |
              |          [speech ended]           |        |        |
              |                 v                 |        |        |
              |         +---------------+         |        |        |
              |         | UserSpeechEnded|        |        |        |
              |         +-------+-------+         |        |        |
              |                 |                  |        |        |
              |                 v                  |        |        |
              |          +-------------+           |        |        |
              |          | Recognizing |-----------|---> Recovering |
              |          +------+------+           |                |
              |                 |                   |               |
              |                 v                   |               |
              |           +----------+              |               |
              |           | Thinking |--------------|---> Recovering|
              |           +-----+----+              |               |
              |                 |                    |               |
              |                 v                    |               |
              |       +-------------------+          |               |
              |       | AssistantSpeaking |--->  Interrupted --------+
              |       +---------+---------+          |
              |                 |                    |
              |          [playback done]       [user interrupts]
              |                 |                    |
              +----<------------+                    +---> Completed
              |                                      |
              +---> Paused ---> Listening             |
              |                                      |
              +---> Completed <----------------------+
              |
              +---> Failed (reachable from most states)
```

## Threading Model

The application uses three distinct thread categories:

1. **UI Thread** (main thread)
   - Runs the egui/eframe event loop on desktop, or native UI framework on mobile.
   - Renders at ~60 FPS, polling `SharedAppState` for display data.
   - Pushes command strings into `SharedAppState.pending_commands` when buttons are clicked.
   - Never blocks on I/O or engine operations.

2. **Engine Thread** (background, tokio runtime)
   - Runs the `Engine` in a polling loop (~50ms interval).
   - Drains UI commands and processes them through the session state machine.
   - Polls the transport for incoming events.
   - Manages reconnection logic with exponential backoff.

3. **Audio Threads** (cpal callbacks)
   - Input callback: captures microphone data into `AudioFrame` structs.
   - Output callback: plays buffered audio frames to the speaker.
   - These callbacks run on real-time OS threads managed by `cpal` and must never block.

**Communication:**

- UI <-> Engine: `SharedAppState` (an `Arc<parking_lot::Mutex<...>>` wrapper with typed accessors).
- Engine <-> Transport: direct method calls on the `Transport` trait object.
- Events: `EventBroadcaster` (tokio broadcast channel) for engine-internal distribution.

## FFI Boundary

The `prxvc-ffi` crate exposes a minimal C-compatible API for mobile platforms:

- **JSON-based** -- all function arguments and return values are JSON strings.
- **Flat surface area** -- no complex types cross the boundary; only `*const c_char` and integers.
- **Event polling model** -- the host calls `prxvc_poll_events()` periodically to receive events as a JSON array.
- **Global engine** -- a single `LazyLock<Mutex<Option<EngineHandle>>>` holds the engine instance.

Key FFI functions:

| Function | Purpose |
|----------|---------|
| `prxvc_initialize(config_json)` | Initialize engine, returns status JSON |
| `prxvc_shutdown()` | Graceful shutdown |
| `prxvc_create_session(config_json)` | Create a new voice session |
| `prxvc_start_session(session_id)` | Start a session |
| `prxvc_stop_session(session_id)` | Stop a session |
| `prxvc_interrupt(session_id)` | Interrupt assistant playback |
| `prxvc_poll_events()` | Poll for pending events |
| `prxvc_get_session_state(session_id)` | Query session state |
| `prxvc_list_devices()` | List audio devices |
| `prxvc_free_string(ptr)` | Free a string returned by FFI functions |

## Storage

- **SQLite** (`prxvc-storage`) -- persists session records (id, state, created_at, duration, turn count, transcript). Located in the platform data directory.
- **TOML** (`prxvc-config`) -- persists user configuration (audio, UI, network, privacy, debug settings). Located in the platform config directory.
- **Platform paths** resolved via the `directories` crate:
  - macOS: `~/Library/Application Support/com.prx.prx-voice-client/`
  - Linux: `~/.config/prx/prx-voice-client/` (config), `~/.local/share/prx/prx-voice-client/` (data)
  - Windows: `%APPDATA%\prx\prx-voice-client\`

## Audio Pipeline

```
Input Path:
  Platform Microphone
      |
      v
  cpal Input Callback (real-time thread)
      |
      v
  AudioFrame { sequence_no, sample_rate, channels, format, data }
      |
      v
  VAD (Voice Activity Detection) -> VadDecision { speaking, confidence }
      |
      v
  Session Router (tag with session_id) -> AudioInputFrame
      |
      v
  Transport.send_audio_data()

Output Path:
  Transport (received audio events)
      |
      v
  AudioBuffer (jitter buffer, reordering)
      |
      v
  AudioOutputFrame (tagged with session_id)
      |
      v
  cpal Output Callback (real-time thread)
      |
      v
  Platform Speaker / Headphone
```

Audio configuration defaults:
- Sample rate: 16,000 Hz
- Channels: 1 (mono)
- Format: F32
- Echo control: enabled
- Input/output gain: 1.0

## Interpreting Pipeline

The bilingual interpreter runs two independent, symmetric pipelines (one per
direction) over a consecutive turn model. Each turn is captured, recognized,
translated, synthesized, and played back before the floor is released; the
per-turn latency is absorbed by turn-taking.

```
Capture (per application)
    |
    v
AppAudioSource  (macOS: CoreAudioTapSource over a Core Audio process tap)
    |
    v
MonoResampler16k  (downmix to mono, resample to 16 kHz)
    |
    v
ASR  ->  recognized text
    |
    v
InterpretDriver / InterpretSession  (state machine = single source of truth)
    Listening -> Recognizing -> Translating -> Speaking -> Idle
    |
    |-- Translator (prxvc-adapters)   source text  -> target text
    |-- Synthesizer (prxvc-adapters)  target text  -> audio (optional cloned voice)
    |
    v
Playback
    - to the ear (remote -> local direction), or
    - into the meeting app via VirtualMicSink (local -> remote)
```

The pipeline stays platform- and engine-agnostic through traits:

- `AppAudioSource` / `VirtualMicSink` (`prxvc-audio`) abstract per-app capture and
  virtual-mic injection. macOS backends are provided; other platforms plug in
  behind the same traits.
- `Translator` / `Synthesizer` (`prxvc-adapters`) abstract machine translation and
  speech synthesis. Bundled implementations include an OpenAI-compatible
  translator and a tone synthesizer for development.
