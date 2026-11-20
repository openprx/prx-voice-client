//! Exhaustive (state, event) tests of the direction machine under three output policies, and the announcement rule.

use std::collections::HashMap;

use super::*;
use crate::output::Passthrough;
use DegradeReason::{CaptureOverrun as C, FallbackVoice as A_, MtFallback as B_};

const DIR: Direction = Direction::Outgoing;

fn cfg(when_paused: Passthrough, on_failure: Passthrough) -> DirectionConfig {
    DirectionConfig {
        direction: DIR,
        policy: OutputPolicy {
            when_paused,
            on_failure,
        },
    }
}

fn rs(r: &[DegradeReason]) -> Reasons {
    r.iter().copied().collect()
}

fn degraded(r: &[DegradeReason]) -> DirectionState {
    S::Degraded { reasons: rs(r) }
}

fn paused(r: &[DegradeReason]) -> DirectionState {
    S::Paused { reasons: rs(r) }
}

fn suspended(r: &[DegradeReason]) -> DirectionState {
    S::Suspended { reasons: rs(r) }
}

fn failed() -> DirectionState {
    S::Failed {
        stage: Stage::Asr,
        fault: FaultKind::Crashed,
    }
}

fn states() -> Vec<(&'static str, DirectionState)> {
    vec![
        ("Off", S::Off),
        (
            "Starting",
            S::Starting {
                after_failure: false,
            },
        ),
        (
            "Starting/retry",
            S::Starting {
                after_failure: true,
            },
        ),
        ("Running", S::Running),
        ("Degraded/A", degraded(&[A_])),
        ("Degraded/AB", degraded(&[A_, B_])),
        ("Paused/-", paused(&[])),
        ("Paused/A", paused(&[A_])),
        ("Suspended/-", suspended(&[])),
        ("Suspended/A", suspended(&[A_])),
        ("Failed", failed()),
        (
            "Stopping/Original",
            S::Stopping {
                held: OutputSource::Original,
            },
        ),
        (
            "Stopping/Nothing",
            S::Stopping {
                held: OutputSource::Nothing,
            },
        ),
    ]
}

fn events() -> Vec<(&'static str, DirectionEvent)> {
    vec![
        ("Start", E::Start),
        ("StagesReady", E::StagesReady),
        (
            "StageFailed",
            E::StageFailed {
                stage: Stage::Asr,
                fault: FaultKind::Crashed,
            },
        ),
        ("Degrade/A", E::StageDegraded(A_)),
        ("Degrade/C", E::StageDegraded(C)),
        ("Clear/A", E::ReasonCleared(A_)),
        ("Clear/C", E::ReasonCleared(C)),
        ("Pause", E::Pause),
        ("Resume", E::Resume),
        ("Suspend", E::Suspend),
        ("Retry", E::Retry),
        ("Stop", E::Stop),
        ("Joined", E::Joined),
    ]
}

type Row = (
    &'static str,
    &'static str,
    DirectionState,
    Vec<DirectionAction>,
);

/// The announcement the table expects: nothing when the source stays the same.
fn chg(from: OutputSource, to: OutputSource, cause: OutputCause) -> Vec<DirectionAction> {
    if from == to {
        return vec![];
    }
    vec![A::ChangeOutput {
        direction: DIR,
        from,
        to,
        cause,
    }]
}

fn cat(mut a: Vec<DirectionAction>, b: Vec<DirectionAction>) -> Vec<DirectionAction> {
    a.extend(b);
    a
}

fn spawn() -> Vec<DirectionAction> {
    vec![A::SpawnStages, A::OpenAsrStream]
}

fn cancel() -> DirectionAction {
    A::CancelDirection(DIR)
}

fn fail_actions() -> Vec<DirectionAction> {
    vec![
        cancel(),
        A::ReportFailure {
            stage: Stage::Asr,
            fault: FaultKind::Crashed,
        },
    ]
}

