//! Tap supervisor thread (macOS): polls the tap's health on a normal thread, reports changes as events and re-creates the tap when needed.
//!
//! Decisions come from [`crate::tap_watchdog`] (stall, target gone) and
//! [`crate::tap_zero_watchdog`] (exact zeros while the target plays); this file
//! only gathers the observations (callback counters, whether the target runs and
//! plays) and carries out the resulting steps. It is never on a real-time thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::call_activity::native::target_output_running;
use crate::core_audio_tap::{NativeTap, TapCtx, target_running};
use crate::tap_types::{TapConfig, TapError, TapEvent};
use crate::tap_types::TapStats;
use crate::tap_watchdog::{DEFAULT_STALL_TIMEOUT, Observation, Phase, StallWatchdog, TapPolicy};
use crate::tap_zero_watchdog::{DEFAULT_ZERO_TIMEOUT, ZeroObservation, ZeroWatchdog};

/// How often health is checked.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Everything the supervisor thread owns.
pub(crate) struct Supervisor {
    pub(crate) config: TapConfig,
    pub(crate) ctx: Arc<TapCtx>,
    /// Whether re-creation runs the consent gate.
    pub(crate) check_consent: bool,
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
    let mut zero = ZeroWatchdog::new(
        DEFAULT_ZERO_TIMEOUT,
        sup.config.target.describe(),
        Instant::now(),
        zero_observation(false, &counters.snapshot()),
    );
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
        let mut step = policy.step(observation);
        if policy.phase() == Phase::Running && !step.teardown {
            // Only a running tap with a running target has zeros worth judging.
            let zero_step = zero.step(now, zero_observation(target_output_running(&sup.config.target), &stats));
            if let Some(event) = zero_step.emit {
                send(&sup, event);
            }
            if zero_step.rebuild {
                step = policy.rebuild();
            }
        }
        if let Some(event) = step.emit {
            send(&sup, event);
        }
        if step.teardown {
            sup.native = None; // stops IO and destroys the tap
        }
        if step.create {
            match NativeTap::open(&sup.config, &sup.ctx, sup.check_consent) {
                Ok(native) => {
                    let bundle_form = native.bundle_form;
                    sup.native = Some(native);
                    policy.created();
                    let (now, fresh) = (Instant::now(), counters.snapshot());
                    watchdog.reset(now, fresh.callbacks);
                    zero.restart_window(now, zero_observation(false, &fresh));
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

fn zero_observation(output_running: bool, stats: &TapStats) -> ZeroObservation {
    ZeroObservation { output_running, callbacks: stats.callbacks, nonzero_callbacks: stats.nonzero_callbacks }
}

fn send(sup: &Supervisor, event: TapEvent) {
    // The receiver is gone only when the tap handle is being dropped.
    let _ = sup.events.send(event);
}
