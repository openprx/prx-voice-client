//! Tests for the digital-zero watchdog with an injected clock.

use super::*;

const T: Duration = Duration::from_secs(15);
const T_MS: u64 = 15_000;

fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

/// Observation of a tap that has delivered `callbacks` callbacks, `nonzero` of them with signal.
fn obs(output_running: bool, callbacks: u64, nonzero: u64) -> ZeroObservation {
    ZeroObservation {
        output_running,
        callbacks,
        nonzero_callbacks: nonzero,
    }
}

/// A watchdog with no signal proven.
fn dog(t0: Instant) -> ZeroWatchdog {
    ZeroWatchdog::new(T, "net.whatsapp.WhatsApp", t0, obs(true, 0, 0), false)
}

/// Poll every 250 ms from `from_ms` to `to_ms` inclusive with zeros delivered
/// (callbacks growing, no non-zero), returning the first non-empty step.
fn poll_zeros(
    w: &mut ZeroWatchdog,
    t0: Instant,
    from_ms: u64,
    to_ms: u64,
    running: bool,
) -> Option<(u64, ZeroStep)> {
    let mut ms = from_ms;
    while ms <= to_ms {
        let step = w.step(at(t0, ms), obs(running, ms / 10, w.last_nonzero));
        if step != ZeroStep::default() {
            return Some((ms, step));
        }
        ms += 250;
    }
    None
}

#[test]
fn zeros_with_output_running_fire_at_the_window_when_no_signal_was_ever_proven() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(
        w.step(at(t0, T_MS - 1), obs(true, 300, 0)),
        ZeroStep::default()
    );
    let step = w.step(at(t0, T_MS), obs(true, 301, 0));
    assert_eq!(
        step.emit,
        Some(TapEvent::SilentWhilePlaying { silent_ms: T_MS })
    );
    assert!(step.rebuild);
}

#[test]
fn zeros_with_output_not_running_never_fire() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(poll_zeros(&mut w, t0, 250, 600_000, false), None);
    assert!(!w.is_failed());
}

#[test]
fn silence_after_proven_signal_never_rebuilds_and_never_reports() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    // One callback with signal at 2 s proves the consent.
    assert_eq!(
        w.step(at(t0, 2_000), obs(true, 200, 1)),
        ZeroStep::default()
    );
    assert!(w.is_proven());
    // The other side is silent for 10 minutes while the output runs.
    assert_eq!(poll_zeros(&mut w, t0, 2_250, 600_000, true), None);
    assert!(!w.is_failed());
}

#[test]
fn a_watchdog_started_with_proof_from_an_earlier_tap_never_fires() {
    let t0 = Instant::now();
    let mut w = ZeroWatchdog::new(T, "x", t0, obs(true, 0, 0), true);
    assert!(w.is_proven());
    assert_eq!(poll_zeros(&mut w, t0, 250, 600_000, true), None);
}

#[test]
fn counters_that_already_hold_signal_count_as_proof() {
    let t0 = Instant::now();
    let w = ZeroWatchdog::new(T, "x", t0, obs(true, 500, 7), false);
    assert!(w.is_proven());
}

#[test]
fn a_single_nonzero_callback_before_the_window_ends_proves_consent_for_good() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(poll_zeros(&mut w, t0, 250, T_MS - 500, true), None);
    assert_eq!(
        w.step(at(t0, T_MS - 250), obs(true, 1_450, 1)),
        ZeroStep::default()
    );
    assert_eq!(poll_zeros(&mut w, t0, T_MS, 600_000, true), None);
}

#[test]
fn output_that_stops_and_restarts_needs_a_fresh_continuous_window() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(poll_zeros(&mut w, t0, 250, T_MS - 1_000, true), None);
    // Output stops for one poll: pauses between utterances of an app that
    // closes its stream do not add up.
    assert_eq!(
        w.step(at(t0, 14_250), obs(false, 1_425, 0)),
        ZeroStep::default()
    );
    assert_eq!(
        w.step(at(t0, 16_000), obs(true, 1_600, 0)),
        ZeroStep::default()
    );
    // The window counts from the poll that saw the output stop.
    assert_eq!(
        w.step(at(t0, 14_250 + T_MS - 1), obs(true, 3_100, 0)),
        ZeroStep::default()
    );
    assert!(w.step(at(t0, 14_250 + T_MS), obs(true, 3_101, 0)).rebuild);
}

