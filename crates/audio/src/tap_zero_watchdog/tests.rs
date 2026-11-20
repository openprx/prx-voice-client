//! Tests for the digital-zero watchdog with an injected clock.

use super::*;

const T: Duration = Duration::from_secs(3);

fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

/// Observation of a tap that has delivered `callbacks` callbacks, `nonzero` of them with signal.
fn obs(output_running: bool, callbacks: u64, nonzero: u64) -> ZeroObservation {
    ZeroObservation { output_running, callbacks, nonzero_callbacks: nonzero }
}

fn dog(t0: Instant) -> ZeroWatchdog {
    ZeroWatchdog::new(T, "net.whatsapp.WhatsApp", t0, obs(true, 0, 0))
}

/// Poll every 250 ms from `from_ms` to `to_ms` inclusive with zeros delivered
/// (callbacks growing, no non-zero), returning the first non-empty step.
fn poll_zeros(w: &mut ZeroWatchdog, t0: Instant, from_ms: u64, to_ms: u64, running: bool) -> Option<(u64, ZeroStep)> {
    let mut ms = from_ms;
    while ms <= to_ms {
        let step = w.step(at(t0, ms), obs(running, ms / 10, 0));
        if step != ZeroStep::default() {
            return Some((ms, step));
        }
        ms += 250;
    }
    None
}

#[test]
fn zeros_with_output_running_fire_at_the_threshold() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(w.step(at(t0, 2_999), obs(true, 300, 0)), ZeroStep::default());
    let step = w.step(at(t0, 3_000), obs(true, 301, 0));
    assert_eq!(step.emit, Some(TapEvent::SilentWhilePlaying { silent_ms: 3_000 }));
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
fn a_single_nonzero_callback_resets_the_window() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(poll_zeros(&mut w, t0, 250, 2_500, true), None);
    // One callback with signal at 2.75 s: the window starts again.
    assert_eq!(w.step(at(t0, 2_750), obs(true, 275, 1)), ZeroStep::default());
    assert_eq!(w.step(at(t0, 5_749), obs(true, 575, 1)), ZeroStep::default());
    let step = w.step(at(t0, 5_750), obs(true, 576, 1));
    assert!(matches!(step.emit, Some(TapEvent::SilentWhilePlaying { .. })));
}

#[test]
fn output_that_stops_and_restarts_needs_a_fresh_continuous_window() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(poll_zeros(&mut w, t0, 250, 2_000, true), None);
    // Output stops for one poll: pauses between utterances of an app that
    // closes its stream do not add up.
    assert_eq!(w.step(at(t0, 2_250), obs(false, 225, 0)), ZeroStep::default());
    assert_eq!(w.step(at(t0, 4_000), obs(true, 400, 0)), ZeroStep::default());
    assert_eq!(w.step(at(t0, 5_249), obs(true, 525, 0)), ZeroStep::default());
    assert!(w.step(at(t0, 5_250), obs(true, 526, 0)).rebuild);
}

#[test]
fn rebuild_is_requested_once_and_the_error_state_follows_a_second_window() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    let (at_ms, first) = poll_zeros(&mut w, t0, 250, 10_000, true).expect("fires");
    assert_eq!(at_ms, 3_000);
    assert!(first.rebuild);

    // The supervisor rebuilt the tap: counters restart at zero.
    let rebuilt_at = at(t0, 3_100);
    w.restart_window(rebuilt_at, obs(true, 0, 0));
    assert!(!w.is_failed());
    assert_eq!(w.step(at(t0, 6_099), obs(true, 300, 0)), ZeroStep::default());
    let step = w.step(at(t0, 6_100), obs(true, 301, 0));
    assert!(!step.rebuild, "the rebuild happens only once");
    assert_eq!(
        step.emit,
        Some(TapEvent::NoSignal { error: TapError::NoSignal { target: "net.whatsapp.WhatsApp".into() } })
    );
    assert!(w.is_failed());

    // Persisting zeros report nothing more.
    assert_eq!(poll_zeros(&mut w, t0, 6_350, 600_000, true), None);
}

#[test]
fn signal_after_the_report_clears_it_and_allows_a_new_cycle() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    w.step(at(t0, 3_000), obs(true, 300, 0));
    w.restart_window(at(t0, 3_100), obs(true, 0, 0));
    w.step(at(t0, 6_100), obs(true, 300, 0));
    assert!(w.is_failed());

    let step = w.step(at(t0, 7_000), obs(true, 400, 5));
    assert_eq!(step, ZeroStep { emit: Some(TapEvent::SignalRestored), rebuild: false });
    assert!(!w.is_failed());

    // A later silence starts the cycle again, with a fresh rebuild.
    assert_eq!(w.step(at(t0, 9_999), obs(true, 700, 5)), ZeroStep::default());
    assert!(w.step(at(t0, 10_000), obs(true, 701, 5)).rebuild);
}

#[test]
fn signal_before_any_report_emits_nothing() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(w.step(at(t0, 1_000), obs(true, 100, 3)), ZeroStep::default());
}

#[test]
fn signal_after_the_first_report_but_before_the_second_clears_it() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    w.step(at(t0, 3_000), obs(true, 300, 0));
    w.restart_window(at(t0, 3_100), obs(true, 0, 0));
    let step = w.step(at(t0, 4_000), obs(true, 90, 2));
    assert_eq!(step.emit, Some(TapEvent::SignalRestored));
    // After recovery the budget of one rebuild is available again.
    assert!(w.step(at(t0, 7_000), obs(true, 400, 2)).rebuild);
}

#[test]
fn a_tap_that_delivers_no_callbacks_is_left_to_the_stall_watchdog() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    assert_eq!(w.step(at(t0, 30_000), obs(true, 0, 0)), ZeroStep::default());
}

#[test]
fn restart_window_after_a_stall_rebuild_keeps_what_was_reported() {
    let t0 = Instant::now();
    let mut w = dog(t0);
    w.step(at(t0, 3_000), obs(true, 300, 0)); // first report, rebuild requested
    // A stall rebuild (not ours) restarts the window; the phase stays Rebuilt.
    w.restart_window(at(t0, 4_000), obs(true, 0, 0));
    assert!(!w.step(at(t0, 7_000), obs(true, 300, 0)).rebuild);
    assert!(w.is_failed());
}
