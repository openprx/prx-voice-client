//! Scenarios of the tap's health policy against a fake tap and an injected clock.

use super::*;
use crate::tap_incidents::TapIncidentCounts;

const POLL: Duration = Duration::from_millis(250);

/// A tap that is not Core Audio: counters the test moves, a creation time and an
/// optional number of failing creations.
struct Rig {
    health: TapHealth,
    incidents: Arc<TapIncidents>,
    t0: Instant,
    now: Instant,
    stats: TapStats,
    output_running: bool,
    target_running: bool,
    create_latency: Duration,
    create_failures: u32,
    events: Vec<TapEvent>,
    teardowns: u32,
    creates: u32,
}

impl Rig {
    fn new(proven: bool) -> Self {
        let t0 = Instant::now();
        let incidents = Arc::new(TapIncidents::default());
        let stats = TapStats::default();
        Self {
            health: TapHealth::new(t0, "net.whatsapp.WhatsApp".into(), &stats, proven, Arc::clone(&incidents)),
            incidents,
            t0,
            now: t0,
            stats,
            output_running: true,
            target_running: true,
            create_latency: Duration::from_millis(300),
            create_failures: 0,
            events: Vec::new(),
            teardowns: 0,
            creates: 0,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.now.duration_since(self.t0).as_millis()).unwrap()
    }

    /// One poll: the tap delivers callbacks as the time passes, with or without signal.
    fn poll(&mut self, signal: bool, callbacks_flow: bool) {
        self.now += POLL;
        if callbacks_flow {
            self.stats.callbacks += 25;
            if signal {
                self.stats.nonzero_callbacks += 1;
            }
        }
        let facts = Facts {
            target_running: self.target_running,
            native_restore: false,
            output_running: self.output_running,
            stats: self.stats,
        };
        let actions = self.health.poll(self.now, &facts);
        self.events.extend(actions.events);
        if actions.teardown {
            self.teardowns += 1;
        }
        if actions.create {
            if self.create_failures > 0 {
                self.create_failures -= 1;
                return;
            }
            self.creates += 1;
            self.now += self.create_latency;
            self.health.created(self.now, &self.stats);
        }
    }

    fn run_zeros(&mut self, seconds: u64) {
        for _ in 0..seconds * 4 {
            self.poll(false, true);
        }
    }

    fn counts(&self) -> TapIncidentCounts {
        self.incidents.snapshot()
    }

    fn has(&self, f: impl Fn(&TapEvent) -> bool) -> bool {
        self.events.iter().any(f)
    }
}

fn is_silent_while_playing(e: &TapEvent) -> bool {
    matches!(e, TapEvent::SilentWhilePlaying { .. })
}

fn is_no_signal(e: &TapEvent) -> bool {
    matches!(e, TapEvent::NoSignal { .. })
}

#[test]
fn silence_after_proven_signal_for_a_minute_causes_no_rebuild_and_no_issue() {
    let mut rig = Rig::new(false);
    for _ in 0..8 {
        rig.poll(true, true); // the other side speaks for 2 s
    }
    assert!(rig.health.signal_proven());
    rig.run_zeros(60);
    assert!(rig.events.is_empty(), "{:?}", rig.events);
    assert_eq!((rig.teardowns, rig.creates), (0, 0));
    assert_eq!(rig.counts(), TapIncidentCounts::default());
}

#[test]
fn silence_after_proof_from_an_earlier_tap_of_the_run_causes_nothing() {
    let mut rig = Rig::new(true);
    rig.run_zeros(120);
    assert!(rig.events.is_empty());
    assert_eq!(rig.teardowns, 0);
}

#[test]
fn never_any_signal_raises_the_issue_after_the_window_and_rebuilds_exactly_once() {
    let mut rig = Rig::new(false);
    rig.run_zeros(14);
    assert!(rig.events.is_empty(), "nothing before the window: {:?}", rig.events);
    rig.run_zeros(4);
    assert!(rig.has(is_silent_while_playing));
    assert_eq!((rig.teardowns, rig.creates), (1, 1));
    assert!(!rig.has(is_no_signal), "the issue follows only a second window");
    rig.run_zeros(20);
    assert!(rig.has(is_no_signal));
    // Minutes more of zeros: no further rebuild, no second report.
    rig.run_zeros(300);
    assert_eq!((rig.teardowns, rig.creates), (1, 1));
    assert_eq!(rig.events.iter().filter(|e| is_no_signal(e)).count(), 1);
    assert_eq!(rig.counts().silence, 1);
    assert_eq!(rig.counts().longest_interruption_ms, 300);
}

#[test]
fn zeros_while_the_target_plays_nothing_never_count_as_suspicious() {
    let mut rig = Rig::new(false);
    rig.output_running = false;
    rig.run_zeros(600);
    assert!(rig.events.is_empty());
    assert_eq!(rig.teardowns, 0);
}

#[test]
fn signal_arriving_after_the_issue_clears_it_and_later_silence_is_quiet() {
    let mut rig = Rig::new(false);
    rig.run_zeros(40);
    assert!(rig.has(is_no_signal));
    rig.events.clear();
    rig.poll(true, true);
    assert_eq!(rig.events, vec![TapEvent::SignalRestored]);
    rig.run_zeros(120);
    assert_eq!(rig.events, vec![TapEvent::SignalRestored]);
    assert_eq!(rig.teardowns, 1);
}

#[test]
fn a_stall_rebuilds_and_the_summary_counts_the_reason_and_the_interruption() {
    let mut rig = Rig::new(true);
    rig.poll(true, true);
    // No callbacks for the stall timeout.
    for _ in 0..12 {
        rig.poll(false, false);
    }
    assert!(rig.has(|e| matches!(e, TapEvent::Stalled { .. })));
    let c = rig.counts();
    assert_eq!((c.stall, c.silence, c.target_returned), (1, 0, 0));
    assert_eq!(c.longest_interruption_ms, 300);
}

#[test]
fn a_creation_that_fails_for_a_while_lengthens_the_interruption() {
    let mut rig = Rig::new(true);
    rig.create_failures = 3;
    for _ in 0..12 {
        rig.poll(false, false);
    }
    rig.run_zeros(2);
    let c = rig.counts();
    assert_eq!(c.stall, 1);
    // Teardown, three failed attempts at 250 ms and the creation itself.
    assert_eq!(c.longest_interruption_ms, 3 * 250 + 300);
}

#[test]
fn the_target_quitting_and_returning_is_counted_apart_and_not_as_an_interruption_of_ours() {
    let mut rig = Rig::new(true);
    rig.poll(true, true);
    rig.target_running = false;
    rig.poll(false, true);
    assert_eq!(rig.teardowns, 1);
    for _ in 0..240 {
        rig.poll(false, false); // a minute away
    }
    assert_eq!(rig.counts(), TapIncidentCounts::default());
    rig.target_running = true;
    rig.poll(false, true);
    let c = rig.counts();
    assert_eq!((c.target_returned, c.stall, c.silence), (1, 0, 0));
    assert_eq!(c.longest_interruption_ms, 300, "only the creation, not the absence");
    assert!(rig.elapsed_ms() > 60_000);
}
