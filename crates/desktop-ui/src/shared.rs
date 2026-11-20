//! Shared state between the egui UI thread and the async tokio runtime.
//!
//! Uses `parking_lot::Mutex` for cross-thread synchronization, keeping lock
//! durations minimal (copy-in / copy-out pattern).

use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

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
    /// Name of the input device being captured; empty when none.
    pub input_device: String,
}

/// Wakes the UI thread when another thread changes what the UI displays.
///
/// The app sets the `egui::Context` once at startup; until then (and in tests
/// without a window) a wake-up is a no-op. Clones share the same slot.
#[derive(Debug, Clone, Default)]
pub struct RepaintSignal {
    ctx: Arc<OnceLock<egui::Context>>,
    requests: Arc<AtomicUsize>,
}

impl RepaintSignal {
    /// Set the context to wake. Only the first call has an effect.
    pub fn set_context(&self, ctx: egui::Context) {
        let _ = self.ctx.set(ctx);
    }

    /// Ask for a repaint of the UI.
    pub fn request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if let Some(ctx) = self.ctx.get() {
            ctx.request_repaint();
        }
    }

    /// How many wake-ups were requested so far (for tests and diagnostics).
    pub fn request_count(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }
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
    /// Name of the input device being captured; empty when none.
    input_device: String,
}

/// Thread-safe shared state between the UI and async runtime.
///
/// Clone is cheap — all clones share the same inner `Arc<Mutex<...>>`.
#[derive(Debug, Clone)]
pub struct SharedAppState {
    inner: Arc<Mutex<SharedStateInner>>,
    repaint: RepaintSignal,
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
                input_device: String::new(),
            })),
            repaint: RepaintSignal::default(),
        }
    }

    /// Give the state the context to wake when the UI-visible data changes.
    /// Called once by the app at startup.
    pub fn set_repaint_context(&self, ctx: egui::Context) {
        self.repaint.set_context(ctx);
    }

    /// The wake-up signal (shared by all clones).
    pub fn repaint_signal(&self) -> &RepaintSignal {
        &self.repaint
    }

    /// The one way to change displayed data: runs `f` under the lock and, when
    /// it reports a change, wakes the UI after the lock is released. Setters
    /// that the engine calls with unchanged values every tick must report
    /// `false` then, or the UI would repaint at the engine's tick rate.
    fn mutate(&self, f: impl FnOnce(&mut SharedStateInner) -> bool) {
        let changed = f(&mut self.inner.lock());
        if changed {
            self.repaint.request();
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
        let debug_line = format!(
            "[{}] {}({})",
            event.timestamp, event.event_type, event.detail
        );
        self.mutate(|inner| {
            inner.debug_events.push(debug_line);
            if inner.debug_events.len() > MAX_DEBUG_EVENTS {
                let excess = inner.debug_events.len() - MAX_DEBUG_EVENTS;
                inner.debug_events.drain(..excess);
            }
            inner.pending_events.push(event);
            true
        });
    }

    /// Drain and return all pending events (called by the UI sync pass).
    pub fn take_events(&self) -> Vec<EventSnapshot> {
        let mut inner = self.inner.lock();
        std::mem::take(&mut inner.pending_events)
    }

    /// Update the session state label and active flag.
    pub fn update_session_state(&self, state: &str, active: bool) {
        self.mutate(|inner| {
            let changed = inner.session_state != state || inner.session_active != active;
            if changed {
                inner.session_state = state.to_owned();
                inner.session_active = active;
            }
            changed
        });
    }

    /// Set or clear the current session ID.
    pub fn set_session_id(&self, id: Option<String>) {
        self.mutate(|inner| {
            let changed = inner.session_id != id;
            inner.session_id = id;
            changed
        });
    }

    /// Append a transcript line to the active session transcript.
    pub fn add_transcript_line(&self, line: TranscriptLine) {
        self.mutate(|inner| {
            inner.transcript_lines.push(line);
            true
        });
    }

    /// Replace the text of the latest user line (a turn extended by a
    /// continuation); appends a user line when there is none.
    pub fn replace_last_user_line(&self, text: String) {
        self.mutate(|inner| {
            match inner.transcript_lines.iter_mut().rev().find(|l| l.role == "user") {
                Some(line) if line.text == text => return false,
                Some(line) => line.text = text,
                None => inner.transcript_lines.push(TranscriptLine {
                    role: "user".into(),
                    text,
                    is_final: true,
                }),
            }
            true
        });
    }

    /// Clear all transcript lines (e.g. when a session ends).
    pub fn clear_transcript(&self) {
        self.mutate(|inner| {
            let changed = !inner.transcript_lines.is_empty();
            inner.transcript_lines.clear();
            changed
        });
    }

    /// Append a completed session to the history list.
    pub fn add_history_entry(&self, entry: HistoryEntry) {
        self.mutate(|inner| {
            inner.history_entries.push(entry);
            true
        });
    }

    /// Set an error message for display in the UI.
    pub fn set_error(&self, message: &str, recoverable: bool) {
        self.mutate(|inner| {
            let changed = inner.error_message.as_deref() != Some(message)
                || inner.error_recoverable != recoverable;
            if changed {
                inner.error_message = Some(message.to_owned());
                inner.error_recoverable = recoverable;
            }
            changed
        });
    }

    /// Clear the current error message.
    pub fn clear_error(&self) {
        self.mutate(|inner| {
            let changed = inner.error_message.is_some() || inner.error_recoverable;
            inner.error_message = None;
            inner.error_recoverable = false;
            changed
        });
    }

    /// Update the network/transport status.
    pub fn set_network_status(&self, status: NetworkStatus) {
        self.mutate(|inner| {
            let changed = inner.network_status != status;
            inner.network_status = status;
            changed
        });
    }

    /// Update the reconnect attempt counter.
    pub fn set_reconnect_attempts(&self, count: u32) {
        self.mutate(|inner| {
            let changed = inner.reconnect_attempts != count;
            inner.reconnect_attempts = count;
            changed
        });
    }

    /// Record whether the microphone is paused by the echo gate.
    pub fn set_mic_paused(&self, paused: bool) {
        self.mutate(|inner| {
            let changed = inner.mic_paused != paused;
            inner.mic_paused = paused;
            changed
        });
    }

    /// Record the name of the input device being captured (empty when none).
    pub fn set_input_device(&self, name: &str) {
        self.mutate(|inner| {
            let changed = inner.input_device != name;
            if changed {
                inner.input_device = name.to_owned();
            }
            changed
        });
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
            input_device: inner.input_device.clone(),
        }
    }
}

