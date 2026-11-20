//! State machine of the call session (pipeline-design table 3.6): which events move a call between its states and what each move asks for.

use std::collections::BTreeSet;

use prxvc_media::Direction;
use prxvc_ports::BundleId;

use crate::dir_set::DirSet;
use crate::fault::{Blocker, DegradeReason, Endpoint, FaultKind, ProviderRole, Stage};
use crate::rejection::InvalidTransition;

/// Why a call is degraded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CallReason {
    /// A direction runs in a reduced way (it still translates).
    Direction {
        /// The direction.
        direction: Direction,
        /// The condition.
        reason: DegradeReason,
    },
    /// A direction failed and its failure policy applies.
    DirectionFailed(Direction),
    /// A provider role could not be prepared.
    Providers(ProviderRole),
    /// An endpoint did not come back.
    EndpointMissing(Endpoint),
}

/// The set of reasons of a degraded call.
pub type CallReasons = BTreeSet<CallReason>;

/// How far provider preparation has come while the call starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderProgress {
    /// Translation was not requested; no provider is prepared.
    NotNeeded,
    /// Preparation is running.
    Pending,
    /// Every provider is ready.
    Ready,
    /// A role failed.
    Failed(ProviderRole),
}

/// The state of a call session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallState {
    /// No call.
    Idle,
    /// Preflight and audio start-up are running; providers prepare in parallel.
    Initializing {
        /// Provider preparation so far, kept so that an early result is not lost.
        providers: ProviderProgress,
    },
    /// The four endpoints run with passthrough; directions are not started.
    AudioReady {
        /// Whether translation was requested (providers are still preparing).
        translate: bool,
    },
    /// Both directions translate.
    Translating,
    /// The user paused some directions.
    Paused {
        /// The paused directions.
        dirs: DirSet,
        /// Reasons that survive the pause; the call resumes into `Degraded` when any remain.
        reasons: CallReasons,
    },
    /// Something is reduced or failed; the reasons are never empty.
    Degraded {
        /// Why (not empty).
        reasons: CallReasons,
    },
    /// Endpoints are lost or the system sleeps; the call returns to `resume_to`.
    Recovering {
        /// The state to return to when the endpoints are back.
        resume_to: Box<CallState>,
    },
    /// Shutting down.
    Stopping,
}

