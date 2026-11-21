//! Tap supervisor thread (macOS): polls the tap's health on a normal thread, reports changes as events and re-creates the tap when needed.
//!
//! Decisions come from [`crate::tap_health`] (stall, target gone, exact zeros while no
//! signal was ever proven); this file only gathers the observations (callback
//! counters, whether the target runs and plays) and carries out the resulting steps.
//! It is never on a real-time thread.
//!
//! **What a re-creation interrupts.** Only the capture of the tapped application: the
//! old tap, its aggregate device and IO proc are stopped and destroyed, then a new set
//! is created and started, all on this thread. The incoming pump's mixer and sink keep
//! running on the pump thread (the original lane fills the missing capture time with
//! silence by timestamp), and the outgoing path shares nothing with the tap. The time
//! without capture is measured and kept ([`crate::tap_incidents`]). With a
//! muted-when-tapped tap the application is audible directly while no tap exists, so a
//! re-creation can let the original through for that time. What remains: the new tap
//! cannot be created before the old one is destroyed, because both would write into
//! the one single-producer ring through the same callback context; overlapping them
//! needs a ring and context per tap. A re-creation therefore happens only for a real
//! failure (stall, vanished target, a first window of exact zeros without any proven
//! signal), never for silence after signal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::call_activity::native::target_output_running;
use crate::core_audio_tap::{NativeTap, TapCtx, target_running};
use crate::tap_health::{Facts, TapHealth};
use crate::tap_incidents::TapIncidents;
use crate::tap_session_proof::{consent_proven, mark_consent_proven};
use crate::tap_types::{TapConfig, TapError, TapEvent};

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
    /// Re-creation counters shared with the owner of the tap.
    pub(crate) incidents: Arc<TapIncidents>,
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
    let mut health = TapHealth::new(
        Instant::now(),
        sup.config.target.describe(),
        &counters.snapshot(),
        consent_proven(),
        Arc::clone(&sup.incidents),
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
        let target_running = target_running(&sup.config.target);
        let facts = Facts {
            target_running,
            native_restore: sup.native.as_ref().is_some_and(|n| n.bundle_form),
            output_running: target_running && target_output_running(&sup.config.target),
            stats,
        };
        let actions = health.poll(Instant::now(), &facts);
        for event in actions.events {
            send(&sup, event);
        }
        if health.signal_proven() {
            mark_consent_proven();
        }
        if actions.teardown {
            sup.native = None; // stops IO and destroys the tap
        }
        if actions.create {
            match NativeTap::open(&sup.config, &sup.ctx, sup.check_consent) {
                Ok(native) => {
                    let bundle_form = native.bundle_form;
                    sup.native = Some(native);
                    health.created(Instant::now(), &counters.snapshot());
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
