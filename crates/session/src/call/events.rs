//! What happens while call mode runs: path events, the once-a-second poll of routing and call activity, sleep and wake, and recovery.
//!
//! Policies, in one place:
//!
//! * `VirtualMicrophoneLost`: the default input is restored at once (the user
//!   must not be left with a dead microphone), the outgoing path counts as lost
//!   and recovery keeps trying to reopen it and to re-enter call mode.
//! * `DefaultInputDisplaced`: shown, never undone by the coordinator; the
//!   explicit `ReapplyVirtualMicrophone` command re-applies it.
//! * Capture without signal (zero watchdog), a stalled tap, the call
//!   application quitting, microphone changes: shown as issues, the call keeps
//!   its state. A path that ends (its source failed) is an endpoint loss and
//!   goes through recovery.
//! * Sleep: the call goes to `Recovering`; on wake every path is reopened.
//! * Recovery gives up after `recovery_timeout` (the machine then degrades the call).

use std::collections::BTreeSet;

use prxvc_media::{Direction, MonoTime};

use super::coordinator::{BOTH, Coordinator, slot};
use super::error::CallErrorCode;
use super::host::{ActivityJudgement, PathEvent, RoutingNotice};
use super::start::endpoint_of;
use crate::call_state::{CallEvent, CallState};
use crate::fault::Endpoint;

/// Progress of a recovery.
#[derive(Debug)]
pub(super) struct Recovery {
    /// When it began (or when the machine woke, after sleep).
    pub since: MonoTime,
    /// The last attempt to reopen.
    pub last_attempt: Option<MonoTime>,
    /// Paths still to reopen.
    pub reopen: BTreeSet<Direction>,
    /// The machine sleeps: no attempts, no timeout.
    pub sleeping: bool,
    /// Call mode was left (virtual microphone lost) and must be entered again.
    pub routing_left: bool,
    /// The endpoint named when recovery times out.
    pub missing: Endpoint,
}

impl Coordinator {
    /// Read counters and events from both paths.
    pub(super) fn collect_paths(&mut self) {
        for d in BOTH {
            let lane = &mut self.lanes[slot(d)];
            let Some(path) = lane.path.as_mut() else {
                continue;
            };
            let stats = path.stats();
            let grew = lane.stats.as_ref().is_some_and(|old| stats.frames > old.frames);
            lane.stats = Some(stats);
            let events = path.take_events();
            if grew {
                self.clear(CallErrorCode::CaptureStalled);
            }
            for event in events {
                self.on_path_event(d, event);
            }
        }
    }

    fn on_path_event(&mut self, d: Direction, event: PathEvent) {
        match event {
            PathEvent::Lost(fault) => {
                let default = match d {
                    Direction::Outgoing => Endpoint::LocalInput,
                    Direction::Incoming => Endpoint::RemoteInput,
                };
                let endpoint = endpoint_of(fault.code, default);
                self.raise(CallErrorCode::EndpointLost(endpoint), fault.detail);
                self.lose_path(d, endpoint, false);
            }
            PathEvent::Fault(fault) => self.raise_fault(&fault),
            PathEvent::NoSignal(detail) => self.raise(CallErrorCode::CaptureNoSignal, detail),
            PathEvent::SignalRestored => {
                self.clear(CallErrorCode::CaptureNoSignal);
                self.clear(CallErrorCode::CaptureConsentDenied);
            }
            PathEvent::TargetGone => {
                self.raise(CallErrorCode::TargetGone, "the call application quit");
            }
            PathEvent::TargetBack => self.clear(CallErrorCode::TargetGone),
            PathEvent::Stalled { idle_ms } => self.raise(
                CallErrorCode::CaptureStalled,
                format!("no audio buffers for {idle_ms} ms"),
            ),
            PathEvent::DeviceChanged(name) => {
                self.clear(CallErrorCode::MicrophoneUnavailable);
                self.raise(CallErrorCode::MicrophoneChanged, name.clone());
                self.devices.microphone = Some(name);
            }
            PathEvent::DeviceUnavailable(message) => {
                self.devices.microphone = None;
                self.raise(CallErrorCode::MicrophoneUnavailable, message);
            }
        }
    }

