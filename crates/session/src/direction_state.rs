//! State machine of one translation direction (pipeline-design table 3.7), including what the listener hears in every state.

use std::collections::BTreeSet;

use prxvc_media::Direction;

use crate::fault::{DegradeReason, FaultKind, Stage};
use crate::output::{DirectionConfig, OutputCause, OutputPolicy, OutputSource};
use crate::rejection::InvalidTransition;

/// The set of conditions that currently reduce a direction.
pub type Reasons = BTreeSet<DegradeReason>;

/// The state of one direction. Activity inside `Running` (listening,
/// recognizing, speaking) is derived from the segment book, not a state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectionState {
    /// No stages exist.
    Off,
    /// Stages are being spawned. `after_failure` is set when this is a retry,
    /// so that the failure fallback keeps playing until the stages are ready.
    Starting {
        /// Whether this start follows a failure.
        after_failure: bool,
    },
    /// Translating.
    Running,
    /// Paused by the user; the reasons that remain are restored on resume.
    Paused {
        /// Degradation reasons that survive the pause.
        reasons: Reasons,
    },
    /// Suspended by the system (sleep, lost endpoint); reasons survive.
    Suspended {
        /// Degradation reasons that survive the suspension.
        reasons: Reasons,
    },
    /// Translating in a reduced way; never constructed with an empty set.
    Degraded {
        /// Why the direction is reduced (not empty).
        reasons: Reasons,
    },
    /// A stage failed and no fallback is configured for it.
    Failed {
        /// The stage that failed.
        stage: Stage,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// Stopping; `held` is what the listener hears until the stages joined.
    Stopping {
        /// The audible source kept while stopping.
        held: OutputSource,
    },
}

/// An event a direction machine accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectionEvent {
    /// Start the direction.
    Start,
    /// Every stage is ready.
    StagesReady,
    /// A stage failed and no fallback is configured.
    StageFailed {
        /// The stage.
        stage: Stage,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// A fallback or overload condition began.
    StageDegraded(DegradeReason),
    /// A condition ended.
    ReasonCleared(DegradeReason),
    /// The user paused the direction.
    Pause,
    /// The pause or the suspension ended.
    Resume,
    /// The system suspended the direction.
    Suspend,
    /// Try again after a failure.
    Retry,
    /// Stop the direction.
    Stop,
    /// Every stage thread has joined.
    Joined,
}

/// A side effect a direction machine asks its owner to perform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectionAction {
    /// Create the stage workers.
    SpawnStages,
    /// Open the recognition stream.
    OpenAsrStream,
    /// Cancel every in-flight and queued segment of the direction (the other direction is untouched).
    CancelDirection(Direction),
    /// Reset the recognition stream after a suspension.
    ResetAsrStream,
    /// Wait for the stage threads to end.
    JoinStages,
    /// Show a degradation to the user.
    Report(DegradeReason),
    /// Show a failure to the user.
    ReportFailure {
        /// The stage.
        stage: Stage,
        /// The class of the failure.
        fault: FaultKind,
    },
    /// The audible source changed. Emitted for every change and only for a
    /// change; the mixer crossfades and the UI shows it (PD-4.3.1).
    ChangeOutput {
        /// The direction whose listener is affected.
        direction: Direction,
        /// What was heard.
        from: OutputSource,
        /// What is heard now.
        to: OutputSource,
        /// Why it changed.
        cause: OutputCause,
    },
}

/// A rejected direction event.
pub type DirectionRejection = InvalidTransition<DirectionState, DirectionEvent>;

type Step = Result<(DirectionState, Vec<DirectionAction>), DirectionRejection>;

use DirectionAction as A;
use DirectionEvent as E;
use DirectionState as S;

impl DirectionState {
    /// What the listener hears in this state under `policy`.
    pub fn output(&self, policy: &OutputPolicy) -> OutputSource {
        match self {
            S::Off
            | S::Starting {
                after_failure: false,
            } => OutputSource::Original,
            S::Starting {
                after_failure: true,
            }
            | S::Suspended { .. }
            | S::Failed { .. } => policy.on_failure.source(),
            S::Running | S::Degraded { .. } => OutputSource::Translation,
            S::Paused { .. } => policy.when_paused.source(),
            S::Stopping { held } => *held,
        }
    }

    /// The degradation reasons of this state; empty where none apply.
    pub fn reasons(&self) -> Reasons {
        match self {
            S::Paused { reasons } | S::Suspended { reasons } | S::Degraded { reasons } => {
                reasons.clone()
            }
            _ => Reasons::new(),
        }
    }

    /// Apply `event`; a legal event returns the new state and its actions.
    ///
    /// Every change of the audible source is announced by exactly one
    /// [`DirectionAction::ChangeOutput`] appended to the transition's actions.
    pub fn on(self, cfg: &DirectionConfig, event: DirectionEvent) -> Step {
        let before = self.output(&cfg.policy);
        let prev = self.clone();
        let (next, mut actions) = self.step(cfg, event)?;
        let after = next.output(&cfg.policy);
        if after != before {
            actions.push(A::ChangeOutput {
                direction: cfg.direction,
                from: before,
                to: after,
                cause: cause_of(&prev, &next),
            });
        }
        Ok((next, actions))
    }

