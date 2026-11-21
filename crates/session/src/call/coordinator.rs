//! The call coordinator: single writer of the call machine and both direction machines, driven by commands and a periodic tick, publishing a snapshot after every change.
//!
//! The coordinator itself has no thread: [`Coordinator::handle`] applies one
//! command and [`Coordinator::tick`] does the periodic work (collect path
//! counters and events, poll the routing controller about once a second,
//! advance recovery). [`super::runner`] gives it its thread; tests call both
//! directly with an injected clock. The pieces of the work live in sibling
//! files: [`super::start`] (start-up sequence), [`super::actions`] (executing
//! the machines' actions), [`super::directions`] (driving the direction
//! machines and what each listener hears), [`super::events`] (routing, path
//! and recovery events).

use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{Clock, Direction, MonoTime};
use prxvc_ports::{BundleId, CancellationToken};

use super::error::{CallErrorCode, CallIssue, HostFault};
use super::events::Recovery;
use super::host::{ActivityJudgement, AudioPath, CallHost, PathStats, PreparedProviders};
use super::ownership::{MicClaim, MicOwnership};
use super::snapshot::{
    CallCommand, CallSnapshot, DeviceNames, DirectionSnapshot, LocalOutputMode, OutputChange,
    RoutingSnapshot,
};
use super::translation::Running;
use crate::call_state::{CallEvent, CallState};
use crate::dir_set::DirSet;
use crate::direction_state::DirectionState;
use crate::fault::ProviderRole;
use crate::output::{DirectionConfig, OutputPolicy, OutputSource};

/// Settings the coordinator reads (from `call.*` and `[pipeline]`).
#[derive(Clone, Debug)]
pub struct CallSettings {
    /// What the remote party hears while paused or after a failure.
    pub outgoing: OutputPolicy,
    /// What the local user hears while paused or after a failure.
    pub incoming: OutputPolicy,
    /// What the local user hears of the call application.
    pub local_output_mode: LocalOutputMode,
    /// The call application when a start command names none.
    pub default_target: Option<BundleId>,
    /// Time between polls of the routing controller and the call activity.
    pub routing_poll: Duration,
    /// How long recovery may take before the call is degraded (design: 10 s).
    pub recovery_timeout: Duration,
    /// Time between attempts to reopen a lost path.
    pub reopen_interval: Duration,
}

impl Default for CallSettings {
    fn default() -> Self {
        Self {
            outgoing: OutputPolicy::default(),
            incoming: OutputPolicy::default(),
            local_output_mode: LocalOutputMode::default(),
            default_target: None,
            routing_poll: Duration::from_secs(1),
            recovery_timeout: Duration::from_secs(10),
            reopen_interval: Duration::from_secs(1),
        }
    }
}

/// Where snapshots go.
pub type SnapshotSink = Box<dyn FnMut(&CallSnapshot) + Send>;

/// What a coordinator is built from.
pub struct CoordinatorDeps {
    /// The machine it runs on.
    pub host: Box<dyn CallHost>,
    /// Time source (a manual clock in tests).
    pub clock: Arc<dyn Clock>,
    /// The record that keeps call mode and an assistant session apart.
    pub ownership: MicOwnership,
    /// Receives every changed snapshot.
    pub sink: SnapshotSink,
    /// Configuration.
    pub settings: CallSettings,
}

/// One direction: its machine, what its listener hears and its path.
pub(super) struct Lane {
    pub machine: DirectionState,
    pub cfg: DirectionConfig,
    pub hears: OutputSource,
    /// The listener hears the original because the speaker is already in the listener's language
    /// (a pipeline's passthrough); any announced output change ends it.
    pub passthrough: bool,
    pub path: Option<Box<dyn AudioPath>>,
    pub stats: Option<PathStats>,
    /// The translation pipeline of this direction, when it runs one.
    pub running: Option<Running>,
    /// What the direction came to, captured as its pipeline stopped (for the call summary).
    pub totals: super::summary::DirectionTotals,
}

impl Lane {
    fn new(direction: Direction, policy: OutputPolicy) -> Self {
        Self {
            machine: DirectionState::Off,
            cfg: DirectionConfig { direction, policy },
            hears: OutputSource::Original,
            passthrough: false,
            path: None,
            stats: None,
            running: None,
            totals: super::summary::DirectionTotals::default(),
        }
    }
}