    /// A path is gone (or must be treated as gone): enter or extend recovery.
    fn lose_path(&mut self, d: Direction, endpoint: Endpoint, routing_left: bool) {
        let now = self.now();
        let recovery = self.recovery.get_or_insert_with(|| Recovery {
            since: now,
            last_attempt: None,
            reopen: BTreeSet::new(),
            sleeping: false,
            routing_left: false,
            missing: endpoint,
        });
        recovery.reopen.insert(d);
        recovery.routing_left |= routing_left;
        if matches!(
            self.state,
            CallState::AudioReady { .. } | CallState::Translating | CallState::Paused { .. } | CallState::Degraded { .. }
        ) {
            self.feed(CallEvent::EndpointLost(endpoint));
        }
    }

    /// Poll routing and call activity (about once a second).
    pub(super) fn poll_system(&mut self) {
        let active = matches!(
            self.state,
            CallState::AudioReady { .. }
                | CallState::Translating
                | CallState::Paused { .. }
                | CallState::Degraded { .. }
                | CallState::Recovering { .. }
        );
        if !active {
            return;
        }
        for notice in self.host.poll_routing() {
            self.on_routing_notice(notice);
        }
        if self.activity_probe_on {
            let judgement = self.host.call_activity();
            self.on_activity(judgement);
        }
    }

    fn on_activity(&mut self, judgement: ActivityJudgement) {
        self.clear(CallErrorCode::CallOnOtherMicrophone);
        self.clear(CallErrorCode::ActivityUnavailable);
        match &judgement {
            ActivityJudgement::ActiveOnOtherMicrophone { devices } => self.raise(
                CallErrorCode::CallOnOtherMicrophone,
                format!("the call application records from {}", devices.join(", ")),
            ),
            ActivityJudgement::Unavailable => {
                self.raise(CallErrorCode::ActivityUnavailable, "call detection is not available");
            }
            _ => {}
        }
        self.routing.call_activity = judgement;
    }

    fn on_routing_notice(&mut self, notice: RoutingNotice) {
        match notice {
            RoutingNotice::DefaultInputDisplaced { to } => {
                self.routing.default_input_is_ours = false;
                self.raise(CallErrorCode::DefaultInputDisplaced, format!("the default input is now {to}"));
                self.routing.displaced_to = Some(to);
            }
            RoutingNotice::OwnMicrophoneReselected => {
                self.routing.default_input_is_ours = true;
                self.routing.displaced_to = None;
                self.clear(CallErrorCode::DefaultInputDisplaced);
            }
            RoutingNotice::RestoreTargetChanged { target } => self.routing.restore_target = target,
            RoutingNotice::VirtualMicrophoneLost => self.on_virtual_microphone_lost(),
            RoutingNotice::DefaultOutputChanged { to } => self.routing.default_output = Some(to),
            RoutingNotice::SystemSleeping => self.on_sleep(),
            RoutingNotice::SystemWoke { default_input_intact } => {
                tracing::info!(default_input_intact, "system woke");
                self.on_wake();
            }
        }
    }

    /// The virtual microphone is gone: restore the user's microphone now, then try to bring call mode back.
    fn on_virtual_microphone_lost(&mut self) {
        self.raise(CallErrorCode::VirtualMicrophoneLost, "the virtual microphone disappeared");
        let was_in_call_mode = self.routing.in_call_mode;
        self.restore_routing();
        self.lose_path(Direction::Outgoing, Endpoint::RemoteOutput, was_in_call_mode);
    }

    fn on_sleep(&mut self) {
        let now = self.now();
        let reopen: BTreeSet<Direction> = BOTH
            .into_iter()
            .filter(|d| self.lanes[slot(*d)].path.is_some())
            .collect();
        let routing_left = self.recovery.as_ref().is_some_and(|r| r.routing_left);
        self.recovery = Some(Recovery {
            since: now,
            last_attempt: None,
            reopen,
            sleeping: true,
            routing_left,
            missing: Endpoint::LocalInput,
        });
        if !self.feed(CallEvent::SystemWillSleep) && !matches!(self.state, CallState::Recovering { .. }) {
            self.recovery = None;
        }
    }

    fn on_wake(&mut self) {
        if !matches!(self.state, CallState::Recovering { .. }) {
            return;
        }
        self.feed(CallEvent::SystemDidWake);
    }

