//! Half-duplex echo gate.
//!
//! With speakers and a microphone in the same room the assistant's voice
//! comes back through the microphone and would be transcribed as if the user
//! had said it. Acoustic echo cancellation is out of scope here; instead the
//! microphone is simply not listened to while the assistant is speaking.
//!
//! [`EchoGate`] is a small pure state machine: callers tell it what the
//! playback side is doing and the current time, and it answers whether
//! microphone audio may reach the recogniser. It owns no threads, clocks or
//! devices, so it is unit tested without audio hardware.
//!
//! The gate is closed while
//! - a reply is open (from `tts_start` / the first audio chunk until the
//!   server's `tts_end`), which covers the time before playback starts,
//! - or output audio is still queued (not drained),
//!
//! and for [`DEFAULT_HOLD_OFF`] after both stopped, so the tail of the voice
//! still travelling through the room is not transcribed either.
//! [`EchoGateHandle`] shares one gate between the engine thread (playback
//! side) and the audio thread (microphone side).

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How long the microphone stays closed after playback has drained.
pub const DEFAULT_HOLD_OFF: Duration = Duration::from_millis(300);

/// Longest a reply may stay "open" without any audio or event activity while
/// nothing is queued. Protects against a lost `tts_end` muting the microphone
/// forever.
pub const REPLY_STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// What [`EchoGate::poll`] tells the microphone side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    /// Microphone audio must be discarded.
    Closed,
    /// Microphone audio may be processed. `just_reopened` is true exactly
    /// once after a closed period: the consumer must discard whatever it
    /// accumulated so far (a tail of the assistant's voice).
    Open {
        /// The gate has just reopened after being closed.
        just_reopened: bool,
    },
}

/// Decides whether microphone audio may reach the recogniser.
#[derive(Debug, Clone)]
pub struct EchoGate {
    enabled: bool,
    hold_off: Duration,
    /// A reply is being delivered (between its start and its end marker).
    reply_open: bool,
    /// Last time the reply produced audio or an event.
    last_reply_activity: Option<Instant>,
    /// Output still holds audio (last value passed to `update`).
    queued: bool,
    /// The gate stays closed until this time once playback is over.
    closed_until: Option<Instant>,
    /// The gate closed since the consumer last saw it open: whatever the
    /// consumer accumulated may contain the assistant's voice.
    was_closed: bool,
}

impl EchoGate {
    /// Create a gate. A disabled gate (`echo_control` off, full duplex) is
    /// always open.
    pub fn new(enabled: bool, hold_off: Duration) -> Self {
        Self {
            enabled,
            hold_off,
            reply_open: false,
            last_reply_activity: None,
            queued: false,
            closed_until: None,
            was_closed: false,
        }
    }

    /// Whether the gate does anything at all.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// A reply started (`tts_start`) or a chunk of it arrived: from now on
    /// the microphone is closed, even before playback begins.
    pub fn reply_activity(&mut self, now: Instant) {
        self.reply_open = true;
        self.last_reply_activity = Some(now);
        self.closed_until = None;
        self.was_closed |= self.enabled;
    }

    /// The server announced the end of the reply (`tts_end`). Queued audio
    /// keeps the gate closed until it has drained.
    pub fn reply_ended(&mut self, now: Instant) {
        if self.reply_open {
            self.reply_open = false;
            self.begin_hold_off_if_idle(now);
        }
    }

    /// The assistant was cut off locally (interrupt, stop): playback has been
    /// flushed, so only the hold-off remains.
    pub fn flushed(&mut self, now: Instant) {
        self.reply_open = false;
        self.queued = false;
        self.was_closed |= self.enabled;
        self.begin_hold_off_if_idle(now);
    }

    /// Report whether the output still holds queued audio. Call regularly
    /// (the gate has no timers of its own).
    pub fn update(&mut self, output_drained: bool, now: Instant) {
        let was_queued = self.queued;
        self.queued = !output_drained;
        if self.reply_open
            && output_drained
            && self
                .last_reply_activity
                .is_some_and(|at| now.duration_since(at) > REPLY_STALL_TIMEOUT)
        {
            tracing::warn!("reply never ended; reopening the microphone");
            self.reply_open = false;
            self.begin_hold_off_if_idle(now);
        }
        if self.queued {
            self.closed_until = None;
            self.was_closed |= self.enabled;
        } else if was_queued {
            self.begin_hold_off_if_idle(now);
        }
    }

    fn begin_hold_off_if_idle(&mut self, now: Instant) {
        if !self.reply_open && !self.queued {
            self.closed_until = Some(now + self.hold_off);
        }
    }

    /// Whether the microphone is currently closed (without side effects).
    pub fn is_closed(&self, now: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        if self.reply_open || self.queued {
            return true;
        }
        self.closed_until.is_some_and(|until| now < until)
    }

    /// Current decision for the microphone side. The `just_reopened` flag is
    /// consumed by this call.
    pub fn poll(&mut self, now: Instant) -> GateState {
        if self.is_closed(now) {
            self.was_closed = true;
            return GateState::Closed;
        }
        self.closed_until = None;
        let just_reopened = std::mem::take(&mut self.was_closed);
        GateState::Open { just_reopened }
    }
}