/// An event a call machine accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallEvent {
    /// The user started call mode.
    StartRequested {
        /// The call application to tap, if given.
        target: Option<BundleId>,
        /// Whether translation is requested (otherwise audio only).
        translate: bool,
    },
    /// A preflight step is blocking.
    PreflightFailed(Blocker),
    /// Endpoints run, the default input is switched, passthrough is active.
    AudioRunning,
    /// An endpoint failed while starting.
    EndpointFailed {
        /// The endpoint.
        endpoint: Endpoint,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// Switching the default input failed.
    RoutingFailed(FaultKind),
    /// Every provider is ready.
    ProvidersReady,
    /// A provider role could not be prepared.
    ProvidersFailed {
        /// The role.
        role: ProviderRole,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// A direction runs in a reduced way.
    DirectionDegraded {
        /// The direction.
        direction: Direction,
        /// The condition.
        reason: DegradeReason,
    },
    /// A direction failed without a fallback.
    DirectionFailed {
        /// The direction.
        direction: Direction,
        /// The stage.
        stage: Stage,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// A direction has no remaining problem.
    DirectionHealthy(Direction),
    /// The user paused these directions.
    PauseRequested(DirSet),
    /// The user resumed every paused direction.
    ResumeRequested,
    /// An endpoint disappeared.
    EndpointLost(Endpoint),
    /// The system is about to sleep.
    SystemWillSleep,
    /// The system woke.
    SystemDidWake,
    /// Every lost endpoint is open again.
    EndpointsRestored,
    /// Recovery took too long (10 s) and `missing` is still absent.
    RecoveryTimeout {
        /// The endpoint that did not return.
        missing: Endpoint,
    },
    /// The user stopped call mode.
    StopRequested,
    /// Shutdown finished.
    Stopped,
    /// Shutdown took too long (2 s).
    StopTimeout,
}

/// A side effect a call machine asks the coordinator to perform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallAction {
    /// Run the start-up checks (section 4.1).
    RunPreflight {
        /// The call application to tap, if given.
        target: Option<BundleId>,
    },
    /// Start preparing providers.
    PrepareProviders,
    /// Show why the call cannot start.
    ReportBlocker(Blocker),
    /// Publish a snapshot.
    Publish,
    /// Restore the system default input.
    RestoreRouting,
    /// Close the four endpoints.
    CloseEndpoints,
    /// Start both directions.
    StartDirections,
    /// Make these directions' failure policy audible (`call.*_on_failure`).
    ApplyFailurePolicy(DirSet),
    /// Make these directions' pause policy audible (`call.*_when_paused`).
    ApplyPausePolicy(DirSet),
    /// Show a reason to the user.
    Report(CallReason),
    /// A direction recovered; its translated output returns.
    RestoreTranslatedOutput(Direction),
    /// Pause these directions.
    PauseDirections(DirSet),
    /// Resume these directions.
    ResumeDirections(DirSet),
    /// Suspend these directions, cancelling their in-flight work.
    SuspendDirections(DirSet),
    /// Reopen the endpoints after wake.
    ReopenEndpoints,
    /// Cancel the call token.
    CancelAll,
    /// Stop both directions.
    StopDirections,
    /// Close what is still open after a shutdown timeout.
    ForceClose,
    /// Report that shutdown left something open.
    ReportLeak,
}

/// A rejected call event.
pub type CallRejection = InvalidTransition<CallState, CallEvent>;

type Moved = Result<(CallState, Vec<CallAction>), CallEvent>;

use CallAction as A;
use CallEvent as E;
use CallState as S;

impl CallState {
    /// The directions that are running (neither paused nor absent) in this state.
    pub fn running_dirs(&self) -> DirSet {
        match self {
            S::Translating | S::Degraded { .. } => DirSet::BOTH,
            S::Paused { dirs, .. } => DirSet::BOTH.minus(*dirs),
            _ => DirSet::NONE,
        }
    }

    /// Apply `event`; a legal event returns the new state and its actions,
    /// an illegal one a typed rejection that carries the unchanged state.
    pub fn on(self, event: CallEvent) -> Result<(Self, Vec<CallAction>), CallRejection> {
        let backup = self.clone();
        self.route(event)
            .map_err(|event| InvalidTransition::new(backup, event))
    }

    /// The transition table (3.6): rows shared by several states first, then one function per state.
    fn route(self, event: CallEvent) -> Moved {
        match (self, event) {
            (S::Idle | S::Stopping, e @ E::StopRequested) => Err(e),
            (_, E::StopRequested) => Ok(stopping()),
            (
                s @ (S::AudioReady { .. } | S::Translating | S::Paused { .. } | S::Degraded { .. }),
                E::EndpointLost(ep),
            ) => Ok(enter_recovery(s, DirSet::one(ep.direction()))),
            (
                s @ (S::AudioReady { .. } | S::Translating | S::Paused { .. } | S::Degraded { .. }),
                E::SystemWillSleep,
            ) => Ok(enter_recovery(s, DirSet::BOTH)),
            (S::Idle, e) => idle(e),
            (S::Initializing { providers }, e) => initializing(providers, e),
            (S::AudioReady { translate }, e) => audio_ready(translate, e),
            (S::Translating, e) => translating(e),
            (S::Paused { dirs, reasons }, e) => paused(dirs, reasons, e),
            (S::Degraded { reasons }, e) => degraded(reasons, e),
            (S::Recovering { resume_to }, e) => recovering(*resume_to, e),
            (S::Stopping, e) => stopping_state(e),
        }
    }
}

fn stopping() -> (CallState, Vec<CallAction>) {
    (
        S::Stopping,
        vec![
            A::CancelAll,
            A::StopDirections,
            A::RestoreRouting,
            A::CloseEndpoints,
        ],
    )
}

fn idle(event: CallEvent) -> Moved {
    match event {
        E::StartRequested { target, translate } => {
            let providers = if translate {
                ProviderProgress::Pending
            } else {
                ProviderProgress::NotNeeded
            };
            let mut actions = vec![A::RunPreflight { target }];
            if translate {
                actions.push(A::PrepareProviders);
            }
            Ok((S::Initializing { providers }, actions))
        }
        e => Err(e),
    }
}

fn initializing(providers: ProviderProgress, event: CallEvent) -> Moved {
    use ProviderProgress as P;
    match (providers, event) {
        (_, E::PreflightFailed(b)) => Ok((S::Idle, vec![A::ReportBlocker(b)])),
        (_, E::EndpointFailed { endpoint, fault }) => {
            Ok(failed_start(Blocker::EndpointFailed(endpoint, fault)))
        }
        (_, E::RoutingFailed(fault)) => Ok(failed_start(Blocker::RoutingFailed(fault))),
        (P::Pending, E::ProvidersReady) => Ok((
            S::Initializing {
                providers: P::Ready,
            },
            vec![],
        )),
        (P::Pending, E::ProvidersFailed { role, .. }) => Ok((
            S::Initializing {
                providers: P::Failed(role),
            },
            vec![],
        )),
        (P::NotNeeded, E::AudioRunning) => {
            Ok((S::AudioReady { translate: false }, vec![A::Publish]))
        }
        (P::Pending, E::AudioRunning) => Ok((S::AudioReady { translate: true }, vec![A::Publish])),
        (P::Ready, E::AudioRunning) => Ok((S::Translating, vec![A::Publish, A::StartDirections])),
        (P::Failed(role), E::AudioRunning) => {
            let reason = CallReason::Providers(role);
            Ok((
                S::Degraded {
                    reasons: [reason].into(),
                },
                vec![
                    A::Publish,
                    A::ApplyFailurePolicy(DirSet::BOTH),
                    A::Report(reason),
                ],
            ))
        }
        (_, e) => Err(e),
    }
}

fn failed_start(blocker: Blocker) -> (CallState, Vec<CallAction>) {
    (
        S::Stopping,
        vec![
            A::RestoreRouting,
            A::CloseEndpoints,
            A::ReportBlocker(blocker),
        ],
    )
}

fn audio_ready(translate: bool, event: CallEvent) -> Moved {
    match (translate, event) {
        (true, E::ProvidersReady) => Ok((S::Translating, vec![A::StartDirections])),
        (true, E::ProvidersFailed { role, .. }) => {
            let reason = CallReason::Providers(role);
            Ok((
                S::Degraded {
                    reasons: [reason].into(),
                },
                vec![A::ApplyFailurePolicy(DirSet::BOTH), A::Report(reason)],
            ))
        }
        (_, e) => Err(e),
    }
}

/// What a direction event does to a set of reasons, shared by the states that carry directions.
///
/// `None` means the event is not a direction event, or is illegal here
/// (`DirectionHealthy` while nothing is degraded is decided by the caller).
fn direction_event(reasons: &mut CallReasons, event: &CallEvent) -> Option<Vec<CallAction>> {
    match event {
        E::DirectionDegraded { direction, reason } => {
            let why = CallReason::Direction {
                direction: *direction,
                reason: *reason,
            };
            reasons.insert(why);
            Some(vec![A::Report(why)])
        }
        E::DirectionFailed { direction, .. } => {
            reasons.insert(CallReason::DirectionFailed(*direction));
            Some(vec![A::ApplyFailurePolicy(DirSet::one(*direction))])
        }
        E::DirectionHealthy(d) => {
            let failed = reasons.remove(&CallReason::DirectionFailed(*d));
            reasons.retain(
                |r| !matches!(r, CallReason::Direction { direction, .. } if direction == d),
            );
            Some(if failed {
                vec![A::RestoreTranslatedOutput(*d)]
            } else {
                vec![]
            })
        }
        _ => None,
    }
}

fn degraded_or_translating(reasons: CallReasons) -> CallState {
    if reasons.is_empty() {
        S::Translating
    } else {
        S::Degraded { reasons }
    }
}

fn translating(event: CallEvent) -> Moved {
    match event {
        e @ E::DirectionHealthy(_) => Err(e),
        e @ (E::DirectionDegraded { .. } | E::DirectionFailed { .. }) => {
            let mut reasons = CallReasons::new();
            let actions = direction_event(&mut reasons, &e).unwrap_or_default();
            Ok((S::Degraded { reasons }, actions))
        }
        E::PauseRequested(dirs) => Ok(pause(dirs, DirSet::NONE, CallReasons::new())),
        e => Err(e),
    }
}

fn degraded(mut reasons: CallReasons, event: CallEvent) -> Moved {
    if let Some(actions) = direction_event(&mut reasons, &event) {
        return Ok((degraded_or_translating(reasons), actions));
    }
    match event {
        E::PauseRequested(dirs) => Ok(pause(dirs, DirSet::NONE, reasons)),
        e => Err(e),
    }
}

/// Enter or extend `Paused`; `already` are the directions paused before.
fn pause(dirs: DirSet, already: DirSet, reasons: CallReasons) -> (CallState, Vec<CallAction>) {
    let new = dirs.minus(already);
    let mut actions = vec![];
    if !new.is_empty() {
        actions.push(A::PauseDirections(new));
        actions.push(A::ApplyPausePolicy(new));
    }
    (
        S::Paused {
            dirs: already.union(dirs),
            reasons,
        },
        actions,
    )
}

fn paused(dirs: DirSet, mut reasons: CallReasons, event: CallEvent) -> Moved {
    if let Some(actions) = direction_event(&mut reasons, &event) {
        return Ok((S::Paused { dirs, reasons }, actions));
    }
    match event {
        E::PauseRequested(more) => Ok(pause(more, dirs, reasons)),
        E::ResumeRequested => Ok((
            degraded_or_translating(reasons),
            vec![A::ResumeDirections(dirs)],
        )),
        e => Err(e),
    }
}

/// Enter `Recovering` from `from`; `affected` are the directions whose endpoints are gone.
fn enter_recovery(from: CallState, affected: DirSet) -> (CallState, Vec<CallAction>) {
    let paused = match &from {
        S::Paused { dirs, .. } => *dirs,
        _ => DirSet::NONE,
    };
    let running = from.running_dirs();
    let audible = affected.minus(paused);
    let mut actions = vec![];
    if !running.is_empty() {
        actions.push(A::SuspendDirections(running));
    }
    if !audible.is_empty() {
        actions.push(A::ApplyFailurePolicy(audible));
    }
    (
        S::Recovering {
            resume_to: Box::new(from),
        },
        actions,
    )
}

fn recovering(resume_to: CallState, event: CallEvent) -> Moved {
    match event {
        E::SystemDidWake => Ok((
            S::Recovering {
                resume_to: Box::new(resume_to),
            },
            vec![A::ReopenEndpoints],
        )),
        E::EndpointsRestored => {
            let running = resume_to.running_dirs();
            let actions = if running.is_empty() {
                vec![]
            } else {
                vec![A::ResumeDirections(running)]
            };
            Ok((resume_to, actions))
        }
        E::RecoveryTimeout { missing } => {
            let why = CallReason::EndpointMissing(missing);
            let mut reasons = match resume_to {
                S::Degraded { reasons } | S::Paused { reasons, .. } => reasons,
                _ => CallReasons::new(),
            };
            reasons.insert(why);
            Ok((S::Degraded { reasons }, vec![A::Report(why)]))
        }
        e => Err(e),
    }
}

fn stopping_state(event: CallEvent) -> Moved {
    match event {
        E::Stopped => Ok((S::Idle, vec![A::Publish])),
        E::StopTimeout => Ok((S::Idle, vec![A::ForceClose, A::ReportLeak])),
        e => Err(e),
    }
}

#[cfg(test)]
mod tests;