/// Index of a direction in the lane array.
pub(super) const fn slot(d: Direction) -> usize {
    match d {
        Direction::Outgoing => 0,
        Direction::Incoming => 1,
    }
}

/// Both directions, outgoing first.
pub(super) const BOTH: [Direction; 2] = [Direction::Outgoing, Direction::Incoming];

/// The coordinator. See the module docs.
pub struct Coordinator {
    pub(super) host: Box<dyn CallHost>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) ownership: MicOwnership,
    sink: SnapshotSink,
    pub(super) settings: CallSettings,
    pub(super) state: CallState,
    pub(super) since: MonoTime,
    pub(super) lanes: [Lane; 2],
    pub(super) routing: RoutingSnapshot,
    pub(super) devices: DeviceNames,
    pub(super) issues: Vec<CallIssue>,
    pub(super) last_error: Option<CallIssue>,
    pub(super) last_change: Option<OutputChange>,
    pub(super) target: Option<BundleId>,
    pub(super) translate: bool,
    pub(super) local_output_mode: LocalOutputMode,
    pub(super) claim: Option<MicClaim>,
    pub(super) recovery: Option<Recovery>,
    pub(super) activity_probe_on: bool,
    pub(super) last_poll: MonoTime,
    pub(super) provider_role: Option<ProviderRole>,
    /// Providers prepared and not yet handed to the pipelines.
    pub(super) prepared: Option<PreparedProviders>,
    /// The token of the call: cancelling it cancels every pipeline.
    pub(super) call_cancel: CancellationToken,
    /// A path failed to stop in time during the current stop.
    pub(super) leaked: bool,
    /// The spoken notice to the other party of the call that is running (translated calls with
    /// the setting on).
    pub(super) disclosure: Option<Arc<super::disclosure::Disclosure>>,
    /// The failure of the notice was shown.
    pub(super) disclosure_failure_shown: bool,
    /// When the call that is running began (for the summary at its end).
    pub(super) call_began: Option<MonoTime>,
    published: Option<CallSnapshot>,
    revision: u64,
}

impl Coordinator {
    /// A coordinator in `Idle`; nothing is started and no snapshot is published yet.
    pub fn new(deps: CoordinatorDeps) -> Self {
        let CoordinatorDeps {
            host,
            clock,
            ownership,
            sink,
            settings,
        } = deps;
        let now = clock.now();
        Self {
            host,
            clock,
            ownership,
            sink,
            lanes: [
                Lane::new(Direction::Outgoing, settings.outgoing),
                Lane::new(Direction::Incoming, settings.incoming),
            ],
            local_output_mode: settings.local_output_mode,
            settings,
            state: CallState::Idle,
            since: now,
            routing: RoutingSnapshot::default(),
            devices: DeviceNames::default(),
            issues: Vec::new(),
            last_error: None,
            last_change: None,
            target: None,
            translate: false,
            claim: None,
            recovery: None,
            activity_probe_on: false,
            last_poll: now,
            provider_role: None,
            prepared: None,
            call_cancel: CancellationToken::new(),
            leaked: false,
            disclosure: None,
            disclosure_failure_shown: false,
            call_began: None,
            published: None,
            revision: 0,
        }
    }

    /// The call machine's state.
    pub fn state(&self) -> &CallState {
        &self.state
    }

    /// The current time on the coordinator's clock.
    pub(super) fn now(&self) -> MonoTime {
        self.clock.now()
    }

    /// Apply one command, then publish the snapshot if anything changed.
    pub fn handle(&mut self, command: CallCommand) {
        self.clear(CallErrorCode::CommandRejected);
        match command {
            CallCommand::Start {
                target_app,
                translate,
            } => self.start(target_app, translate),
            CallCommand::Stop => self.stop(),
            CallCommand::Pause(dirs) => self.pause(dirs),
            CallCommand::Resume => self.resume(),
            CallCommand::SetLocalOutputMode(mode) => self.set_local_output_mode(mode),
            CallCommand::Skip(direction) => self.skip(direction),
            CallCommand::ReapplyVirtualMicrophone => self.reassert(),
            CallCommand::SayDisclosure => self.say_disclosure(),
        }
        self.publish();
    }

    /// The periodic work; call it every few hundred milliseconds.
    pub fn tick(&mut self) {
        let now = self.now();
        self.collect_paths();
        self.collect_translation();
        self.check_disclosure();
        self.advance_recovery(now, false);
        if now.saturating_since(self.last_poll) >= self.settings.routing_poll {
            self.last_poll = now;
            self.poll_system();
        }
        self.publish();
    }

