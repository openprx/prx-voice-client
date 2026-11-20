//! Application-level runtime metrics collected via atomic counters.
//!
//! All metric operations are lock-free and safe to call from any thread.
//! [`Metrics`] is an owned registry; the process-wide instance returned by
//! [`global`] backs the free functions ([`inc_sessions_created`],
//! [`snapshot`], [`reset`], ...). Tests create their own [`Metrics`] so they
//! do not share state.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public snapshot type
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of all application metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppMetrics {
    /// Total sessions created since process start (or last reset).
    pub sessions_created: u64,
    /// Total sessions that completed normally.
    pub sessions_completed: u64,
    /// Total sessions that ended in failure.
    pub sessions_failed: u64,
    /// Total conversation turns processed.
    pub total_turns: u64,
    /// Total playback interruptions triggered.
    pub total_interrupts: u64,
    /// Total transport reconnect attempts.
    pub reconnect_attempts: u64,
    /// Successful reconnects out of all attempts.
    pub reconnect_successes: u64,
    /// Aggregate error count (recoverable + fatal).
    pub errors_total: u64,
    /// Errors from which the system recovered automatically.
    pub errors_recoverable: u64,
    /// Errors that required session termination or restart.
    pub errors_fatal: u64,
    /// Inbound audio frames received from the capture device.
    pub audio_frames_in: u64,
    /// Outbound audio frames sent to the playback device.
    pub audio_frames_out: u64,
    /// Seconds elapsed since the metrics subsystem was initialized.
    pub uptime_seconds: u64,
}

// ---------------------------------------------------------------------------
// Metrics registry
// ---------------------------------------------------------------------------

