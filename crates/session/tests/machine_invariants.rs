//! Invariants of the machines over seeded random event sequences: no output for dead segments, ordered release, announced output changes, and Stop from anywhere.

mod support;

use std::collections::HashSet;

use prxvc_media::{ClauseId, SegmentId};
use prxvc_session::{
    CallEvent, CallState, CancelAction, ClauseEventKind as K, ClauseState, DegradeReason, DirSet,
    DirectionEvent, DirectionState, Endpoint, FaultKind, OutputPolicy, Passthrough, ProviderRole,
    SegmentEvent, Stage,
};
use support::{BOTH, IN, OUT, Rig, Rng};

const SEEDS: u64 = 250;
const STEPS: usize = 300;

fn random_policy(rng: &mut Rng) -> OutputPolicy {
    let p = |rng: &mut Rng| {
        if rng.below(2) == 0 {
            Passthrough::Original
        } else {
            Passthrough::Silence
        }
    };
    OutputPolicy {
        when_paused: p(rng),
        on_failure: p(rng),
    }
}

fn random_dirset(rng: &mut Rng) -> DirSet {
    rng.pick(&[DirSet::one(OUT), DirSet::one(IN), DirSet::BOTH])
}

fn random_call_event(rng: &mut Rng) -> CallEvent {
    let endpoint = rng.pick(&[
        Endpoint::LocalInput,
        Endpoint::RemoteInput,
        Endpoint::LocalOutput,
        Endpoint::RemoteOutput,
    ]);
    let d = rng.pick(&BOTH);
    match rng.below(21) {
        0 | 1 => CallEvent::StartRequested {
            target: None,
            translate: rng.below(4) != 0,
        },
        2 => CallEvent::PreflightFailed(support::Rig::blocker()),
        3 | 4 => CallEvent::AudioRunning,
        5 => CallEvent::EndpointFailed {
            endpoint,
            fault: FaultKind::Crashed,
        },
        6 => CallEvent::RoutingFailed(FaultKind::Rejected),
        7 | 8 => CallEvent::ProvidersReady,
        9 => CallEvent::ProvidersFailed {
            role: ProviderRole::Tts,
            fault: FaultKind::Unavailable,
        },
        10 => CallEvent::DirectionDegraded {
            direction: d,
            reason: DegradeReason::BacklogSkipping,
        },
        11 => CallEvent::DirectionFailed {
            direction: d,
            stage: Stage::Mt,
            fault: FaultKind::Timeout,
        },
        12 => CallEvent::DirectionHealthy(d),
        13 => CallEvent::PauseRequested(random_dirset(rng)),
        14 => CallEvent::ResumeRequested,
        15 => CallEvent::EndpointLost(endpoint),
        16 => CallEvent::SystemWillSleep,
        17 => CallEvent::SystemDidWake,
        18 => CallEvent::EndpointsRestored,
        19 => CallEvent::RecoveryTimeout { missing: endpoint },
        _ => match rng.below(3) {
            0 => CallEvent::Stopped,
            1 => CallEvent::StopTimeout,
            _ => CallEvent::StopRequested,
        },
    }
}

fn random_dir_event(rng: &mut Rng) -> DirectionEvent {
    let reason = rng.pick(&[
        DegradeReason::FallbackVoice,
        DegradeReason::MtFallback,
        DegradeReason::CaptureOverrun,
    ]);
    match rng.below(11) {
        0 => DirectionEvent::Start,
        1 | 2 => DirectionEvent::StagesReady,
        3 => DirectionEvent::StageFailed {
            stage: Stage::Asr,
            fault: FaultKind::Crashed,
        },
        4 => DirectionEvent::StageDegraded(reason),
        5 => DirectionEvent::ReasonCleared(reason),
        6 => DirectionEvent::Pause,
        7 => DirectionEvent::Resume,
        8 => DirectionEvent::Suspend,
        9 => DirectionEvent::Retry,
        _ => match rng.below(2) {
            0 => DirectionEvent::Stop,
            _ => DirectionEvent::Joined,
        },
    }
}