    /// Stop call mode if it is on (the application is closing).
    pub fn shutdown(&mut self) {
        self.stop();
        self.publish();
    }

    // ---- commands -------------------------------------------------------

    fn start(&mut self, target_app: Option<BundleId>, translate: bool) {
        if !matches!(self.state, CallState::Idle) {
            self.reject(format!("start while {:?}", self.state));
            return;
        }
        let claim = match self.ownership.claim(super::ownership::MicOwner::Call) {
            Ok(claim) => claim,
            Err(busy) => {
                let fault = HostFault::new(
                    CallErrorCode::AssistantActive,
                    format!("not started: {busy}"),
                );
                self.raise_fault(&fault);
                return;
            }
        };
        self.claim = Some(claim);
        self.reset_for_start(translate);
        let target = target_app.or_else(|| self.settings.default_target.clone());
        self.target.clone_from(&target);
        self.feed(CallEvent::StartRequested { target, translate });
        self.release_claim_when_idle();
    }

    fn stop(&mut self) {
        if matches!(self.state, CallState::Idle | CallState::Stopping) {
            tracing::debug!(state = ?self.state, "stop ignored: call mode is not running");
            return;
        }
        self.feed(CallEvent::StopRequested);
        self.release_claim_when_idle();
    }

    fn pause(&mut self, dirs: DirSet) {
        if dirs.is_empty() || !self.feed(CallEvent::PauseRequested(dirs)) {
            self.reject(format!("pause {dirs:?} while {:?}", self.state));
        }
    }

    fn resume(&mut self) {
        if !self.feed(CallEvent::ResumeRequested) {
            self.reject(format!("resume while {:?}", self.state));
        }
    }

    fn set_local_output_mode(&mut self, mode: LocalOutputMode) {
        self.local_output_mode = mode;
        if let Some(path) = self.lanes[slot(Direction::Incoming)].path.as_mut() {
            path.set_local_output_mode(mode);
        }
    }

    /// Skip: cancel what the direction has in flight and queued (design 3.4).
    fn skip(&mut self, direction: Direction) {
        match self.lanes[slot(direction)].running.as_ref() {
            Some(running) => running.skip(),
            None => self.reject(format!(
                "skip {direction:?}: the direction does not translate"
            )),
        }
    }

    fn reassert(&mut self) {
        if !self.routing.in_call_mode {
            self.reject("reapply microphone while call mode is off".to_owned());
            return;
        }
        match self.host.reassert_routing() {
            Ok(()) => {
                self.routing.default_input_is_ours = true;
                self.routing.displaced_to = None;
                self.clear(CallErrorCode::DefaultInputDisplaced);
            }
            Err(fault) => self.raise_fault(&fault),
        }
    }

    // ---- state ----------------------------------------------------------

    /// Offer `event` to the call machine and execute its actions. Returns
    /// whether the machine accepted it; a rejection is logged and the state kept.
    pub(super) fn feed(&mut self, event: CallEvent) -> bool {
        let state = std::mem::replace(&mut self.state, CallState::Idle);
        let previous = state.clone();
        match state.on(event) {
            Ok((next, actions)) => {
                self.set_state(&previous, next);
                self.apply(actions);
                true
            }
            Err(rejection) => {
                tracing::warn!(%rejection, "call event rejected");
                self.state = rejection.from;
                false
            }
        }
    }

    /// Take `next` as the state after the transition out of `previous` (`self.state` holds a
    /// placeholder while the machine runs, so the previous state is passed in).
    fn set_state(&mut self, previous: &CallState, next: CallState) {
        if let Some((from, to)) = transition(previous, &next) {
            tracing::info!(from = ?from, to = ?to, "call state");
            self.since = self.now();
            if matches!(next, CallState::Idle) {
                self.log_call_summary();
            }
        }
        self.state = next;
        // A snapshot per transition: the UI sees `Initializing` while the
        // start-up sequence still runs.
        self.publish();
    }

    fn reset_for_start(&mut self, translate: bool) {
        self.issues.clear();
        self.last_error = None;
        self.last_change = None;
        self.translate = translate;
        self.provider_role = None;
        self.prepared = None;
        self.new_call_token();
        self.leaked = false;
        self.call_began = Some(self.now());
        self.disclosure = None;
        self.disclosure_failure_shown = false;
        self.recovery = None;
        self.routing.restored_to = None;
        self.routing.call_activity = ActivityJudgement::Unknown;
        self.devices = DeviceNames::default();
        self.lanes = [
            Lane::new(Direction::Outgoing, self.settings.outgoing),
            Lane::new(Direction::Incoming, self.settings.incoming),
        ];
    }

