//! Drives the direction machine: executes its actions, applies degradation reasons and recovers from failures.

use std::sync::atomic::Ordering;

use super::{Core, Recovery};
use crate::direction_state::{DirectionAction, DirectionEvent, DirectionState};
use crate::fault::{DegradeReason, FaultKind, Stage};
use crate::interrupt::CancelAction;
use crate::pipeline::events::PipelineEvent;
use crate::pipeline::messages::{MtItem, StageFailure, TtsItem, WorkerEvent};
use crate::pipeline::queue::Pushed;
use crate::pipeline::snapshot::{ErrorSnapshot, Stats};
use crate::segment_book::SegmentEvent;

impl Core {
    /// Feed an event to the direction machine and execute its actions.
    pub(super) fn direction_event(&mut self, event: DirectionEvent) {
        let before = self.state.clone();
        match self.state.clone().on(&self.cfg, event.clone()) {
            Err(rejection) => {
                Stats::add(&self.w.stats.rejected_events, 1);
                tracing::debug!(%rejection, "direction machine refused an event");
            }
            Ok((next, actions)) => {
                self.state = next;
                self.on_state_changed(&before);
                for action in actions {
                    self.run_direction_action(action);
                }
                if let DirectionEvent::ReasonCleared(r) = event
                    && before.reasons().contains(&r)
                {
                    self.emit(PipelineEvent::Recovered(r));
                }
            }
        }
        self.dirty = true;
    }

    fn on_state_changed(&mut self, before: &DirectionState) {
        let active = matches!(
            self.state,
            DirectionState::Running | DirectionState::Degraded { .. }
        );
        self.w.gate.active.store(active, Ordering::SeqCst);
        if &self.state != before {
            let state = self.state.clone();
            self.emit(PipelineEvent::State(state));
        }
        if matches!(self.state, DirectionState::Failed { .. }) {
            self.schedule_recovery();
        }
        if matches!(
            self.state,
            DirectionState::Off | DirectionState::Stopping { .. }
        ) {
            self.recovery = None;
        }
    }

    fn run_direction_action(&mut self, action: DirectionAction) {
        match action {
            DirectionAction::SpawnStages
            | DirectionAction::OpenAsrStream
            | DirectionAction::JoinStages => {}
            DirectionAction::CancelDirection(d) => {
                self.apply_segment(SegmentEvent::Cancel(CancelAction::CancelDirection(d)));
            }
            DirectionAction::ResetAsrStream => {
                self.w.gate.reset.store(true, Ordering::SeqCst);
                self.w.audio_in.wake();
            }
            DirectionAction::Report(r) => self.emit(PipelineEvent::Degraded(r)),
            DirectionAction::ReportFailure { stage, fault } => {
                self.emit(PipelineEvent::Failed { stage, fault });
            }
            DirectionAction::ChangeOutput {
                from, to, cause, ..
            } => {
                self.emit(PipelineEvent::Output { from, to, cause });
            }
        }
    }

    /// Raise or clear a degradation reason, in the states where it can apply.
    pub(super) fn set_reason(&mut self, reason: DegradeReason, on: bool) {
        let applies = matches!(
            self.state,
            DirectionState::Running
                | DirectionState::Degraded { .. }
                | DirectionState::Paused { .. }
                | DirectionState::Suspended { .. }
        );
        if !applies {
            return;
        }
        let present = self.state.reasons().contains(&reason);
        if on && !present {
            self.direction_event(DirectionEvent::StageDegraded(reason));
        } else if !on && present {
            self.direction_event(DirectionEvent::ReasonCleared(reason));
        }
    }

