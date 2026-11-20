//! Fixtures of the call-machine cross-product test: every state, every event and the table of legal rows.

use super::*;

pub(super) const FV: DegradeReason = DegradeReason::FallbackVoice;

pub(super) fn out() -> DirSet {
    DirSet::one(Out)
}

pub(super) fn inc() -> DirSet {
    DirSet::one(In)
}

pub(super) fn dir_deg() -> CallReason {
    R::Direction {
        direction: Out,
        reason: FV,
    }
}

pub(super) fn missing(ep: Endpoint) -> CallReason {
    R::EndpointMissing(ep)
}

pub(super) fn degraded(reasons: &[CallReason]) -> CallState {
    S::Degraded {
        reasons: reasons.iter().copied().collect(),
    }
}

pub(super) fn paused(dirs: DirSet, reasons: &[CallReason]) -> CallState {
    S::Paused {
        dirs,
        reasons: reasons.iter().copied().collect(),
    }
}

pub(super) fn recovering(s: CallState) -> CallState {
    S::Recovering {
        resume_to: Box::new(s),
    }
}

pub(super) fn target() -> Option<BundleId> {
    Some(BundleId::new("net.whatsapp.WhatsApp"))
}

pub(super) fn stop_actions() -> Vec<CallAction> {
    vec![
        A::CancelAll,
        A::StopDirections,
        A::RestoreRouting,
        A::CloseEndpoints,
    ]
}

/// Every state the table has a row for, with a label. A new variant must be added here.
pub(super) fn states() -> Vec<(&'static str, CallState)> {
    use ProviderProgress as P;
    vec![
        ("Idle", S::Idle),
        (
            "Init/NotNeeded",
            S::Initializing {
                providers: P::NotNeeded,
            },
        ),
        (
            "Init/Pending",
            S::Initializing {
                providers: P::Pending,
            },
        ),
        (
            "Init/Ready",
            S::Initializing {
                providers: P::Ready,
            },
        ),
        (
            "Init/Failed",
            S::Initializing {
                providers: P::Failed(ProviderRole::Tts),
            },
        ),
        ("AudioReady/audio-only", S::AudioReady { translate: false }),
        ("AudioReady/translate", S::AudioReady { translate: true }),
        ("Translating", S::Translating),
        ("Degraded/D1", degraded(&[dir_deg()])),
        (
            "Degraded/D2",
            degraded(&[dir_deg(), R::DirectionFailed(In)]),
        ),
        ("Paused/out", paused(out(), &[])),
        ("Paused/both", paused(DirSet::BOTH, &[])),
        (
            "Paused/out+reason",
            paused(out(), &[R::DirectionFailed(In)]),
        ),
        ("Recovering/Translating", recovering(S::Translating)),
        (
            "Recovering/AudioReady",
            recovering(S::AudioReady { translate: true }),
        ),
        ("Recovering/Paused", recovering(paused(out(), &[]))),
        ("Recovering/Degraded", recovering(degraded(&[dir_deg()]))),
        ("Stopping", S::Stopping),
    ]
}