/// The next legal lifecycle step of a clause, with a random choice where there are two.
fn forward_kind(state: ClauseState, rng: &mut Rng) -> K {
    match state {
        ClauseState::Committed => K::Recognized,
        ClauseState::Recognized => K::TranslationStarted,
        ClauseState::Translating => K::Translated,
        ClauseState::Translated => K::SynthesisStarted,
        ClauseState::Synthesizing if rng.below(2) == 0 => K::AudioReady,
        ClauseState::Synthesizing => K::PlaybackStarted,
        ClauseState::Playing => K::PlaybackFinished,
        _ => K::Recognized,
    }
}

fn random_kind(rng: &mut Rng) -> K {
    rng.pick(&[
        K::Recognized,
        K::TranslationStarted,
        K::Translated,
        K::SynthesisStarted,
        K::AudioReady,
        K::PlaybackStarted,
        K::PlaybackFinished,
        K::Failed(Stage::Tts),
        K::Skip,
        K::Cancel,
    ])
}

fn random_segment(rng: &mut Rng, d: prxvc_media::Direction) -> SegmentId {
    SegmentId::new(d, 1 + rng.below(6) as u64).expect("seq >= 1")
}

fn segment_step(rig: &mut Rig, rng: &mut Rng) {
    let d = rng.pick(&BOTH);
    let views = rig.book(d).clause_views();
    match rng.below(20) {
        0..=2 => {
            let _ = rig.try_speech_start(d);
        }
        3 | 4 => {
            let segment = random_segment(rng, d);
            let revision = rng.below(6) as u32;
            let _ = rig.seg_event(d, SegmentEvent::Hypothesis { segment, revision });
        }
        5..=7 => {
            let segment = random_segment(rng, d);
            let _ = rig.seg_event(
                d,
                SegmentEvent::CommitClause {
                    segment,
                    revision: 1,
                    source_end: rig.now,
                },
            );
        }
        8 | 9 => {
            let segment = random_segment(rng, d);
            let _ = rig.seg_event(d, SegmentEvent::EndSegment(segment));
        }
        10 => {
            let c = match rng.below(3) {
                0 => CancelAction::CancelDirection(d),
                1 => CancelAction::CancelSegment(random_segment(rng, d)),
                _ => CancelAction::CancelClause(ClauseId::new(
                    random_segment(rng, d),
                    rng.below(3) as u16,
                )),
            };
            let _ = rig.seg_event(d, SegmentEvent::Cancel(c));
        }
        _ if views.is_empty() => {}
        _ => {
            let v = views[rng.below(views.len())];
            let kind = if rng.below(5) == 0 {
                random_kind(rng)
            } else {
                forward_kind(v.state, rng)
            };
            let _ = rig.seg_event(
                d,
                SegmentEvent::Clause {
                    clause: v.clause,
                    kind,
                },
            );
        }
    }
}

fn step(rig: &mut Rig, rng: &mut Rng) {
    rig.advance_ms(rng.below(2_500) as u64);
    // Mostly segment traffic, with the machines' own events interleaved.
    match rng.below(8) {
        0 => {
            let ev = random_call_event(rng);
            let before = rig.call.clone();
            if let Err(rejected) = rig.call_event(ev.clone()) {
                assert_eq!(rejected.from, before);
                assert_eq!(
                    rig.call, before,
                    "a rejection must not change the call state"
                );
            }
        }
        1 => {
            let d = rng.pick(&BOTH);
            let ev = random_dir_event(rng);
            let before = rig.dir_state(d).clone();
            if let Err(rejected) = rig.dir_event(d, ev) {
                assert_eq!(rejected.from, before);
                assert_eq!(
                    rig.dir_state(d),
                    &before,
                    "a rejection must not change the direction state"
                );
            }
        }
        _ => segment_step(rig, rng),
    }
}

fn walk(seed: u64) -> Rig {
    let mut rng = Rng::new(seed);
    let (out, inc) = (random_policy(&mut rng), random_policy(&mut rng));
    let mut rig = Rig::new(out, inc);
    // Half of the walks begin with a running call so that the deep states are reached.
    if seed.is_multiple_of(2) {
        rig.start_translating();
    }
    for _ in 0..STEPS {
        step(&mut rig, &mut rng);
    }
    rig
}