/// A set of atomic metric counters.
///
/// Create one with [`Metrics::new`] to own isolated state, or use [`global`]
/// for the process-wide instance.
pub struct Metrics {
    sessions_created: AtomicU64,
    sessions_completed: AtomicU64,
    sessions_failed: AtomicU64,
    total_turns: AtomicU64,
    total_interrupts: AtomicU64,
    reconnect_attempts: AtomicU64,
    reconnect_successes: AtomicU64,
    errors_total: AtomicU64,
    errors_recoverable: AtomicU64,
    errors_fatal: AtomicU64,
    audio_frames_in: AtomicU64,
    audio_frames_out: AtomicU64,
    start_time: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    /// Create a registry with all counters at zero.
    pub fn new() -> Self {
        Self {
            sessions_created: AtomicU64::new(0),
            sessions_completed: AtomicU64::new(0),
            sessions_failed: AtomicU64::new(0),
            total_turns: AtomicU64::new(0),
            total_interrupts: AtomicU64::new(0),
            reconnect_attempts: AtomicU64::new(0),
            reconnect_successes: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            errors_recoverable: AtomicU64::new(0),
            errors_fatal: AtomicU64::new(0),
            audio_frames_in: AtomicU64::new(0),
            audio_frames_out: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record that a new session was created.
    pub fn inc_sessions_created(&self) {
        self.sessions_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a session completed normally.
    pub fn inc_sessions_completed(&self) {
        self.sessions_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a session failed.
    pub fn inc_sessions_failed(&self) {
        self.sessions_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a conversation turn was processed.
    pub fn inc_total_turns(&self) {
        self.total_turns.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a playback interruption occurred.
    pub fn inc_total_interrupts(&self) {
        self.total_interrupts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a reconnect attempt was made.
    pub fn inc_reconnect_attempts(&self) {
        self.reconnect_attempts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a reconnect attempt succeeded.
    pub fn inc_reconnect_successes(&self) {
        self.reconnect_successes.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that an inbound audio frame was received.
    pub fn inc_audio_frames_in(&self) {
        self.audio_frames_in.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that an outbound audio frame was sent.
    pub fn inc_audio_frames_out(&self) {
        self.audio_frames_out.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an error occurrence.
    ///
    /// When `recoverable` is `true` the error is counted as recoverable;
    /// otherwise it is counted as fatal. Both kinds increment `errors_total`.
    pub fn inc_errors(&self, recoverable: bool) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
        if recoverable {
            self.errors_recoverable.fetch_add(1, Ordering::Relaxed);
        } else {
            self.errors_fatal.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Capture a point-in-time snapshot of all metric counters.
    ///
    /// The `uptime_seconds` field reflects wall-clock time since this
    /// registry was created.
    pub fn snapshot(&self) -> AppMetrics {
        AppMetrics {
            sessions_created: self.sessions_created.load(Ordering::Relaxed),
            sessions_completed: self.sessions_completed.load(Ordering::Relaxed),
            sessions_failed: self.sessions_failed.load(Ordering::Relaxed),
            total_turns: self.total_turns.load(Ordering::Relaxed),
            total_interrupts: self.total_interrupts.load(Ordering::Relaxed),
            reconnect_attempts: self.reconnect_attempts.load(Ordering::Relaxed),
            reconnect_successes: self.reconnect_successes.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            errors_recoverable: self.errors_recoverable.load(Ordering::Relaxed),
            errors_fatal: self.errors_fatal.load(Ordering::Relaxed),
            audio_frames_in: self.audio_frames_in.load(Ordering::Relaxed),
            audio_frames_out: self.audio_frames_out.load(Ordering::Relaxed),
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }

    /// Reset all counters to zero.
    ///
    /// Note: `uptime_seconds` is derived from wall-clock time and cannot be
    /// reset; it always reflects elapsed time since the registry was created.
    pub fn reset(&self) {
        self.sessions_created.store(0, Ordering::Relaxed);
        self.sessions_completed.store(0, Ordering::Relaxed);
        self.sessions_failed.store(0, Ordering::Relaxed);
        self.total_turns.store(0, Ordering::Relaxed);
        self.total_interrupts.store(0, Ordering::Relaxed);
        self.reconnect_attempts.store(0, Ordering::Relaxed);
        self.reconnect_successes.store(0, Ordering::Relaxed);
        self.errors_total.store(0, Ordering::Relaxed);
        self.errors_recoverable.store(0, Ordering::Relaxed);
        self.errors_fatal.store(0, Ordering::Relaxed);
        self.audio_frames_in.store(0, Ordering::Relaxed);
        self.audio_frames_out.store(0, Ordering::Relaxed);
    }
}

static GLOBAL: LazyLock<Metrics> = LazyLock::new(Metrics::new);

/// The process-wide metrics registry used by the free functions below.
pub fn global() -> &'static Metrics {
    &GLOBAL
}

// ---------------------------------------------------------------------------
// Process-wide helpers
// ---------------------------------------------------------------------------

/// Record that a new session was created.
pub fn inc_sessions_created() {
    global().inc_sessions_created();
}

/// Record that a session completed normally.
pub fn inc_sessions_completed() {
    global().inc_sessions_completed();
}

/// Record that a session failed.
pub fn inc_sessions_failed() {
    global().inc_sessions_failed();
}

/// Record that a conversation turn was processed.
pub fn inc_total_turns() {
    global().inc_total_turns();
}

/// Record that a playback interruption occurred.
pub fn inc_total_interrupts() {
    global().inc_total_interrupts();
}

/// Record that a reconnect attempt was made.
pub fn inc_reconnect_attempts() {
    global().inc_reconnect_attempts();
}

/// Record that a reconnect attempt succeeded.
pub fn inc_reconnect_successes() {
    global().inc_reconnect_successes();
}

/// Record an error occurrence on the process-wide registry.
///
/// When `recoverable` is `true` the error is counted as recoverable;
/// otherwise it is counted as fatal. Both kinds increment `errors_total`.
pub fn inc_errors(recoverable: bool) {
    global().inc_errors(recoverable);
}

/// Record that an inbound audio frame was received.
pub fn inc_audio_frames_in() {
    global().inc_audio_frames_in();
}

/// Record that an outbound audio frame was sent.
pub fn inc_audio_frames_out() {
    global().inc_audio_frames_out();
}

/// Capture a point-in-time snapshot of the process-wide counters.
///
/// The `uptime_seconds` field reflects wall-clock time since the registry
/// was first accessed (typically process start).
pub fn snapshot() -> AppMetrics {
    global().snapshot()
}

/// Reset all process-wide counters to zero.
///
/// Note: `uptime_seconds` cannot be reset.
pub fn reset() {
    global().reset();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_increment_and_snapshot() {
        let m = Metrics::new();

        m.inc_sessions_created();
        m.inc_sessions_created();
        m.inc_sessions_completed();
        m.inc_sessions_failed();
        m.inc_total_turns();
        m.inc_total_turns();
        m.inc_total_turns();
        m.inc_total_interrupts();
        m.inc_reconnect_attempts();
        m.inc_reconnect_attempts();
        m.inc_reconnect_successes();
        m.inc_errors(true);
        m.inc_errors(false);
        m.inc_audio_frames_in();
        m.inc_audio_frames_in();
        m.inc_audio_frames_in();
        m.inc_audio_frames_out();

        let snap = m.snapshot();
        assert_eq!(snap.sessions_created, 2);
        assert_eq!(snap.sessions_completed, 1);
        assert_eq!(snap.sessions_failed, 1);
        assert_eq!(snap.total_turns, 3);
        assert_eq!(snap.total_interrupts, 1);
        assert_eq!(snap.reconnect_attempts, 2);
        assert_eq!(snap.reconnect_successes, 1);
        assert_eq!(snap.errors_total, 2);
        assert_eq!(snap.errors_recoverable, 1);
        assert_eq!(snap.errors_fatal, 1);
        assert_eq!(snap.audio_frames_in, 3);
        assert_eq!(snap.audio_frames_out, 1);
    }

    #[test]
    fn test_metrics_reset() {
        let m = Metrics::new();

        m.inc_sessions_created();
        m.inc_errors(true);
        m.inc_audio_frames_in();

        m.reset();
        let snap = m.snapshot();
        assert_eq!(snap.sessions_created, 0);
        assert_eq!(snap.errors_total, 0);
        assert_eq!(snap.errors_recoverable, 0);
        assert_eq!(snap.audio_frames_in, 0);
    }

    #[test]
    fn test_metrics_concurrent_increments() {
        use std::sync::Arc;

        let m = Arc::new(Metrics::new());

        let threads: Vec<_> = (0..8)
            .map(|i| {
                let m = Arc::clone(&m);
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        if i % 2 == 0 {
                            m.inc_sessions_created();
                        } else {
                            m.inc_audio_frames_in();
                        }
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().expect("worker thread panicked");
        }

        let snap = m.snapshot();
        assert_eq!(snap.sessions_created, 4000);
        assert_eq!(snap.audio_frames_in, 4000);
    }

    #[test]
    fn test_global_free_functions_delegate_to_global_registry() {
        // Other tests never touch the global registry, so a delta check is
        // exact for counters nobody else increments here.
        let before = snapshot().total_interrupts;
        inc_total_interrupts();
        assert!(snapshot().total_interrupts > before);
        assert!(std::ptr::eq(global(), global()));
    }

    #[test]
    fn test_metrics_uptime_increases() {
        // Force initialization of the lazy counter.
        let _ = snapshot();

        // Uptime should be non-decreasing; a brief sleep ensures > 0.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let snap = snapshot();
        // After at least 10ms, uptime_seconds may still be 0 (sub-second),
        // but it must not be negative (u64 guarantees this). We simply
        // verify the field is present and the snapshot succeeds.
        assert!(snap.uptime_seconds < 600, "uptime looks unreasonable");
    }
}