/// Part 1 of the legal rows (see `legal_rows`).
fn rows_part_1(p: OutputSource, f: OutputSource) -> Vec<Row> {
    use OutputCause as Cause;
    use OutputSource::{Original as O, Translation as T};
    vec![
        (
            "Off",
            "Start",
            S::Starting {
                after_failure: false,
            },
            spawn(),
        ),
        (
            "Starting",
            "StagesReady",
            S::Running,
            chg(O, T, Cause::TranslationStarted),
        ),
        (
            "Starting",
            "StageFailed",
            failed(),
            cat(fail_actions(), chg(O, f, Cause::Failed)),
        ),
        (
            "Starting/retry",
            "StagesReady",
            S::Running,
            chg(f, T, Cause::TranslationStarted),
        ),
        (
            "Starting/retry",
            "StageFailed",
            failed(),
            cat(fail_actions(), chg(f, f, Cause::Failed)),
        ),
        (
            "Running",
            "StageFailed",
            failed(),
            cat(fail_actions(), chg(T, f, Cause::Failed)),
        ),
        ("Running", "Degrade/A", degraded(&[A_]), vec![A::Report(A_)]),
        ("Running", "Degrade/C", degraded(&[C]), vec![A::Report(C)]),
        (
            "Running",
            "Pause",
            paused(&[]),
            cat(vec![cancel()], chg(T, p, Cause::Paused)),
        ),
        (
            "Running",
            "Suspend",
            suspended(&[]),
            cat(vec![cancel()], chg(T, f, Cause::Suspended)),
        ),
        (
            "Degraded/A",
            "StageFailed",
            failed(),
            cat(fail_actions(), chg(T, f, Cause::Failed)),
        ),
        ("Degraded/A", "Degrade/A", degraded(&[A_]), vec![]),
    ]
}

/// Part 2 of the legal rows (see `legal_rows`).
fn rows_part_2(p: OutputSource, f: OutputSource) -> Vec<Row> {
    use OutputCause as Cause;
    use OutputSource::Translation as T;
    vec![
        (
            "Degraded/A",
            "Degrade/C",
            degraded(&[A_, C]),
            vec![A::Report(C)],
        ),
        ("Degraded/A", "Clear/A", S::Running, vec![]),
        ("Degraded/A", "Clear/C", degraded(&[A_]), vec![]),
        (
            "Degraded/A",
            "Pause",
            paused(&[A_]),
            cat(vec![cancel()], chg(T, p, Cause::Paused)),
        ),
        (
            "Degraded/A",
            "Suspend",
            suspended(&[A_]),
            cat(vec![cancel()], chg(T, f, Cause::Suspended)),
        ),
        (
            "Degraded/AB",
            "StageFailed",
            failed(),
            cat(fail_actions(), chg(T, f, Cause::Failed)),
        ),
        ("Degraded/AB", "Degrade/A", degraded(&[A_, B_]), vec![]),
        (
            "Degraded/AB",
            "Degrade/C",
            degraded(&[A_, B_, C]),
            vec![A::Report(C)],
        ),
        ("Degraded/AB", "Clear/A", degraded(&[B_]), vec![]),
        ("Degraded/AB", "Clear/C", degraded(&[A_, B_]), vec![]),
        (
            "Degraded/AB",
            "Pause",
            paused(&[A_, B_]),
            cat(vec![cancel()], chg(T, p, Cause::Paused)),
        ),
        (
            "Degraded/AB",
            "Suspend",
            suspended(&[A_, B_]),
            cat(vec![cancel()], chg(T, f, Cause::Suspended)),
        ),
    ]
}

/// Part 3 of the legal rows (see `legal_rows`).
fn rows_part_3(p: OutputSource, f: OutputSource) -> Vec<Row> {
    use OutputCause as Cause;
    use OutputSource::Translation as T;
    vec![
        ("Paused/-", "Degrade/A", paused(&[A_]), vec![A::Report(A_)]),
        ("Paused/-", "Degrade/C", paused(&[C]), vec![A::Report(C)]),
        ("Paused/-", "Clear/A", paused(&[]), vec![]),
        ("Paused/-", "Clear/C", paused(&[]), vec![]),
        (
            "Paused/-",
            "Resume",
            S::Running,
            chg(p, T, Cause::TranslationResumed),
        ),
        (
            "Paused/-",
            "Suspend",
            suspended(&[]),
            cat(vec![cancel()], chg(p, f, Cause::Suspended)),
        ),
        ("Paused/A", "Degrade/A", paused(&[A_]), vec![]),
        (
            "Paused/A",
            "Degrade/C",
            paused(&[A_, C]),
            vec![A::Report(C)],
        ),
        ("Paused/A", "Clear/A", paused(&[]), vec![]),
        ("Paused/A", "Clear/C", paused(&[A_]), vec![]),
        (
            "Paused/A",
            "Resume",
            degraded(&[A_]),
            chg(p, T, Cause::TranslationResumed),
        ),
        (
            "Paused/A",
            "Suspend",
            suspended(&[A_]),
            cat(vec![cancel()], chg(p, f, Cause::Suspended)),
        ),
    ]
}

