//! Shared state between the egui UI thread and the async tokio runtime.
//!
//! Uses `parking_lot::Mutex` for cross-thread synchronization, keeping lock
//! durations minimal (copy-in / copy-out pattern).

use parking_lot::Mutex;
use std::sync::Arc;

use crate::{HistoryEntry, TranscriptLine};

/// Network/transport connection status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkStatus {
    /// Transport is connected and operational.
    Connected,
    /// Transport has been disconnected.
    Disconnected,
    /// Transport is establishing the initial connection.
    Connecting,
    /// Transport is attempting to reconnect.
    Reconnecting,
    /// Transport reconnection has failed permanently.
    Failed,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self::Connected
    }
}

/// Maximum number of debug events retained in the ring buffer.
const MAX_DEBUG_EVENTS: usize = 100;

/// Snapshot of a protocol event for UI display.
#[derive(Debug, Clone)]
pub struct EventSnapshot {
    /// Type name of the event (e.g. `"SessionCreated"`).
    pub event_type: String,
    /// Human-readable detail string.
    pub detail: String,
    /// RFC 3339 timestamp.
    pub timestamp: String,
}

/// Read-only snapshot of the shared state, safe for the UI render pass.
#[derive(Debug, Clone)]
pub struct UiSnapshot {
    /// Human-readable label for the current session state.
    pub session_state: String,
    /// Whether a session is currently active.
    pub session_active: bool,
    /// Current session ID, if any.
    pub session_id: Option<String>,
    /// Transcript lines for the active session.
    pub transcript_lines: Vec<TranscriptLine>,
    /// Completed session history entries.
    pub history_entries: Vec<HistoryEntry>,
    /// Debug event log (most recent last).
    pub debug_events: Vec<String>,
    /// Optional status bar message.
    pub status_message: Option<String>,
    /// Current error message (if any).
    pub error_message: Option<String>,
    /// Whether the current error is recoverable.
    pub error_recoverable: bool,
    /// Network/transport status.
    pub network_status: NetworkStatus,
    /// Reconnect attempt count.
    pub reconnect_attempts: u32,
    /// The microphone is paused while the assistant speaks (echo gate closed).
    pub mic_paused: bool,
}

/// Interior state guarded by the mutex.
#[derive(Debug)]
struct SharedStateInner {
    /// Commands queued by UI, consumed by the runtime.
    pending_commands: Vec<String>,
    /// Events from the runtime, consumed by the UI.
    pending_events: Vec<EventSnapshot>,
    /// Human-readable current session state label.
    session_state: String,
    /// Whether a session is currently active.
    session_active: bool,
    /// Current session ID (if any).
    session_id: Option<String>,
    /// Transcript lines for the active session.
    transcript_lines: Vec<TranscriptLine>,
    /// Completed session history entries.
    history_entries: Vec<HistoryEntry>,
    /// Debug event log (capped at [`MAX_DEBUG_EVENTS`]).
    debug_events: Vec<String>,
    /// Optional status bar message.
    status_message: Option<String>,
    /// Current error message (if any).
    error_message: Option<String>,
    /// Whether the current error is recoverable.
    error_recoverable: bool,
    /// Network/transport status.
    network_status: NetworkStatus,
    /// Reconnect attempt count.
    reconnect_attempts: u32,
    /// The microphone is paused while the assistant speaks (echo gate closed).
    mic_paused: bool,
}

/// Thread-safe shared state between the UI and async runtime.
///
/// Clone is cheap — all clones share the same inner `Arc<Mutex<...>>`.
#[derive(Debug, Clone)]
pub struct SharedAppState {
    inner: Arc<Mutex<SharedStateInner>>,
}