/// Every event with a label.
pub(super) fn events() -> Vec<(&'static str, CallEvent)> {
    vec![
        (
            "Start/translate",
            E::StartRequested {
                target: target(),
                translate: true,
            },
        ),
        (
            "Start/audio-only",
            E::StartRequested {
                target: None,
                translate: false,
            },
        ),
        ("PreflightFailed", E::PreflightFailed(Blocker::NoMicrophone)),
        ("AudioRunning", E::AudioRunning),
        (
            "EndpointFailed",
            E::EndpointFailed {
                endpoint: Endpoint::RemoteInput,
                fault: FaultKind::Crashed,
            },
        ),
        ("RoutingFailed", E::RoutingFailed(FaultKind::Rejected)),
        ("ProvidersReady", E::ProvidersReady),
        (
            "ProvidersFailed",
            E::ProvidersFailed {
                role: ProviderRole::Tts,
                fault: FaultKind::Unavailable,
            },
        ),
        (
            "DirDegraded/out",
            E::DirectionDegraded {
                direction: Out,
                reason: FV,
            },
        ),
        (
            "DirFailed/out",
            E::DirectionFailed {
                direction: Out,
                stage: Stage::Mt,
                fault: FaultKind::Timeout,
            },
        ),
        ("DirHealthy/out", E::DirectionHealthy(Out)),
        ("DirHealthy/in", E::DirectionHealthy(In)),
        ("Pause/out", E::PauseRequested(out())),
        ("Pause/in", E::PauseRequested(inc())),
        ("Resume", E::ResumeRequested),
        (
            "EndpointLost/RemoteOutput",
            E::EndpointLost(Endpoint::RemoteOutput),
        ),
        ("WillSleep", E::SystemWillSleep),
        ("DidWake", E::SystemDidWake),
        ("EndpointsRestored", E::EndpointsRestored),
        (
            "RecoveryTimeout",
            E::RecoveryTimeout {
                missing: Endpoint::LocalOutput,
            },
        ),
        ("Stop", E::StopRequested),
        ("Stopped", E::Stopped),
        ("StopTimeout", E::StopTimeout),
    ]
}

pub(super) type Row = (&'static str, &'static str, CallState, Vec<CallAction>);

pub(super) fn init_rows(label: &'static str) -> Vec<Row> {
    vec![
        (
            label,
            "PreflightFailed",
            S::Idle,
            vec![A::ReportBlocker(Blocker::NoMicrophone)],
        ),
        (
            label,
            "EndpointFailed",
            S::Stopping,
            vec![
                A::RestoreRouting,
                A::CloseEndpoints,
                A::ReportBlocker(Blocker::EndpointFailed(
                    Endpoint::RemoteInput,
                    FaultKind::Crashed,
                )),
            ],
        ),
        (
            label,
            "RoutingFailed",
            S::Stopping,
            vec![
                A::RestoreRouting,
                A::CloseEndpoints,
                A::ReportBlocker(Blocker::RoutingFailed(FaultKind::Rejected)),
            ],
        ),
        (label, "Stop", S::Stopping, stop_actions()),
    ]
}

/// Common rows of the four states that can enter recovery (`from` is the state itself).
pub(super) fn recovery_entry(
    label: &'static str,
    from: CallState,
    suspend: DirSet,
    lost_audible: DirSet,
    sleep_audible: DirSet,
) -> Vec<Row> {
    let build = |audible: DirSet| {
        let mut a = vec![];
        if !suspend.is_empty() {
            a.push(A::SuspendDirections(suspend));
        }
        if !audible.is_empty() {
            a.push(A::ApplyFailurePolicy(audible));
        }
        a
    };
    vec![
        (
            label,
            "EndpointLost/RemoteOutput",
            recovering(from.clone()),
            build(lost_audible),
        ),
        (label, "WillSleep", recovering(from), build(sleep_audible)),
        (label, "Stop", S::Stopping, stop_actions()),
    ]
}

/// Part 1 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_1() -> Vec<Row> {
    use ProviderProgress as P;
    vec![
        (
            "Idle",
            "Start/translate",
            S::Initializing {
                providers: P::Pending,
            },
            vec![A::RunPreflight { target: target() }, A::PrepareProviders],
        ),
        (
            "Idle",
            "Start/audio-only",
            S::Initializing {
                providers: P::NotNeeded,
            },
            vec![A::RunPreflight { target: None }],
        ),
        (
            "Init/NotNeeded",
            "AudioRunning",
            S::AudioReady { translate: false },
            vec![A::Publish],
        ),
        (
            "Init/Pending",
            "AudioRunning",
            S::AudioReady { translate: true },
            vec![A::Publish],
        ),
        (
            "Init/Pending",
            "ProvidersReady",
            S::Initializing {
                providers: P::Ready,
            },
            vec![],
        ),
        (
            "Init/Pending",
            "ProvidersFailed",
            S::Initializing {
                providers: P::Failed(ProviderRole::Tts),
            },
            vec![],
        ),
        (
            "Init/Ready",
            "AudioRunning",
            S::Translating,
            vec![A::Publish, A::StartDirections],
        ),
        (
            "Init/Failed",
            "AudioRunning",
            degraded(&[R::Providers(ProviderRole::Tts)]),
            vec![
                A::Publish,
                A::ApplyFailurePolicy(DirSet::BOTH),
                A::Report(R::Providers(ProviderRole::Tts)),
            ],
        ),
        (
            "AudioReady/translate",
            "ProvidersReady",
            S::Translating,
            vec![A::StartDirections],
        ),
    ]
}