    /// Events of the workers that belong to no clause.
    pub(super) fn worker_event(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::RecognizerReady => {
                if matches!(self.state, DirectionState::Starting { .. }) {
                    self.direction_event(DirectionEvent::StagesReady);
                }
            }
            WorkerEvent::RecognizerFailed { stage, failure } => {
                self.stage_failed(stage, &failure);
            }
            WorkerEvent::Overrun => {
                self.set_reason(DegradeReason::CaptureOverrun, true);
            }
            WorkerEvent::OverrunCleared => {
                self.set_reason(DegradeReason::CaptureOverrun, false);
            }
            WorkerEvent::ProbeResult { stage, result } => self.probe_result(stage, result),
            WorkerEvent::CaptureEnded(e) => self.emit(PipelineEvent::CaptureEnded(e)),
        }
    }

    /// A stage failed for good: record it and apply the failure policy through the machine.
    pub(super) fn stage_failed(&mut self, stage: Stage, failure: &StageFailure) {
        self.last_error = Some(ErrorSnapshot {
            stage,
            fault: failure.fault,
            at: self.w.tracer.now(),
            detail: failure.detail.clone(),
        });
        self.direction_event(DirectionEvent::StageFailed {
            stage,
            fault: failure.fault,
        });
    }

    fn schedule_recovery(&mut self) {
        let now = self.w.tracer.now();
        let wait = match &self.recovery {
            Some(r) => (r.wait * 2).min(self.p.recovery.max_wait),
            None => self.p.recovery.first_probe_after,
        };
        self.recovery = Some(Recovery {
            due: now.saturating_add(wait),
            wait,
            probing: false,
        });
    }

    /// When the next probe of a failed provider is due.
    pub(crate) fn next_deadline(&self) -> Option<prxvc_media::MonoTime> {
        let retry = match (&self.state, &self.recovery) {
            (DirectionState::Failed { .. }, Some(r)) if !r.probing => Some(r.due),
            _ => None,
        };
        match (retry, self.backlog_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The injected clock reached a deadline of [`Core::next_deadline`].
    pub(crate) fn on_timer(&mut self, now: prxvc_media::MonoTime) {
        if self.backlog_due.is_some_and(|due| now >= due) {
            self.after_event();
        }
        let retry_due = match (&self.state, &self.recovery) {
            (DirectionState::Failed { .. }, Some(r)) if !r.probing => Some(r.due),
            _ => None,
        };
        if retry_due.is_some_and(|due| now >= due) {
            self.begin_retry();
            self.after_event();
        }
    }

    /// Retry a failed direction: the machine goes to `Starting` and the failed provider is probed.
    pub(super) fn begin_retry(&mut self) {
        let DirectionState::Failed { stage, .. } = self.state else {
            return;
        };
        self.direction_event(DirectionEvent::Retry);
        if let Some(r) = self.recovery.as_mut() {
            r.probing = true;
        }
        match stage {
            Stage::Vad | Stage::Asr | Stage::Refine => {
                self.w.gate.probe.store(true, Ordering::SeqCst);
                self.w.audio_in.wake();
            }
            Stage::Mt => {
                if !matches!(
                    self.w.mt_in.push(MtItem::Probe, None),
                    Pushed::Accepted | Pushed::Evicted(_)
                ) {
                    self.probe_result(
                        stage,
                        Err(StageFailure::new(FaultKind::Unavailable, "probe refused")),
                    );
                }
            }
            Stage::Tts => {
                if !matches!(
                    self.w.tts_in.push(TtsItem::Probe, None),
                    Pushed::Accepted | Pushed::Evicted(_)
                ) {
                    self.probe_result(
                        stage,
                        Err(StageFailure::new(FaultKind::Unavailable, "probe refused")),
                    );
                }
            }
            Stage::Playout => self.probe_result(stage, Ok(())),
        }
    }

    fn probe_result(&mut self, stage: Stage, result: Result<(), StageFailure>) {
        if !matches!(
            self.state,
            DirectionState::Starting {
                after_failure: true
            }
        ) {
            return;
        }
        match result {
            Ok(()) => {
                self.recovery = None;
                self.direction_event(DirectionEvent::StagesReady);
            }
            Err(failure) => self.stage_failed(stage, &failure),
        }
    }

    /// Every worker thread ended (or the wait for them timed out).
    pub(super) fn joined(&mut self, clean: bool) {
        if !clean {
            self.last_error = Some(ErrorSnapshot {
                stage: Stage::Playout,
                fault: FaultKind::Crashed,
                at: self.w.tracer.now(),
                detail: "a stage thread did not end in time".to_owned(),
            });
        }
        self.direction_event(DirectionEvent::Joined);
        self.finished = true;
    }
}
