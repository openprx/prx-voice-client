//! Exhaustive (state, event) tests of the call machine: the legal table, and every other pair rejected.

use std::collections::HashMap;

use super::*;
use CallReason as R;
use Direction::{Incoming as In, Outgoing as Out};

mod table;

use table::{degraded, events, legal_rows, states};

#[test]
fn every_state_event_pair_is_legal_with_exact_result_or_rejected_unchanged() {
    let table: HashMap<(&str, &str), (CallState, Vec<CallAction>)> = legal_rows()
        .into_iter()
        .map(|(s, e, next, actions)| {
            let key = (s, e);
            (key, (next, actions))
        })
        .collect();
    let (states, events) = (states(), events());
    let known: Vec<&str> = states.iter().map(|(l, _)| *l).collect();
    let known_events: Vec<&str> = events.iter().map(|(l, _)| *l).collect();
    for (s, e) in table.keys() {
        assert!(known.contains(s), "table row names unknown state {s}");
        assert!(
            known_events.contains(e),
            "table row names unknown event {e}"
        );
    }
    let (mut legal, mut rejected) = (0, 0);
    for (sl, state) in &states {
        for (el, event) in &events {
            let result = state.clone().on(event.clone());
            match table.get(&(*sl, *el)) {
                Some((next, actions)) => {
                    let (got_state, got_actions) =
                        result.unwrap_or_else(|r| panic!("{sl} + {el} must be legal, got {r}"));
                    assert_eq!(&got_state, next, "{sl} + {el}: new state");
                    assert_eq!(&got_actions, actions, "{sl} + {el}: actions");
                    legal += 1;
                }
                None => {
                    let rejection = result.expect_err(&format!("{sl} + {el} must be rejected"));
                    assert_eq!(
                        &rejection.from, state,
                        "{sl} + {el}: a rejection returns the state unchanged"
                    );
                    assert_eq!(
                        &rejection.event, event,
                        "{sl} + {el}: a rejection returns the event"
                    );
                    rejected += 1;
                }
            }
        }
    }
    assert_eq!(
        legal,
        table.len(),
        "every table row was exercised exactly once"
    );
    assert_eq!(legal + rejected, states.len() * events.len());
}

/// Position of a state in declaration order; an exhaustive match, so a new variant does not compile until `states()` is updated.
fn variant_index(s: &CallState) -> usize {
    match s {
        S::Idle => 0,
        S::Initializing { .. } => 1,
        S::AudioReady { .. } => 2,
        S::Translating => 3,
        S::Paused { .. } => 4,
        S::Degraded { .. } => 5,
        S::Recovering { .. } => 6,
        S::Stopping => 7,
    }
}

/// Position of an event in declaration order (same purpose).
fn event_index(e: &CallEvent) -> usize {
    match e {
        E::StartRequested { .. } => 0,
        E::PreflightFailed(_) => 1,
        E::AudioRunning => 2,
        E::EndpointFailed { .. } => 3,
        E::RoutingFailed(_) => 4,
        E::ProvidersReady => 5,
        E::ProvidersFailed { .. } => 6,
        E::DirectionDegraded { .. } => 7,
        E::DirectionFailed { .. } => 8,
        E::DirectionHealthy(_) => 9,
        E::PauseRequested(_) => 10,
        E::ResumeRequested => 11,
        E::EndpointLost(_) => 12,
        E::SystemWillSleep => 13,
        E::SystemDidWake => 14,
        E::EndpointsRestored => 15,
        E::RecoveryTimeout { .. } => 16,
        E::StopRequested => 17,
        E::Stopped => 18,
        E::StopTimeout => 19,
    }
}

#[test]
fn the_fixtures_cover_every_state_and_event_variant() {
    let mut seen: Vec<usize> = states().iter().map(|(_, s)| variant_index(s)).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen,
        (0..8).collect::<Vec<_>>(),
        "a state variant has no fixture"
    );
    let mut seen: Vec<usize> = events().iter().map(|(_, e)| event_index(e)).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen,
        (0..20).collect::<Vec<_>>(),
        "an event variant has no fixture"
    );
    let labels: Vec<&str> = states().iter().map(|(l, _)| *l).collect();
    let mut unique = labels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(labels.len(), unique.len(), "state labels are unique");
}

#[test]
fn audio_only_call_never_translates() {
    let (s, _) = S::Idle
        .on(E::StartRequested {
            target: None,
            translate: false,
        })
        .and_then(|(s, _)| s.on(E::AudioRunning))
        .expect("start");
    assert_eq!(s, S::AudioReady { translate: false });
    assert!(s.clone().on(E::ProvidersReady).is_err());
    assert!(
        s.on(E::ProvidersFailed {
            role: ProviderRole::Mt,
            fault: FaultKind::Crashed
        })
        .is_err()
    );
}

#[test]
fn a_degraded_call_that_pauses_resumes_into_degraded_while_a_reason_remains() {
    let d = degraded(&[R::DirectionFailed(Out)]);
    let (p, _) = d.on(E::PauseRequested(DirSet::BOTH)).expect("pause");
    let (r, actions) = p.on(E::ResumeRequested).expect("resume");
    assert_eq!(r, degraded(&[R::DirectionFailed(Out)]));
    assert_eq!(actions, vec![A::ResumeDirections(DirSet::BOTH)]);
}