#[test]
fn random_sequences_never_break_ordering_late_drop_or_announcement_invariants() {
    let mut releases = 0;
    let mut late = 0;
    let mut changes = 0;
    let mut call_variants = HashSet::new();
    for seed in 0..SEEDS {
        // The rig asserts the invariants on every step; a failure names the seed below.
        let rig = std::panic::catch_unwind(|| walk(seed))
            .unwrap_or_else(|_| panic!("invariant broken with seed {seed}"));
        releases += rig.released.iter().map(Vec::len).sum::<usize>();
        late += rig.books.iter().map(|b| b.late_dropped()).sum::<u64>();
        changes += BOTH
            .iter()
            .map(|d| rig.announcements(*d).len())
            .sum::<usize>();
        call_variants.insert(std::mem::discriminant(&rig.call));
    }
    // The walks must be non-vacuous.
    assert!(
        releases > 100,
        "only {releases} clauses were released over all walks"
    );
    assert!(
        late > 100,
        "only {late} late results were seen over all walks"
    );
    assert!(
        changes > 500,
        "only {changes} output changes were announced over all walks"
    );
    assert!(
        call_variants.len() >= 4,
        "walks ended in only {} call states",
        call_variants.len()
    );
}

#[test]
fn stop_from_every_reachable_state_leads_to_idle_through_stopping_with_all_segments_cancelled() {
    let mut stopped = 0;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed ^ 0xABCD);
        let mut rig = Rig::new(random_policy(&mut rng), random_policy(&mut rng));
        if seed.is_multiple_of(2) {
            rig.start_translating();
        }
        for i in 0..STEPS {
            step(&mut rig, &mut rng);
            if !i.is_multiple_of(25) {
                continue;
            }
            check_stop(&mut rig.clone(), &mut stopped);
        }
    }
    assert!(
        stopped > 500,
        "the probes covered too few stoppable states: {stopped}"
    );
}

fn check_stop(rig: &mut Rig, stopped: &mut usize) {
    let before = rig.call.clone();
    match rig.call_event(CallEvent::StopRequested) {
        Err(rejected) => {
            assert!(
                matches!(before, CallState::Idle | CallState::Stopping),
                "Stop rejected in {before:?}"
            );
            assert_eq!(rejected.from, before);
        }
        Ok(()) => {
            assert_eq!(
                rig.call,
                CallState::Stopping,
                "Stop from {before:?} must enter Stopping"
            );
            for d in BOTH {
                assert_eq!(
                    rig.dir_state(d),
                    &DirectionState::Off,
                    "{d:?} after stop from {before:?}"
                );
                assert!(
                    rig.book(d).clause_views().is_empty(),
                    "{d:?}: segments survive a stop from {before:?}"
                );
                assert_eq!(rig.book(d).activity(), prxvc_session::Activity::Listening);
            }
            rig.call_event(CallEvent::Stopped)
                .expect("Stopping accepts Stopped");
            assert_eq!(rig.call, CallState::Idle);
            *stopped += 1;
        }
    }
}

#[test]
fn a_direction_stops_from_every_state_it_can_be_in() {
    let mut seen = HashSet::new();
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed ^ 0x5151);
        let mut rig = Rig::new(random_policy(&mut rng), random_policy(&mut rng));
        for _ in 0..STEPS {
            let d = rng.pick(&BOTH);
            let ev = random_dir_event(&mut rng);
            let _ = rig.dir_event(d, ev);
            let state = rig.dir_state(d).clone();
            seen.insert(std::mem::discriminant(&state));
            let mut probe = Rig::new(rig.cfgs[0].policy, rig.cfgs[1].policy);
            probe.dirs[support::ix(d)] = state.clone();
            probe.heard[support::ix(d)] = state.output(&probe.cfgs[support::ix(d)].policy);
            match (&state, probe.dir_event(d, DirectionEvent::Stop)) {
                (DirectionState::Off, Err(_)) => {}
                (s, Ok(())) => {
                    assert!(
                        matches!(probe.dir_state(d), DirectionState::Stopping { .. }),
                        "Stop from {s:?}"
                    );
                    probe.dir_event(d, DirectionEvent::Joined).expect("joined");
                    assert_eq!(probe.dir_state(d), &DirectionState::Off);
                }
                (s, Err(e)) => panic!("Stop must be legal in {s:?}: {e}"),
            }
        }
    }
    assert_eq!(seen.len(), 8, "the walks must reach every direction state");
}