/// Part 2 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_2() -> Vec<Row> {
    vec![
        (
            "AudioReady/translate",
            "ProvidersFailed",
            degraded(&[R::Providers(ProviderRole::Tts)]),
            vec![
                A::ApplyFailurePolicy(DirSet::BOTH),
                A::Report(R::Providers(ProviderRole::Tts)),
            ],
        ),
        (
            "Translating",
            "DirDegraded/out",
            degraded(&[dir_deg()]),
            vec![A::Report(dir_deg())],
        ),
        (
            "Translating",
            "DirFailed/out",
            degraded(&[R::DirectionFailed(Out)]),
            vec![A::ApplyFailurePolicy(out())],
        ),
        (
            "Translating",
            "Pause/out",
            paused(out(), &[]),
            vec![A::PauseDirections(out()), A::ApplyPausePolicy(out())],
        ),
        (
            "Translating",
            "Pause/in",
            paused(inc(), &[]),
            vec![A::PauseDirections(inc()), A::ApplyPausePolicy(inc())],
        ),
        (
            "Degraded/D1",
            "DirDegraded/out",
            degraded(&[dir_deg()]),
            vec![A::Report(dir_deg())],
        ),
        (
            "Degraded/D1",
            "DirFailed/out",
            degraded(&[dir_deg(), R::DirectionFailed(Out)]),
            vec![A::ApplyFailurePolicy(out())],
        ),
        ("Degraded/D1", "DirHealthy/out", S::Translating, vec![]),
        (
            "Degraded/D1",
            "DirHealthy/in",
            degraded(&[dir_deg()]),
            vec![],
        ),
    ]
}

/// Part 3 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_3() -> Vec<Row> {
    vec![
        (
            "Degraded/D1",
            "Pause/out",
            paused(out(), &[dir_deg()]),
            vec![A::PauseDirections(out()), A::ApplyPausePolicy(out())],
        ),
        (
            "Degraded/D1",
            "Pause/in",
            paused(inc(), &[dir_deg()]),
            vec![A::PauseDirections(inc()), A::ApplyPausePolicy(inc())],
        ),
        (
            "Degraded/D2",
            "DirDegraded/out",
            degraded(&[dir_deg(), R::DirectionFailed(In)]),
            vec![A::Report(dir_deg())],
        ),
        (
            "Degraded/D2",
            "DirFailed/out",
            degraded(&[dir_deg(), R::DirectionFailed(In), R::DirectionFailed(Out)]),
            vec![A::ApplyFailurePolicy(out())],
        ),
        (
            "Degraded/D2",
            "DirHealthy/out",
            degraded(&[R::DirectionFailed(In)]),
            vec![],
        ),
        (
            "Degraded/D2",
            "DirHealthy/in",
            degraded(&[dir_deg()]),
            vec![A::RestoreTranslatedOutput(In)],
        ),
        (
            "Degraded/D2",
            "Pause/out",
            paused(out(), &[dir_deg(), R::DirectionFailed(In)]),
            vec![A::PauseDirections(out()), A::ApplyPausePolicy(out())],
        ),
        (
            "Degraded/D2",
            "Pause/in",
            paused(inc(), &[dir_deg(), R::DirectionFailed(In)]),
            vec![A::PauseDirections(inc()), A::ApplyPausePolicy(inc())],
        ),
        (
            "Paused/out",
            "DirDegraded/out",
            paused(out(), &[dir_deg()]),
            vec![A::Report(dir_deg())],
        ),
    ]
}