/// Part 4 of the legal rows (see `legal_rows`).
fn rows_part_4(_p: OutputSource, f: OutputSource) -> Vec<Row> {
    use OutputCause as Cause;
    use OutputSource::{Original as O, Translation as T};
    let stop = |held| S::Stopping { held };
    vec![
        (
            "Suspended/-",
            "Degrade/A",
            suspended(&[A_]),
            vec![A::Report(A_)],
        ),
        (
            "Suspended/-",
            "Degrade/C",
            suspended(&[C]),
            vec![A::Report(C)],
        ),
        ("Suspended/-", "Clear/A", suspended(&[]), vec![]),
        ("Suspended/-", "Clear/C", suspended(&[]), vec![]),
        (
            "Suspended/-",
            "Resume",
            S::Running,
            cat(
                vec![A::ResetAsrStream],
                chg(f, T, Cause::TranslationResumed),
            ),
        ),
        ("Suspended/A", "Degrade/A", suspended(&[A_]), vec![]),
        (
            "Suspended/A",
            "Degrade/C",
            suspended(&[A_, C]),
            vec![A::Report(C)],
        ),
        ("Suspended/A", "Clear/A", suspended(&[]), vec![]),
        ("Suspended/A", "Clear/C", suspended(&[A_]), vec![]),
        (
            "Suspended/A",
            "Resume",
            degraded(&[A_]),
            cat(
                vec![A::ResetAsrStream],
                chg(f, T, Cause::TranslationResumed),
            ),
        ),
        (
            "Failed",
            "Retry",
            S::Starting {
                after_failure: true,
            },
            spawn(),
        ),
        ("Stopping/Original", "Stop", stop(O), vec![]),
    ]
}

/// Part 5 of the legal rows (see `legal_rows`).
fn rows_part_5(_p: OutputSource, _f: OutputSource) -> Vec<Row> {
    use OutputCause as Cause;
    use OutputSource::Original as O;
    let stop = |held| S::Stopping { held };
    vec![
        ("Stopping/Original", "Joined", S::Off, vec![]),
        (
            "Stopping/Nothing",
            "Stop",
            stop(OutputSource::Nothing),
            vec![],
        ),
        (
            "Stopping/Nothing",
            "Joined",
            S::Off,
            chg(OutputSource::Nothing, O, Cause::Stopped),
        ),
    ]
}