/// A gate shared between the playback side and the microphone side.
#[derive(Debug, Clone)]
pub struct EchoGateHandle {
    inner: Arc<Mutex<EchoGate>>,
}

impl EchoGateHandle {
    /// Share `gate`.
    pub fn new(gate: EchoGate) -> Self {
        Self {
            inner: Arc::new(Mutex::new(gate)),
        }
    }

    /// A gate that never closes (full duplex).
    pub fn disabled() -> Self {
        Self::new(EchoGate::new(false, DEFAULT_HOLD_OFF))
    }

    /// Run `f` with the gate locked.
    pub fn with<R>(&self, f: impl FnOnce(&mut EchoGate) -> R) -> R {
        f(&mut self.inner.lock())
    }

    /// See [`EchoGate::poll`].
    pub fn poll(&self, now: Instant) -> GateState {
        self.inner.lock().poll(now)
    }

    /// See [`EchoGate::is_closed`].
    pub fn is_closed(&self, now: Instant) -> bool {
        self.inner.lock().is_closed(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLD: Duration = Duration::from_millis(300);

    fn gate() -> EchoGate {
        EchoGate::new(true, HOLD)
    }

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn open_when_idle() {
        let t0 = Instant::now();
        let mut g = gate();
        assert_eq!(
            g.poll(t0),
            GateState::Open {
                just_reopened: false
            }
        );
    }

    #[test]
    fn closed_from_reply_start_before_any_audio_is_queued() {
        let t0 = Instant::now();
        let mut g = gate();
        g.reply_activity(t0);
        g.update(true, ms(t0, 10));
        assert_eq!(g.poll(ms(t0, 20)), GateState::Closed);
    }

    #[test]
    fn closed_while_playing() {
        let t0 = Instant::now();
        let mut g = gate();
        g.reply_activity(t0);
        g.update(false, ms(t0, 10));
        g.reply_ended(ms(t0, 20));
        // The reply is over but the output still holds audio.
        g.update(false, ms(t0, 2_000));
        assert!(g.is_closed(ms(t0, 2_000)));
    }

    #[test]
    fn stays_closed_for_the_hold_off_after_drain_then_reopens() {
        let t0 = Instant::now();
        let mut g = gate();
        g.reply_activity(t0);
        g.update(false, ms(t0, 10));
        g.reply_ended(ms(t0, 20));
        g.update(true, ms(t0, 1_000)); // drained
        assert_eq!(g.poll(ms(t0, 1_000)), GateState::Closed);
        assert_eq!(g.poll(ms(t0, 1_299)), GateState::Closed);
        assert_eq!(
            g.poll(ms(t0, 1_300)),
            GateState::Open {
                just_reopened: true
            }
        );
        // The reopen signal fires once.
        assert_eq!(
            g.poll(ms(t0, 1_310)),
            GateState::Open {
                just_reopened: false
            }
        );
    }

    #[test]
    fn a_new_chunk_during_the_hold_off_closes_the_gate_again() {
        let t0 = Instant::now();
        let mut g = gate();
        g.reply_activity(t0);
        g.update(false, ms(t0, 10));
        g.reply_ended(ms(t0, 20));
        g.update(true, ms(t0, 100));
        g.reply_activity(ms(t0, 200));
        g.update(false, ms(t0, 210));
        assert_eq!(g.poll(ms(t0, 900)), GateState::Closed);
    }

    #[test]
    fn disabled_gate_never_closes() {
        let t0 = Instant::now();
        let mut g = EchoGate::new(false, HOLD);
        g.reply_activity(t0);
        g.update(false, ms(t0, 10));
        assert_eq!(
            g.poll(ms(t0, 20)),
            GateState::Open {
                just_reopened: false
            }
        );
        assert!(!g.is_closed(ms(t0, 20)));
    }

    #[test]
    fn reopens_after_an_interrupt_flush_following_the_hold_off() {
        let t0 = Instant::now();
        let mut g = gate();
        g.reply_activity(t0);
        g.update(false, ms(t0, 10));
        assert!(g.is_closed(ms(t0, 500)));
        g.flushed(ms(t0, 600));
        assert_eq!(g.poll(ms(t0, 700)), GateState::Closed);
        g.update(true, ms(t0, 750));
        assert_eq!(g.poll(ms(t0, 899)), GateState::Closed);
        assert_eq!(
            g.poll(ms(t0, 900)),
            GateState::Open {
                just_reopened: true
            }
        );
    }

    #[test]
    fn a_lost_reply_end_does_not_mute_the_microphone_forever() {
        let t0 = Instant::now();
        let mut g = gate();
        g.reply_activity(t0);
        g.update(true, ms(t0, 100));
        assert!(g.is_closed(ms(t0, 5_000)));
        g.update(true, t0 + REPLY_STALL_TIMEOUT + Duration::from_secs(1));
        // Reopens (after the hold-off, which starts at the stall detection).
        let later = t0 + REPLY_STALL_TIMEOUT + Duration::from_secs(2);
        assert_eq!(
            g.poll(later),
            GateState::Open {
                just_reopened: true
            }
        );
    }
}