impl SharedAppState {
    /// Create a new shared state with default (idle) values.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SharedStateInner {
                pending_commands: Vec::new(),
                pending_events: Vec::new(),
                session_state: "Idle".to_owned(),
                session_active: false,
                session_id: None,
                transcript_lines: Vec::new(),
                history_entries: Vec::new(),
                debug_events: Vec::new(),
                status_message: None,
                error_message: None,
                error_recoverable: false,
                network_status: NetworkStatus::default(),
                reconnect_attempts: 0,
                mic_paused: false,
            })),
        }
    }

    /// Enqueue a command string from the UI for the runtime to process.
    pub fn push_command(&self, cmd: &str) {
        let mut inner = self.inner.lock();
        inner.pending_commands.push(cmd.to_owned());
    }

    /// Drain and return all pending commands (called by the runtime).
    pub fn take_commands(&self) -> Vec<String> {
        let mut inner = self.inner.lock();
        std::mem::take(&mut inner.pending_commands)
    }

    /// Push a new event snapshot from the runtime for the UI to consume.
    ///
    /// Also appends a formatted string to the debug event log, keeping the
    /// log capped at [`MAX_DEBUG_EVENTS`].
    pub fn push_event(&self, event: EventSnapshot) {
        let mut inner = self.inner.lock();
        let debug_line = format!(
            "[{}] {}({})",
            event.timestamp, event.event_type, event.detail
        );
        inner.debug_events.push(debug_line);
        if inner.debug_events.len() > MAX_DEBUG_EVENTS {
            let excess = inner.debug_events.len() - MAX_DEBUG_EVENTS;
            inner.debug_events.drain(..excess);
        }
        inner.pending_events.push(event);
    }

    /// Drain and return all pending events (called by the UI sync pass).
    pub fn take_events(&self) -> Vec<EventSnapshot> {
        let mut inner = self.inner.lock();
        std::mem::take(&mut inner.pending_events)
    }

    /// Update the session state label and active flag.
    pub fn update_session_state(&self, state: &str, active: bool) {
        let mut inner = self.inner.lock();
        inner.session_state = state.to_owned();
        inner.session_active = active;
    }

    /// Set or clear the current session ID.
    pub fn set_session_id(&self, id: Option<String>) {
        let mut inner = self.inner.lock();
        inner.session_id = id;
    }

    /// Append a transcript line to the active session transcript.
    pub fn add_transcript_line(&self, line: TranscriptLine) {
        let mut inner = self.inner.lock();
        inner.transcript_lines.push(line);
    }

    /// Replace the text of the latest user line (a turn extended by a
    /// continuation); appends a user line when there is none.
    pub fn replace_last_user_line(&self, text: String) {
        let mut inner = self.inner.lock();
        match inner.transcript_lines.iter_mut().rev().find(|l| l.role == "user") {
            Some(line) => line.text = text,
            None => inner.transcript_lines.push(TranscriptLine {
                role: "user".into(),
                text,
                is_final: true,
            }),
        }
    }

    /// Clear all transcript lines (e.g. when a session ends).
    pub fn clear_transcript(&self) {
        let mut inner = self.inner.lock();
        inner.transcript_lines.clear();
    }

    /// Append a completed session to the history list.
    pub fn add_history_entry(&self, entry: HistoryEntry) {
        let mut inner = self.inner.lock();
        inner.history_entries.push(entry);
    }

    /// Set an error message for display in the UI.
    pub fn set_error(&self, message: &str, recoverable: bool) {
        let mut inner = self.inner.lock();
        inner.error_message = Some(message.to_owned());
        inner.error_recoverable = recoverable;
    }

    /// Clear the current error message.
    pub fn clear_error(&self) {
        let mut inner = self.inner.lock();
        inner.error_message = None;
        inner.error_recoverable = false;
    }

    /// Update the network/transport status.
    pub fn set_network_status(&self, status: NetworkStatus) {
        let mut inner = self.inner.lock();
        inner.network_status = status;
    }

    /// Update the reconnect attempt counter.
    pub fn set_reconnect_attempts(&self, count: u32) {
        let mut inner = self.inner.lock();
        inner.reconnect_attempts = count;
    }

    /// Record whether the microphone is paused by the echo gate.
    pub fn set_mic_paused(&self, paused: bool) {
        self.inner.lock().mic_paused = paused;
    }

    /// Produce a read-only snapshot of all display-relevant state.
    ///
    /// The lock is held only for the duration of the clone, keeping the
    /// critical section short.
    pub fn snapshot_for_ui(&self) -> UiSnapshot {
        let inner = self.inner.lock();
        UiSnapshot {
            session_state: inner.session_state.clone(),
            session_active: inner.session_active,
            session_id: inner.session_id.clone(),
            transcript_lines: inner
                .transcript_lines
                .iter()
                .map(|l| TranscriptLine {
                    role: l.role.clone(),
                    text: l.text.clone(),
                    is_final: l.is_final,
                })
                .collect(),
            history_entries: inner
                .history_entries
                .iter()
                .map(|e| HistoryEntry {
                    session_id: e.session_id.clone(),
                    title: e.title.clone(),
                    created_at: e.created_at.clone(),
                    duration: e.duration.clone(),
                    state: e.state.clone(),
                })
                .collect(),
            debug_events: inner.debug_events.clone(),
            status_message: inner.status_message.clone(),
            error_message: inner.error_message.clone(),
            error_recoverable: inner.error_recoverable,
            network_status: inner.network_status.clone(),
            reconnect_attempts: inner.reconnect_attempts,
            mic_paused: inner.mic_paused,
        }
    }
}

impl Default for SharedAppState {
    fn default() -> Self {
        Self::new()
    }
}
