//! Tap supervisor thread (macOS): polls the tap's health on a normal thread, reports changes as events and re-creates the tap when needed.
//!
//! Decisions come from [`crate::tap_watchdog`]; this file only gathers the
//! observations (callback counter, whether the target runs) and carries out the
//! resulting steps. It is never on a real-time thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::core_audio_tap::{NativeTap, TapCtx, target_running};
use crate::tap_types::{TapConfig, TapError, TapEvent};
use crate::tap_watchdog::{DEFAULT_STALL_TIMEOUT, Observation, StallWatchdog, TapPolicy};

/// How often health is checked.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Everything the supervisor thread owns.
pub(crate) struct Supervisor {
    pub(crate) config: TapConfig,
    pub(crate) ctx: Arc<TapCtx>,
    /// The running native tap handed over from `CoreAudioTap::start`.
    pub(crate) native: Option<NativeTap>,
    pub(crate) events: Sender<TapEvent>,
    pub(crate) stop: Arc<AtomicBool>,
}

/// Start the supervisor thread.
pub(crate) fn spawn_supervisor(sup: Supervisor) -> Result<JoinHandle<()>, TapError> {
    std::thread::Builder::new()
        .name("prxvc-tap-supervisor".into())
        .spawn(move || run(sup))
        .map_err(|e| TapError::Internal(format!("could not start the tap supervisor: {e}")))
}

fn run(mut sup: Supervisor) {
    let counters = Arc::clone(&sup.ctx.counters);
    let mut watchdog = StallWatchdog::new(DEFAULT_STALL_TIMEOUT, Instant::now(), counters.snapshot().callbacks);
    let mut policy = TapPolicy::running();
    let mut reported_bad = 0;
    let mut last_failure: Option<TapError> = None;

    if let Some(native) = &sup.native {
        send(&sup, TapEvent::Started { restored: false, bundle_form: native.bundle_form });
    }

    while !sup.stop.load(Ordering::Acquire) {
        std::thread::park_timeout(POLL_INTERVAL);
        if sup.stop.load(Ordering::Acquire) {
            break;
        }
        let stats = counters.snapshot();
        let now = Instant::now();
        let observation = Observation {
            target_running: target_running(&sup.config.target),
            native_restore: sup.native.as_ref().is_some_and(|n| n.bundle_form),
            verdict: watchdog.observe(now, stats.callbacks),
        };
        let step = policy.step(observation);
        if let Some(event) = step.emit {
            send(&sup, event);
        }
        if step.teardown {
            sup.native = None; // stops IO and destroys the tap
        }
        if step.create {
            match NativeTap::open(&sup.config, &sup.ctx) {
                Ok(native) => {
                    let bundle_form = native.bundle_form;
                    sup.native = Some(native);
                    policy.created();
                    watchdog.reset(Instant::now(), counters.snapshot().callbacks);
                    last_failure = None;
                    send(&sup, TapEvent::Started { restored: true, bundle_form });
                }
                Err(error) => {
                    // Report a persisting failure once, not at every poll.
                    if last_failure.as_ref() != Some(&error) {
                        send(&sup, TapEvent::RestartFailed { error: error.clone() });
                    }
                    last_failure = Some(error);
                }
            }
        }
        if stats.bad_buffers > reported_bad {
            reported_bad = stats.bad_buffers;
            send(&sup, TapEvent::FormatMismatch { dropped: reported_bad });
        }
    }
    sup.native = None;
}

fn send(sup: &Supervisor, event: TapEvent) {
    // The receiver is gone only when the tap handle is being dropped.
    let _ = sup.events.send(event);
}