/// Part 4 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_4() -> Vec<Row> {
    vec![
        (
            "Paused/out",
            "DirFailed/out",
            paused(out(), &[R::DirectionFailed(Out)]),
            vec![A::ApplyFailurePolicy(out())],
        ),
        ("Paused/out", "DirHealthy/out", paused(out(), &[]), vec![]),
        ("Paused/out", "DirHealthy/in", paused(out(), &[]), vec![]),
        ("Paused/out", "Pause/out", paused(out(), &[]), vec![]),
        (
            "Paused/out",
            "Pause/in",
            paused(DirSet::BOTH, &[]),
            vec![A::PauseDirections(inc()), A::ApplyPausePolicy(inc())],
        ),
        (
            "Paused/out",
            "Resume",
            S::Translating,
            vec![A::ResumeDirections(out())],
        ),
        (
            "Paused/both",
            "DirDegraded/out",
            paused(DirSet::BOTH, &[dir_deg()]),
            vec![A::Report(dir_deg())],
        ),
        (
            "Paused/both",
            "DirFailed/out",
            paused(DirSet::BOTH, &[R::DirectionFailed(Out)]),
            vec![A::ApplyFailurePolicy(out())],
        ),
        (
            "Paused/both",
            "DirHealthy/out",
            paused(DirSet::BOTH, &[]),
            vec![],
        ),
    ]
}

/// Part 5 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_5() -> Vec<Row> {
    vec![
        (
            "Paused/both",
            "DirHealthy/in",
            paused(DirSet::BOTH, &[]),
            vec![],
        ),
        (
            "Paused/both",
            "Pause/out",
            paused(DirSet::BOTH, &[]),
            vec![],
        ),
        ("Paused/both", "Pause/in", paused(DirSet::BOTH, &[]), vec![]),
        (
            "Paused/both",
            "Resume",
            S::Translating,
            vec![A::ResumeDirections(DirSet::BOTH)],
        ),
        (
            "Paused/out+reason",
            "DirDegraded/out",
            paused(out(), &[dir_deg(), R::DirectionFailed(In)]),
            vec![A::Report(dir_deg())],
        ),
        (
            "Paused/out+reason",
            "DirFailed/out",
            paused(out(), &[R::DirectionFailed(In), R::DirectionFailed(Out)]),
            vec![A::ApplyFailurePolicy(out())],
        ),
        (
            "Paused/out+reason",
            "DirHealthy/out",
            paused(out(), &[R::DirectionFailed(In)]),
            vec![],
        ),
        (
            "Paused/out+reason",
            "DirHealthy/in",
            paused(out(), &[]),
            vec![A::RestoreTranslatedOutput(In)],
        ),
        (
            "Paused/out+reason",
            "Pause/out",
            paused(out(), &[R::DirectionFailed(In)]),
            vec![],
        ),
    ]
}

/// Part 6 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_6() -> Vec<Row> {
    vec![
        (
            "Paused/out+reason",
            "Pause/in",
            paused(DirSet::BOTH, &[R::DirectionFailed(In)]),
            vec![A::PauseDirections(inc()), A::ApplyPausePolicy(inc())],
        ),
        (
            "Paused/out+reason",
            "Resume",
            degraded(&[R::DirectionFailed(In)]),
            vec![A::ResumeDirections(out())],
        ),
        (
            "Recovering/Translating",
            "DidWake",
            recovering(S::Translating),
            vec![A::ReopenEndpoints],
        ),
        (
            "Recovering/Translating",
            "EndpointsRestored",
            S::Translating,
            vec![A::ResumeDirections(DirSet::BOTH)],
        ),
        (
            "Recovering/Translating",
            "RecoveryTimeout",
            degraded(&[missing(Endpoint::LocalOutput)]),
            vec![A::Report(missing(Endpoint::LocalOutput))],
        ),
        (
            "Recovering/AudioReady",
            "DidWake",
            recovering(S::AudioReady { translate: true }),
            vec![A::ReopenEndpoints],
        ),
        (
            "Recovering/AudioReady",
            "EndpointsRestored",
            S::AudioReady { translate: true },
            vec![],
        ),
        (
            "Recovering/AudioReady",
            "RecoveryTimeout",
            degraded(&[missing(Endpoint::LocalOutput)]),
            vec![A::Report(missing(Endpoint::LocalOutput))],
        ),
        (
            "Recovering/Paused",
            "DidWake",
            recovering(paused(out(), &[])),
            vec![A::ReopenEndpoints],
        ),
    ]
}

