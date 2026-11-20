//! Driving the two direction machines and executing their actions, including what each listener hears.
//!
//! The call machine's `ApplyFailurePolicy` is a request. What a listener hears
//! is decided only by the direction machine's `ChangeOutput` action
//! (pipeline-design 4.3): this file forwards each one to the path (a mixer
//! crossfade) and records it in the snapshot, so a change is never silent.
//!
//! A direction that translates runs a pipeline with its own direction machine
//! (`translation`); this file then only forwards commands to it. A direction
//! without a pipeline (providers missing, audio only) uses the coordinator's own
//! machine, which has no stage workers: `SpawnStages`, `OpenAsrStream`,
//! `CancelDirection` and `ResetAsrStream` have nothing to do and the machine is
//! told `Joined` at once. That machine alone carries the failure policy
//! (`call.*_on_failure`) when providers could not be prepared.

use prxvc_media::Direction;

use super::actions::{POLICY_FAULT, stage_of};
use super::coordinator::{Coordinator, slot};
use super::error::CallErrorCode;
use super::snapshot::OutputChange;
use super::translation::Running;
use crate::direction_state::{DirectionAction, DirectionEvent, DirectionState};
use crate::fault::Stage;
use crate::output::{OutputCause, OutputSource};

impl Coordinator {
    /// Offer `event` to a direction machine and execute its actions. A
    /// rejection is logged and the state kept.
    pub(super) fn feed_dir(&mut self, d: Direction, event: DirectionEvent) {
        if self.lanes[slot(d)].running.is_some() && self.command_pipeline(d, &event) {
            return;
        }
        let lane = &mut self.lanes[slot(d)];
        let machine = std::mem::replace(&mut lane.machine, DirectionState::Off);
        match machine.on(&lane.cfg, event) {
            Ok((next, actions)) => {
                lane.machine = next;
                for action in actions {
                    self.apply_dir_action(d, action);
                }
            }
            Err(rejection) => {
                tracing::warn!(direction = ?d, %rejection, "direction event rejected");
                lane.machine = rejection.from;
            }
        }
    }

    /// A lane that runs a pipeline is driven by commands to it; its machine mirrors the
    /// pipeline's (see `translation`). Returns whether `event` was such a command.
    fn command_pipeline(&mut self, d: Direction, event: &DirectionEvent) -> bool {
        match event {
            DirectionEvent::Pause => self.with_running(d, Running::pause),
            DirectionEvent::Resume => self.with_running(d, Running::resume),
            DirectionEvent::Suspend => self.with_running(d, Running::suspend),
            DirectionEvent::Stop => self.stop_translation(d),
            _ => return false,
        }
        true
    }

    fn with_running(&self, d: Direction, f: impl FnOnce(&Running)) {
        if let Some(running) = self.lanes[slot(d)].running.as_ref() {
            f(running);
        }
    }

    fn apply_dir_action(&mut self, d: Direction, action: DirectionAction) {
        match action {
            DirectionAction::ChangeOutput {
                direction,
                from,
                to,
                cause,
            } => self.announce_output(direction, from, to, cause),
            DirectionAction::Report(reason) => self.raise(
                CallErrorCode::DirectionDegraded,
                format!("{d:?} direction: {reason:?}"),
            ),
            DirectionAction::ReportFailure { stage, fault } => self.raise(
                CallErrorCode::TranslationUnavailable,
                format!("{d:?} direction failed at {stage:?}: {fault:?}"),
            ),
            DirectionAction::SpawnStages
            | DirectionAction::OpenAsrStream
            | DirectionAction::CancelDirection(_)
            | DirectionAction::ResetAsrStream => {}
            DirectionAction::JoinStages => self.feed_dir(d, DirectionEvent::Joined),
        }
    }

    /// Record and execute a change of what a listener hears.
    pub(super) fn announce_output(
        &mut self,
        direction: Direction,
        from: OutputSource,
        to: OutputSource,
        cause: OutputCause,
    ) {
        let at = self.now();
        let lane = &mut self.lanes[slot(direction)];
        lane.hears = to;
        if let Some(path) = lane.path.as_mut() {
            path.set_listener_hears(to);
        }
        tracing::info!(
            ?direction,
            ?from,
            ?to,
            ?cause,
            "what the listener hears changed"
        );
        self.last_change = Some(OutputChange {
            direction,
            from,
            to,
            cause,
            at,
        });
    }

    /// `RestoreTranslatedOutput`: a failed direction tries again (a pipeline recovers by itself).
    pub(super) fn restore_translated(&mut self, d: Direction) {
        if self.lanes[slot(d)].running.is_none()
            && matches!(self.lanes[slot(d)].machine, DirectionState::Failed { .. })
        {
            self.feed_dir(d, DirectionEvent::Retry);
            self.feed_dir(d, DirectionEvent::StagesReady);
        }
    }

    /// `ApplyFailurePolicy`: make the direction's machine reflect the failure,
    /// so that its own output rule (`call.*_on_failure`) says what is heard.
    pub(super) fn apply_failure_policy(&mut self, d: Direction) {
        if self.lanes[slot(d)].running.is_some() {
            // The pipeline's own machine already decided what the listener hears.
            return;
        }
        let stage = self.provider_role.map_or(Stage::Asr, stage_of);
        let failed = DirectionEvent::StageFailed {
            stage,
            fault: POLICY_FAULT,
        };
        match self.lanes[slot(d)].machine {
            DirectionState::Off => {
                self.feed_dir(d, DirectionEvent::Start);
                self.feed_dir(d, failed);
            }
            DirectionState::Starting { .. }
            | DirectionState::Running
            | DirectionState::Degraded { .. } => {
                self.feed_dir(d, failed);
            }
            // Already failed, paused (the pause policy is audible), suspended or stopping.
            _ => {}
        }
    }

    /// `StopDirections`: stop every direction that exists.
    pub(super) fn stop_directions(&mut self) {
        self.prepared = None;
        for d in super::coordinator::BOTH {
            if !matches!(self.lanes[slot(d)].machine, DirectionState::Off) {
                self.feed_dir(d, DirectionEvent::Stop);
            }
        }
        self.host.release_providers();
    }
}