impl Default for SharedAppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Methods of `SharedAppState` that do not change displayed data.
    const NOT_DISPLAY_SETTERS: &[&str] = &[
        "new",
        "set_repaint_context",
        "repaint_signal",
        "push_command",
        "take_commands",
        "take_events",
        "snapshot_for_ui",
    ];

    fn line(role: &str, text: &str) -> TranscriptLine {
        TranscriptLine { role: role.into(), text: text.into(), is_final: true }
    }

    fn entry() -> HistoryEntry {
        HistoryEntry {
            session_id: "s".into(),
            title: "t".into(),
            created_at: "now".into(),
            duration: "1s".into(),
            state: "Completed".into(),
        }
    }

    /// One call per display setter, in an order that makes each one change
    /// the state (so `clear_*` run after the matching setter).
    #[allow(clippy::type_complexity)]
    fn setter_calls() -> Vec<(&'static str, Box<dyn Fn(&SharedAppState)>)> {
        vec![
            ("push_event", Box::new(|s| s.push_event(EventSnapshot {
                event_type: "E".into(),
                detail: "d".into(),
                timestamp: "t".into(),
            }))),
            ("update_session_state", Box::new(|s| s.update_session_state("Listening", true))),
            ("set_session_id", Box::new(|s| s.set_session_id(Some("id".into())))),
            ("add_transcript_line", Box::new(|s| s.add_transcript_line(line("user", "a")))),
            ("replace_last_user_line", Box::new(|s| s.replace_last_user_line("b".into()))),
            ("clear_transcript", Box::new(|s| s.clear_transcript())),
            ("add_history_entry", Box::new(|s| s.add_history_entry(entry()))),
            ("set_error", Box::new(|s| s.set_error("boom", true))),
            ("clear_error", Box::new(|s| s.clear_error())),
            ("set_network_status", Box::new(|s| s.set_network_status(NetworkStatus::Failed))),
            ("set_reconnect_attempts", Box::new(|s| s.set_reconnect_attempts(3))),
            ("set_mic_paused", Box::new(|s| s.set_mic_paused(true))),
            ("set_input_device", Box::new(|s| s.set_input_device("Built-in"))),
        ]
    }

    /// Public `&self` methods declared in the `impl SharedAppState` block.
    fn declared_methods() -> Vec<String> {
        let src = include_str!("shared.rs");
        let impl_start = src.find("impl SharedAppState {").expect("impl block");
        let impl_end = src[impl_start..].find("\nimpl Default for SharedAppState").expect("end");
        src[impl_start..impl_start + impl_end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix("pub fn "))
            .map(|rest| rest.split(['(', '<']).next().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn every_display_setter_is_covered_by_this_test() {
        let covered: Vec<&str> = setter_calls().iter().map(|(n, _)| *n).collect();
        for name in declared_methods() {
            assert!(
                covered.contains(&name.as_str()) || NOT_DISPLAY_SETTERS.contains(&name.as_str()),
                "`SharedAppState::{name}` is new: add it to `setter_calls` (it must wake the UI \
                 through `mutate`) or to `NOT_DISPLAY_SETTERS` if it changes nothing displayed"
            );
        }
        for name in covered {
            assert!(declared_methods().iter().any(|m| m == name), "stale entry {name}");
        }
    }

    #[test]
    fn every_display_setter_wakes_the_ui() {
        let shared = SharedAppState::new();
        shared.set_repaint_context(egui::Context::default());
        for (name, call) in setter_calls() {
            let before = shared.repaint_signal().request_count();
            call(&shared);
            assert_eq!(
                shared.repaint_signal().request_count(),
                before + 1,
                "`{name}` did not request exactly one repaint"
            );
        }
    }

    #[test]
    fn unchanged_values_do_not_wake_the_ui() {
        let shared = SharedAppState::new();
        for (_, call) in setter_calls() {
            call(&shared);
        }
        shared.replace_last_user_line("b".into());
        let before = shared.repaint_signal().request_count();
        // The engine loop repeats these every tick.
        shared.update_session_state("Listening", true);
        shared.set_mic_paused(true);
        shared.set_network_status(NetworkStatus::Failed);
        shared.set_reconnect_attempts(3);
        shared.set_session_id(Some("id".into()));
        shared.replace_last_user_line("b".into());
        shared.clear_error();
        shared.clear_error();
        assert_eq!(shared.repaint_signal().request_count(), before);
    }

    #[test]
    fn clones_share_the_signal_and_commands_do_not_wake() {
        let shared = SharedAppState::new();
        let clone = shared.clone();
        clone.set_mic_paused(true);
        assert_eq!(shared.repaint_signal().request_count(), 1);
        shared.push_command("StartSession");
        let _ = shared.take_commands();
        let _ = shared.take_events();
        let _ = shared.snapshot_for_ui();
        assert_eq!(shared.repaint_signal().request_count(), 1);
    }
}