#[test]
fn rebuild_is_requested_once_and_the_error_state_follows_a_second_window() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    let (at_ms, first) = poll_zeros(&mut w, t0, 250, 60_000, true).expect("fires");
    assert_eq!(at_ms, T_MS);
    assert!(first.rebuild);

    // The supervisor rebuilt the tap.
    let rebuilt_at = at(t0, T_MS + 100);
    w.restart_window(rebuilt_at, obs(true, 1_500, 0));
    assert!(!w.is_failed());
    assert_eq!(
        w.step(at(t0, 2 * T_MS + 99), obs(true, 3_000, 0)),
        ZeroStep::default()
    );
    let step = w.step(at(t0, 2 * T_MS + 100), obs(true, 3_001, 0));
    assert!(!step.rebuild, "the rebuild happens only once");
    assert_eq!(
        step.emit,
        Some(TapEvent::NoSignal {
            error: TapError::NoSignal {
                target: "net.whatsapp.WhatsApp".into()
            }
        })
    );
    assert!(w.is_failed());

    // Persisting zeros report and rebuild nothing more.
    assert_eq!(
        poll_zeros(&mut w, t0, 2 * T_MS + 350, 6_000_000, true),
        None
    );
}

#[test]
fn signal_after_the_report_clears_it_and_proves_the_consent() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    w.step(at(t0, T_MS), obs(true, 1_500, 0));
    w.restart_window(at(t0, T_MS + 100), obs(true, 1_500, 0));
    w.step(at(t0, 2 * T_MS + 100), obs(true, 3_000, 0));
    assert!(w.is_failed());

    let step = w.step(at(t0, 2 * T_MS + 1_000), obs(true, 3_100, 5));
    assert_eq!(
        step,
        ZeroStep {
            emit: Some(TapEvent::SignalRestored),
            rebuild: false
        }
    );
    assert!(!w.is_failed() && w.is_proven());

    // Later silence is the other side being quiet: no cycle starts again.
    assert_eq!(
        poll_zeros(&mut w, t0, 2 * T_MS + 1_250, 600_000, true),
        None
    );
}

#[test]
fn signal_before_any_report_emits_nothing() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(
        w.step(at(t0, 1_000), obs(true, 100, 3)),
        ZeroStep::default()
    );
}

#[test]
fn signal_after_the_first_report_but_before_the_second_clears_it() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    w.step(at(t0, T_MS), obs(true, 1_500, 0));
    w.restart_window(at(t0, T_MS + 100), obs(true, 1_500, 0));
    let step = w.step(at(t0, T_MS + 1_000), obs(true, 1_590, 2));
    assert_eq!(step.emit, Some(TapEvent::SignalRestored));
    assert_eq!(poll_zeros(&mut w, t0, T_MS + 1_250, 600_000, true), None);
}

#[test]
fn a_tap_that_delivers_no_callbacks_is_left_to_the_stall_watchdog() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(
        w.step(at(t0, 300_000), obs(true, 0, 0)),
        ZeroStep::default()
    );
}

#[test]
fn restart_window_after_a_stall_rebuild_keeps_what_was_reported() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    w.step(at(t0, T_MS), obs(true, 1_500, 0)); // first report, rebuild requested
    // A stall rebuild (not ours) restarts the window; the phase stays Rebuilt.
    w.restart_window(at(t0, T_MS + 1_000), obs(true, 1_500, 0));
    assert!(
        !w.step(at(t0, 2 * T_MS + 1_000), obs(true, 3_000, 0))
            .rebuild
    );
    assert!(w.is_failed());
}