    /// `ReopenEndpoints` after wake: every path is reopened.
    pub(super) fn reopen_after_wake(&mut self) {
        let now = self.now();
        if let Some(recovery) = self.recovery.as_mut() {
            recovery.sleeping = false;
            recovery.since = now;
            recovery.reopen = BOTH
                .into_iter()
                .filter(|d| self.lanes[slot(*d)].path.is_some() || recovery.reopen.contains(d))
                .collect();
        }
        self.advance_recovery(now, true);
    }

    /// Try to reopen what is lost; finish recovery when nothing is left, give up on timeout.
    pub(super) fn advance_recovery(&mut self, now: MonoTime, force: bool) {
        if !matches!(self.state, CallState::Recovering { .. }) {
            self.recovery = None;
            return;
        }
        let Some(recovery) = self.recovery.as_ref() else {
            return;
        };
        if recovery.sleeping {
            return;
        }
        let due = recovery
            .last_attempt
            .is_none_or(|t| now.saturating_since(t) >= self.settings.reopen_interval);
        if force || due {
            self.attempt_reopen(now);
        }
        let Some(recovery) = self.recovery.as_ref() else {
            return;
        };
        if recovery.reopen.is_empty() && !recovery.routing_left {
            self.recovery = None;
            self.feed(CallEvent::EndpointsRestored);
        } else if now.saturating_since(recovery.since) >= self.settings.recovery_timeout {
            let missing = recovery.missing;
            self.recovery = None;
            self.raise(CallErrorCode::RecoveryTimeout, format!("{missing:?} did not come back"));
            self.feed(CallEvent::RecoveryTimeout { missing });
        }
    }

    fn attempt_reopen(&mut self, now: MonoTime) {
        let Some(recovery) = self.recovery.as_mut() else {
            return;
        };
        recovery.last_attempt = Some(now);
        let wanted: Vec<Direction> = recovery.reopen.iter().copied().collect();
        for d in wanted {
            if self.reopen_path(d)
                && let Some(recovery) = self.recovery.as_mut()
            {
                recovery.reopen.remove(&d);
            }
        }
        let routing_left = self.recovery.as_ref().is_some_and(|r| r.routing_left);
        let outgoing_pending = self
            .recovery
            .as_ref()
            .is_some_and(|r| r.reopen.contains(&Direction::Outgoing));
        if routing_left && !outgoing_pending {
            self.reenter_call_mode();
        }
    }

    /// Stop the old path (its failure is of no interest now) and start a new one.
    fn reopen_path(&mut self, d: Direction) -> bool {
        if let Some(mut old) = self.lanes[slot(d)].path.take() {
            let _ = old.stop(); // a path that is being replaced is already unusable
        }
        let Some(target) = self.target.clone() else {
            return false;
        };
        let started = match d {
            Direction::Outgoing => self.host.start_outgoing(),
            Direction::Incoming => self.host.start_incoming(&target),
        };
        match started {
            Ok(mut path) => {
                let lane = &mut self.lanes[slot(d)];
                path.set_listener_hears(lane.hears);
                lane.stats = Some(path.stats());
                lane.path = Some(path);
                self.clear(CallErrorCode::EndpointLost(match d {
                    Direction::Outgoing => Endpoint::LocalInput,
                    Direction::Incoming => Endpoint::RemoteInput,
                }));
                self.clear(CallErrorCode::EndpointLost(Endpoint::RemoteOutput));
                true
            }
            Err(fault) => {
                tracing::warn!(direction = ?d, %fault, "reopening a path failed; will retry");
                false
            }
        }
    }

    fn reenter_call_mode(&mut self) {
        let Some(target) = self.target.clone() else {
            return;
        };
        match self.host.enter_call_mode(&target) {
            Ok(entered) => {
                self.routing.in_call_mode = true;
                self.routing.default_input_is_ours = true;
                self.routing.displaced_to = None;
                self.routing.saved_input = entered.saved_input;
                self.clear(CallErrorCode::VirtualMicrophoneLost);
                if let Some(recovery) = self.recovery.as_mut() {
                    recovery.routing_left = false;
                }
            }
            Err(fault) => tracing::warn!(%fault, "entering call mode again failed; will retry"),
        }
    }
}