/// The legal rows of table 3.7 for a policy whose paused source is `p` and failure source is `f`.
fn legal_rows(p: OutputSource, f: OutputSource) -> Vec<Row> {
    use OutputCause as Cause;
    use OutputSource::{Original as O, Translation as T};
    let stop = |held| S::Stopping { held };
    let join = || vec![cancel(), A::JoinStages];
    let held_stop = |from: OutputSource| {
        let held = if from == T { O } else { from };
        (stop(held), cat(join(), chg(from, held, Cause::Stopping)))
    };
    // (state, label of each row that every degraded-like state shares is added below)
    let mut rows: Vec<Row> = rows_part_1(p, f);
    rows.extend(rows_part_2(p, f));
    rows.extend(rows_part_3(p, f));
    rows.extend(rows_part_4(p, f));
    rows.extend(rows_part_5(p, f));
    // Stop from every state that has stages, announcing what the stop does to the output.
    let stop_from: [(&'static str, OutputSource); 10] = [
        ("Starting", O),
        ("Starting/retry", f),
        ("Running", T),
        ("Degraded/A", T),
        ("Degraded/AB", T),
        ("Paused/-", p),
        ("Paused/A", p),
        ("Suspended/-", f),
        ("Suspended/A", f),
        ("Failed", f),
    ];
    for (label, from) in stop_from {
        let (next, actions) = held_stop(from);
        rows.push((label, "Stop", next, actions));
    }
    rows
}

fn run_cross_product(p: Passthrough, f: Passthrough) {
    let table: HashMap<(&str, &str), (DirectionState, Vec<DirectionAction>)> =
        legal_rows(p.source(), f.source())
            .into_iter()
            .map(|(s, e, n, a)| ((s, e), (n, a)))
            .collect();
    let config = cfg(p, f);
    let (states, events) = (states(), events());
    for (s, e) in table.keys() {
        assert!(
            states.iter().any(|(l, _)| l == s),
            "row names unknown state {s}"
        );
        assert!(
            events.iter().any(|(l, _)| l == e),
            "row names unknown event {e}"
        );
    }
    let mut legal = 0;
    for (sl, state) in &states {
        for (el, event) in &events {
            let result = state.clone().on(&config, event.clone());
            match table.get(&(*sl, *el)) {
                Some((next, actions)) => {
                    let (gs, ga) =
                        result.unwrap_or_else(|r| panic!("{sl} + {el} must be legal: {r}"));
                    assert_eq!(&gs, next, "{sl} + {el} ({p:?}/{f:?}): state");
                    assert_eq!(&ga, actions, "{sl} + {el} ({p:?}/{f:?}): actions");
                    legal += 1;
                }
                None => {
                    let r = result.expect_err(&format!("{sl} + {el} must be rejected"));
                    assert_eq!(&r.from, state);
                    assert_eq!(&r.event, event);
                }
            }
        }
    }
    assert_eq!(legal, table.len(), "every row exercised");
}

#[test]
fn cross_product_with_original_fallbacks() {
    run_cross_product(Passthrough::Original, Passthrough::Original);
}

#[test]
fn cross_product_with_silence_fallbacks() {
    run_cross_product(Passthrough::Silence, Passthrough::Silence);
}

#[test]
fn cross_product_with_mixed_fallbacks() {
    run_cross_product(Passthrough::Silence, Passthrough::Original);
    run_cross_product(Passthrough::Original, Passthrough::Silence);
}

fn variant_index(s: &DirectionState) -> usize {
    match s {
        S::Off => 0,
        S::Starting { .. } => 1,
        S::Running => 2,
        S::Paused { .. } => 3,
        S::Suspended { .. } => 4,
        S::Degraded { .. } => 5,
        S::Failed { .. } => 6,
        S::Stopping { .. } => 7,
    }
}

fn event_index(e: &DirectionEvent) -> usize {
    match e {
        E::Start => 0,
        E::StagesReady => 1,
        E::StageFailed { .. } => 2,
        E::StageDegraded(_) => 3,
        E::ReasonCleared(_) => 4,
        E::Pause => 5,
        E::Resume => 6,
        E::Suspend => 7,
        E::Retry => 8,
        E::Stop => 9,
        E::Joined => 10,
    }
}

#[test]
fn fixtures_cover_every_variant() {
    let mut s: Vec<usize> = states().iter().map(|(_, s)| variant_index(s)).collect();
    s.sort_unstable();
    s.dedup();
    assert_eq!(s, (0..8).collect::<Vec<_>>());
    let mut e: Vec<usize> = events().iter().map(|(_, e)| event_index(e)).collect();
    e.sort_unstable();
    e.dedup();
    assert_eq!(e, (0..11).collect::<Vec<_>>());
}

#[test]
fn output_of_each_state_follows_the_policy() {
    let policy = OutputPolicy {
        when_paused: Passthrough::Silence,
        on_failure: Passthrough::Original,
    };
    let out = |s: DirectionState| s.output(&policy);
    assert_eq!(out(S::Off), OutputSource::Original);
    assert_eq!(out(S::Running), OutputSource::Translation);
    assert_eq!(out(degraded(&[A_])), OutputSource::Translation);
    assert_eq!(out(paused(&[])), OutputSource::Nothing);
    assert_eq!(out(suspended(&[])), OutputSource::Original);
    assert_eq!(out(failed()), OutputSource::Original);
}

#[test]
fn a_failure_always_ends_in_a_state_with_a_defined_output_and_the_cancel_covers_this_direction_only()
 {
    let config = cfg(Passthrough::Original, Passthrough::Silence);
    let (s, actions) = S::Running
        .on(
            &config,
            E::StageFailed {
                stage: Stage::Tts,
                fault: FaultKind::Timeout,
            },
        )
        .expect("legal");
    assert_eq!(s.output(&config.policy), OutputSource::Nothing);
    assert!(actions.contains(&A::CancelDirection(DIR)));
    assert!(!actions.contains(&A::CancelDirection(DIR.opposite())));
}