    /// The transition table (3.7). Output announcements are added by [`DirectionState::on`].
    fn step(self, cfg: &DirectionConfig, event: DirectionEvent) -> Step {
        let held = self.stopping_hold(&cfg.policy);
        match (self, event) {
            (S::Off, E::Start) => Ok((
                S::Starting {
                    after_failure: false,
                },
                vec![A::SpawnStages, A::OpenAsrStream],
            )),
            (S::Starting { .. }, E::StagesReady) => Ok((S::Running, vec![])),
            (
                S::Starting { .. } | S::Running | S::Degraded { .. },
                E::StageFailed { stage, fault },
            ) => Ok((
                S::Failed { stage, fault },
                vec![
                    A::CancelDirection(cfg.direction),
                    A::ReportFailure { stage, fault },
                ],
            )),
            (S::Running, E::StageDegraded(r)) => Ok((
                S::Degraded {
                    reasons: [r].into(),
                },
                vec![A::Report(r)],
            )),
            (S::Degraded { reasons }, E::StageDegraded(r)) => {
                Ok(add_reason(reasons, r, |reasons| S::Degraded { reasons }))
            }
            (S::Degraded { reasons }, E::ReasonCleared(r)) => Ok(remove_reason(reasons, r)),
            (S::Paused { reasons }, E::StageDegraded(r)) => {
                Ok(add_reason(reasons, r, |reasons| S::Paused { reasons }))
            }
            (S::Paused { mut reasons }, E::ReasonCleared(r)) => {
                reasons.remove(&r);
                Ok((S::Paused { reasons }, vec![]))
            }
            (S::Suspended { reasons }, E::StageDegraded(r)) => {
                Ok(add_reason(reasons, r, |reasons| S::Suspended { reasons }))
            }
            (S::Suspended { mut reasons }, E::ReasonCleared(r)) => {
                reasons.remove(&r);
                Ok((S::Suspended { reasons }, vec![]))
            }
            (s @ (S::Running | S::Degraded { .. }), E::Pause) => {
                let reasons = s.reasons();
                Ok((
                    S::Paused { reasons },
                    vec![A::CancelDirection(cfg.direction)],
                ))
            }
            (S::Paused { reasons }, E::Resume) => Ok((resumed(reasons), vec![])),
            (s @ (S::Running | S::Degraded { .. } | S::Paused { .. }), E::Suspend) => {
                let reasons = s.reasons();
                Ok((
                    S::Suspended { reasons },
                    vec![A::CancelDirection(cfg.direction)],
                ))
            }
            (S::Suspended { reasons }, E::Resume) => {
                Ok((resumed(reasons), vec![A::ResetAsrStream]))
            }
            (S::Failed { .. }, E::Retry) => Ok((
                S::Starting {
                    after_failure: true,
                },
                vec![A::SpawnStages, A::OpenAsrStream],
            )),
            (S::Stopping { held }, E::Stop) => Ok((S::Stopping { held }, vec![])),
            (S::Off, e @ E::Stop) => Err(InvalidTransition::new(S::Off, e)),
            (_, E::Stop) => Ok((
                S::Stopping { held },
                vec![A::CancelDirection(cfg.direction), A::JoinStages],
            )),
            (S::Stopping { .. }, E::Joined) => Ok((S::Off, vec![])),
            (s, e) => Err(InvalidTransition::new(s, e)),
        }
    }

    /// What a stopping direction keeps playing: the original where translation was playing, otherwise what was heard.
    fn stopping_hold(&self, policy: &OutputPolicy) -> OutputSource {
        match self.output(policy) {
            OutputSource::Translation => OutputSource::Original,
            other => other,
        }
    }
}

fn add_reason(
    mut reasons: Reasons,
    r: DegradeReason,
    state: impl FnOnce(Reasons) -> DirectionState,
) -> (DirectionState, Vec<DirectionAction>) {
    let new = reasons.insert(r);
    (
        state(reasons),
        if new { vec![A::Report(r)] } else { vec![] },
    )
}

fn remove_reason(mut reasons: Reasons, r: DegradeReason) -> (DirectionState, Vec<DirectionAction>) {
    reasons.remove(&r);
    (resumed(reasons), vec![])
}

/// `Running` when no reason remains, `Degraded` otherwise.
fn resumed(reasons: Reasons) -> DirectionState {
    if reasons.is_empty() {
        S::Running
    } else {
        S::Degraded { reasons }
    }
}

/// The cause to announce for the output change of the transition `prev` to `next`.
fn cause_of(prev: &DirectionState, next: &DirectionState) -> OutputCause {
    match next {
        S::Running | S::Degraded { .. } if matches!(prev, S::Starting { .. }) => {
            OutputCause::TranslationStarted
        }
        S::Running | S::Degraded { .. } => OutputCause::TranslationResumed,
        S::Paused { .. } => OutputCause::Paused,
        S::Suspended { .. } => OutputCause::Suspended,
        S::Failed { .. } | S::Starting { .. } => OutputCause::Failed,
        S::Stopping { .. } => OutputCause::Stopping,
        S::Off => OutputCause::Stopped,
    }
}

#[cfg(test)]
mod tests;