/// Part 7 of the legal rows (see `legal_rows`).
pub(super) fn rows_part_7() -> Vec<Row> {
    vec![
        (
            "Recovering/Paused",
            "EndpointsRestored",
            paused(out(), &[]),
            vec![A::ResumeDirections(inc())],
        ),
        (
            "Recovering/Paused",
            "RecoveryTimeout",
            degraded(&[missing(Endpoint::LocalOutput)]),
            vec![A::Report(missing(Endpoint::LocalOutput))],
        ),
        (
            "Recovering/Degraded",
            "DidWake",
            recovering(degraded(&[dir_deg()])),
            vec![A::ReopenEndpoints],
        ),
        (
            "Recovering/Degraded",
            "EndpointsRestored",
            degraded(&[dir_deg()]),
            vec![A::ResumeDirections(DirSet::BOTH)],
        ),
        (
            "Recovering/Degraded",
            "RecoveryTimeout",
            degraded(&[dir_deg(), missing(Endpoint::LocalOutput)]),
            vec![A::Report(missing(Endpoint::LocalOutput))],
        ),
        ("Stopping", "Stopped", S::Idle, vec![A::Publish]),
        (
            "Stopping",
            "StopTimeout",
            S::Idle,
            vec![A::ForceClose, A::ReportLeak],
        ),
    ]
}

/// Rows of the states that can enter recovery: what each does on a lost endpoint and on sleep.
fn recovery_entry_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let both = DirSet::BOTH;
    rows.extend(recovery_entry(
        "AudioReady/audio-only",
        S::AudioReady { translate: false },
        DirSet::NONE,
        out(),
        both,
    ));
    rows.extend(recovery_entry(
        "AudioReady/translate",
        S::AudioReady { translate: true },
        DirSet::NONE,
        out(),
        both,
    ));
    rows.extend(recovery_entry(
        "Translating",
        S::Translating,
        both,
        out(),
        both,
    ));
    rows.extend(recovery_entry(
        "Degraded/D1",
        degraded(&[dir_deg()]),
        both,
        out(),
        both,
    ));
    rows.extend(recovery_entry(
        "Degraded/D2",
        degraded(&[dir_deg(), R::DirectionFailed(In)]),
        both,
        out(),
        both,
    ));
    rows.extend(recovery_entry(
        "Paused/out",
        paused(out(), &[]),
        inc(),
        DirSet::NONE,
        inc(),
    ));
    rows.extend(recovery_entry(
        "Paused/both",
        paused(both, &[]),
        DirSet::NONE,
        DirSet::NONE,
        DirSet::NONE,
    ));
    rows.extend(recovery_entry(
        "Paused/out+reason",
        paused(out(), &[R::DirectionFailed(In)]),
        inc(),
        DirSet::NONE,
        inc(),
    ));
    rows
}

/// The legal transitions of table 3.6 (with the decisions listed in the module report); everything else must be rejected.
pub(super) fn legal_rows() -> Vec<Row> {
    let mut rows: Vec<Row> = rows_part_1();
    rows.extend(rows_part_2());
    rows.extend(rows_part_3());
    rows.extend(rows_part_4());
    rows.extend(rows_part_5());
    rows.extend(rows_part_6());
    rows.extend(rows_part_7());
    for l in [
        "Init/NotNeeded",
        "Init/Pending",
        "Init/Ready",
        "Init/Failed",
    ] {
        rows.extend(init_rows(l));
    }
    rows.extend(recovery_entry_rows());
    for l in [
        "Recovering/Translating",
        "Recovering/AudioReady",
        "Recovering/Paused",
        "Recovering/Degraded",
    ] {
        rows.push((l, "Stop", S::Stopping, stop_actions()));
    }
    rows
}