    fn release_claim_when_idle(&mut self) {
        if matches!(self.state, CallState::Idle) {
            self.claim = None;
        }
    }

    // ---- issues ---------------------------------------------------------

    /// Show a condition; one issue per code, the newest detail wins.
    pub(super) fn raise(&mut self, code: CallErrorCode, detail: impl Into<String>) {
        let issue = CallIssue {
            code,
            severity: code.severity(),
            detail: detail.into(),
            since: self.now(),
        };
        tracing::warn!(?code, detail = %issue.detail, "call issue");
        if issue.severity == super::error::Severity::Blocking {
            self.last_error = Some(issue.clone());
        }
        match self.issues.iter_mut().find(|i| i.code == code) {
            Some(existing) => existing.detail = issue.detail,
            None => self.issues.push(issue),
        }
    }

    /// Show a condition that may occur several times with different subjects (one issue per
    /// code and detail), as provider problems do: three models missing are three issues.
    pub(super) fn raise_distinct(&mut self, code: CallErrorCode, detail: impl Into<String>) {
        let detail = detail.into();
        if self
            .issues
            .iter()
            .any(|i| i.code == code && i.detail == detail)
        {
            return;
        }
        tracing::warn!(?code, %detail, "call issue");
        self.issues.push(CallIssue {
            code,
            severity: code.severity(),
            detail,
            since: self.now(),
        });
    }

    pub(super) fn raise_fault(&mut self, fault: &HostFault) {
        self.raise(fault.code, fault.detail.clone());
    }

    /// Withdraw a condition that no longer holds.
    pub(super) fn clear(&mut self, code: CallErrorCode) {
        self.issues.retain(|i| i.code != code);
    }

    fn reject(&mut self, what: String) {
        self.raise(CallErrorCode::CommandRejected, what);
    }

    // ---- snapshot -------------------------------------------------------

    fn build_snapshot(&self) -> CallSnapshot {
        let lane = |d: Direction| {
            let l = &self.lanes[slot(d)];
            DirectionSnapshot {
                direction: d,
                state: l.machine.clone(),
                hears: l.hears,
                path: l.path.as_ref().and(l.stats.clone()),
                pipeline: l.running.as_ref().and_then(Running::snapshot),
                languages: l.running.as_ref().map(Running::languages),
            }
        };
        CallSnapshot {
            revision: self.revision,
            state: self.state.clone(),
            since: self.since,
            translate_enabled: self.translate,
            local_output_mode: self.local_output_mode,
            directions: [lane(Direction::Outgoing), lane(Direction::Incoming)],
            routing: self.routing.clone(),
            devices: self.devices.clone(),
            issues: self.issues.clone(),
            last_error: self.last_error.clone(),
            last_output_change: self.last_change.clone(),
            disclosure: self
                .disclosure
                .as_ref()
                .map(|d| d.snapshot(self.call_began))
                .unwrap_or_default(),
        }
    }

    /// Publish the snapshot when it differs from the last one published.
    pub(super) fn publish(&mut self) {
        let snapshot = self.build_snapshot();
        if self.published.as_ref() == Some(&snapshot) {
            return;
        }
        self.revision += 1;
        let snapshot = CallSnapshot {
            revision: self.revision,
            ..snapshot
        };
        (self.sink)(&snapshot);
        self.published = Some(snapshot);
    }
}

/// The states of a transition that changes the state, `None` when `next` is the state already.
fn transition<'a>(
    previous: &'a CallState,
    next: &'a CallState,
) -> Option<(&'a CallState, &'a CallState)> {
    (previous != next).then_some((previous, next))
}

#[cfg(test)]
mod transition_tests {
    use super::*;

    #[test]
    fn a_transition_names_the_real_previous_state_and_an_unchanged_state_is_not_one() {
        let idle = CallState::Idle;
        let starting = CallState::Translating;
        assert_eq!(transition(&starting, &idle), Some((&starting, &idle)));
        assert_eq!(transition(&idle, &starting), Some((&idle, &starting)));
        assert_eq!(transition(&starting, &starting), None);
        assert_eq!(transition(&idle, &idle), None);
    }
}
