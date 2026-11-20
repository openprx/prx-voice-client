//! Executing the call machine's actions: stop sequence, provider preparation and the hand-over to the direction machines.
//!
//! The machine decides; this file does. Stopping runs the actions in the
//! machine's own order (`CancelAll`, `StopDirections`, `RestoreRouting`,
//! `CloseEndpoints`): the default input is restored first, whatever happens
//! later, and the pumps (the virtual microphone feeder last) are closed after.

use super::coordinator::{BOTH, Coordinator, slot};
use super::error::{CallErrorCode, HostFault};
use crate::call_state::{CallAction, CallEvent, CallReason, CallState};
use crate::direction_state::DirectionEvent;
use crate::fault::{FaultKind, ProviderRole, Stage};
use prxvc_media::Direction;

impl Coordinator {
    /// Execute `actions` in order.
    pub(super) fn apply(&mut self, actions: Vec<CallAction>) {
        for action in actions {
            self.apply_one(action);
        }
    }

    fn apply_one(&mut self, action: CallAction) {
        match action {
            CallAction::RunPreflight { target } => self.start_sequence(target),
            CallAction::PrepareProviders => self.prepare_providers(),
            CallAction::ReportBlocker(blocker) => {
                // The precise typed issue was raised where the failure happened.
                tracing::info!(?blocker, "call mode could not start");
            }
            // A snapshot is published after every command and tick.
            CallAction::Publish => {}
            CallAction::RestoreRouting => self.restore_routing(),
            CallAction::CloseEndpoints => self.close_endpoints(),
            CallAction::StartDirections => {
                for d in BOTH {
                    self.start_direction(d);
                }
            }
            CallAction::ApplyFailurePolicy(dirs) => {
                for d in dirs.iter() {
                    self.apply_failure_policy(d);
                }
            }
            CallAction::ApplyPausePolicy(dirs) => {
                // The direction machine's pause announces what is heard; nothing to add.
                tracing::debug!(?dirs, "pause policy is applied by the direction machines");
            }
            CallAction::Report(reason) => self.report_reason(reason),
            CallAction::RestoreTranslatedOutput(d) => self.restore_translated(d),
            CallAction::PauseDirections(dirs) => {
                for d in dirs.iter() {
                    self.feed_dir(d, DirectionEvent::Pause);
                }
            }
            CallAction::ResumeDirections(dirs) => {
                for d in dirs.iter() {
                    self.feed_dir(d, DirectionEvent::Resume);
                }
            }
            CallAction::SuspendDirections(dirs) => {
                for d in dirs.iter() {
                    self.feed_dir(d, DirectionEvent::Suspend);
                }
            }
            CallAction::ReopenEndpoints => self.reopen_after_wake(),
            CallAction::CancelAll => self.recovery = None,
            CallAction::StopDirections => self.stop_directions(),
            CallAction::ForceClose => self.force_close(),
            CallAction::ReportLeak => {
                self.raise(
                    CallErrorCode::StopLeak,
                    "something was still running after the stop timeout",
                );
            }
        }
    }

    /// Ask the host to prepare the providers and report the result to the machine.
    fn prepare_providers(&mut self) {
        if !matches!(
            self.state,
            CallState::Initializing { .. } | CallState::AudioReady { .. }
        ) {
            return;
        }
        match self.host.prepare_providers() {
            Ok(()) => {
                self.feed(CallEvent::ProvidersReady);
            }
            Err((role, fault)) => {
                self.provider_role = Some(role);
                self.raise_fault(&fault);
                self.feed(CallEvent::ProvidersFailed {
                    role,
                    fault: fault.code.fault_kind(),
                });
            }
        }
    }

    /// Put the default input back (the first thing a stop does) and stop reading call activity.
    pub(super) fn restore_routing(&mut self) {
        if self.activity_probe_on {
            self.host.stop_activity_probe();
            self.activity_probe_on = false;
        }
        if !self.routing.in_call_mode {
            return;
        }
        match self.host.leave_call_mode() {
            Ok(left) => {
                self.routing.in_call_mode = false;
                self.routing.default_input_is_ours = false;
                self.routing.displaced_to = None;
                self.routing.restored_to = left.restored_to;
                self.clear(CallErrorCode::DefaultInputDisplaced);
                if left.previous_gone {
                    tracing::info!(
                        "the previous microphone is gone; the policy's choice was restored"
                    );
                }
            }
            Err(fault) => {
                // Call mode and the journal stay for the next start or launch.
                self.raise_fault(&HostFault::new(CallErrorCode::RestoreFailed, fault.detail));
            }
        }
    }

    /// Stop both paths; a path that does not end in time is a leak. Then finish the stop.
    fn close_endpoints(&mut self) {
        self.recovery = None;
        // The feeder (outgoing) goes last: the incoming path stops first.
        for d in [Direction::Incoming, Direction::Outgoing] {
            let lane = &mut self.lanes[slot(d)];
            lane.stats = None;
            if let Some(mut path) = lane.path.take()
                && let Err(fault) = path.stop()
            {
                self.leaked = true;
                self.raise_fault(&fault);
            }
        }
        self.routing.default_input_is_ours = false;
        let event = if std::mem::take(&mut self.leaked) {
            CallEvent::StopTimeout
        } else {
            CallEvent::Stopped
        };
        if matches!(self.state, CallState::Stopping) {
            self.feed(event);
        }
    }

    /// Drop what is left without waiting (after a stop timeout).
    fn force_close(&mut self) {
        for lane in &mut self.lanes {
            lane.path = None;
            lane.stats = None;
        }
    }

    /// Show a call-level reason as an issue.
    fn report_reason(&mut self, reason: CallReason) {
        match reason {
            CallReason::Direction { direction, reason } => self.raise(
                CallErrorCode::DirectionDegraded,
                format!("{direction:?} direction: {reason:?}"),
            ),
            CallReason::DirectionFailed(direction) => self.raise(
                CallErrorCode::TranslationUnavailable,
                format!("{direction:?} direction failed"),
            ),
            CallReason::Providers(role) => self.raise(
                CallErrorCode::TranslationUnavailable,
                format!("provider not ready: {role:?}"),
            ),
            CallReason::EndpointMissing(endpoint) => self.raise(
                CallErrorCode::RecoveryTimeout,
                format!("{endpoint:?} did not come back"),
            ),
        }
    }
}

/// The pipeline stage a provider role belongs to.
pub(super) const fn stage_of(role: ProviderRole) -> Stage {
    match role {
        ProviderRole::Vad => Stage::Vad,
        ProviderRole::Asr => Stage::Asr,
        ProviderRole::AsrRefine => Stage::Refine,
        ProviderRole::Mt => Stage::Mt,
        ProviderRole::Tts => Stage::Tts,
    }
}

/// The fault class recorded when a failure policy is applied for a missing provider.
pub(super) const POLICY_FAULT: FaultKind = FaultKind::Unavailable;
