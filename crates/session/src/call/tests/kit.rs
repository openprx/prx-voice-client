//! Test kit: a scripted host and paths that log every call in one ordered log, and a rig that owns a coordinator, a manual clock and the published snapshots.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_media::{Direction, ManualClock};
use prxvc_ports::BundleId;

use crate::call::{
    ActivityJudgement, AudioPath, CallCommand, CallErrorCode, CallHost, CallSettings, CallSnapshot,
    Coordinator, CoordinatorDeps, EnteredRouting, HostFault, LeftRouting, MicOwnership,
    PathEvent, PathStats, RoutingNotice,
};
use crate::fault::ProviderRole;
use crate::output::OutputSource;

pub const APP: &str = "net.whatsapp.WhatsApp";

#[derive(Default)]
struct Script {
    in_bundle: bool,
    /// Step name to the fault it returns, once.
    fail: HashMap<&'static str, HostFault>,
    /// Step names that fail every time until cleared.
    fail_always: HashMap<&'static str, HostFault>,
    notices: VecDeque<RoutingNotice>,
    activity: ActivityJudgement,
    providers: Option<(ProviderRole, HostFault)>,
    stop_fails: Vec<&'static str>,
    in_call_mode: bool,
}

#[derive(Default)]
struct PathWorld {
    events: HashMap<&'static str, Vec<PathEvent>>,
    stats: HashMap<&'static str, PathStats>,
}

/// Shared state of the fake machine: the ordered log and the script.
#[derive(Clone)]
pub struct World {
    log: Arc<Mutex<Vec<String>>>,
    script: Arc<Mutex<Script>>,
    paths: Arc<Mutex<PathWorld>>,
}

impl World {
    pub fn new() -> Self {
        let script = Script {
            in_bundle: true,
            ..Script::default()
        };
        Self {
            log: Arc::default(),
            script: Arc::new(Mutex::new(script)),
            paths: Arc::default(),
        }
    }

    pub fn log(&self) -> Vec<String> {
        self.log.lock().clone()
    }

    pub fn clear_log(&self) {
        self.log.lock().clear();
    }

    fn record(&self, entry: impl Into<String>) {
        self.log.lock().push(entry.into());
    }

    /// Fail the next call of `step` with `code`.
    pub fn fail_once(&self, step: &'static str, code: CallErrorCode) {
        self.script
            .lock()
            .fail
            .insert(step, HostFault::new(code, format!("{step} failed (scripted)")));
    }

    /// Fail every call of `step` until [`World::heal`].
    pub fn fail_always(&self, step: &'static str, code: CallErrorCode) {
        self.script
            .lock()
            .fail_always
            .insert(step, HostFault::new(code, format!("{step} failed (scripted)")));
    }

    pub fn heal(&self, step: &'static str) {
        self.script.lock().fail_always.remove(step);
    }

    pub fn set_outside_bundle(&self) {
        self.script.lock().in_bundle = false;
    }

    pub fn push_notice(&self, notice: RoutingNotice) {
        self.script.lock().notices.push_back(notice);
    }

    pub fn set_activity(&self, activity: ActivityJudgement) {
        self.script.lock().activity = activity;
    }

    pub fn fail_providers(&self, role: ProviderRole) {
        self.script.lock().providers = Some((
            role,
            HostFault::new(CallErrorCode::TranslationUnavailable, "no providers (scripted)"),
        ));
    }

    /// The next `stop` of the path called `name` ("outgoing" or "incoming") reports a leak.
    pub fn path_stop_leaks(&self, name: &'static str) {
        self.script.lock().stop_fails.push(name);
    }

    pub fn push_path_event(&self, name: &'static str, event: PathEvent) {
        self.paths.lock().events.entry(name).or_default().push(event);
    }

    pub fn set_path_stats(&self, name: &'static str, stats: PathStats) {
        self.paths.lock().stats.insert(name, stats);
    }

    pub fn in_call_mode(&self) -> bool {
        self.script.lock().in_call_mode
    }

    fn step(&self, name: &'static str) -> Result<(), HostFault> {
        self.record(name);
        let mut s = self.script.lock();
        if let Some(f) = s.fail_always.get(name) {
            return Err(f.clone());
        }
        match s.fail.remove(name) {
            Some(f) => Err(f),
            None => Ok(()),
        }
    }
}

pub struct FakePath {
    name: &'static str,
    world: World,
}

impl AudioPath for FakePath {
    fn set_listener_hears(&mut self, hears: OutputSource) {
        self.world.record(format!("{}.hears:{hears:?}", self.name));
    }

    fn stats(&self) -> PathStats {
        let mut stats = self
            .world
            .paths
            .lock()
            .stats
            .get(self.name)
            .cloned()
            .unwrap_or_default();
        stats.running = true;
        if stats.source_device.is_empty() {
            stats.source_device = format!("{}-source", self.name);
        }
        if self.name == "outgoing" && stats.sink_device.is_none() {
            stats.sink_device = Some("virtual-sink".to_owned());
        }
        stats
    }

    fn take_events(&mut self) -> Vec<PathEvent> {
        self.world
            .paths
            .lock()
            .events
            .remove(self.name)
            .unwrap_or_default()
    }

    fn stop(&mut self) -> Result<(), HostFault> {
        self.world.record(format!("{}.stop", self.name));
        let mut s = self.world.script.lock();
        if let Some(i) = s.stop_fails.iter().position(|n| *n == self.name) {
            s.stop_fails.remove(i);
            return Err(HostFault::new(CallErrorCode::StopLeak, "did not end (scripted)"));
        }
        Ok(())
    }
}

pub struct FakeHost {
    world: World,
}

impl FakeHost {
    pub fn new(world: World) -> Self {
        Self { world }
    }
}

impl CallHost for FakeHost {
    fn in_app_bundle(&self) -> bool {
        self.world.record("bundle");
        self.world.script.lock().in_bundle
    }

    fn ensure_microphone_consent(&mut self) -> Result<(), HostFault> {
        self.world.step("mic_consent")
    }

    fn ensure_capture_consent(&mut self) -> Result<(), HostFault> {
        self.world.step("capture_consent")
    }

    fn check_driver(&mut self, target: &BundleId) -> Result<(), HostFault> {
        self.world.record(format!("driver:{target}"));
        self.world.step("driver")
    }

    fn check_microphone(&mut self) -> Result<String, HostFault> {
        self.world.step("mic_check").map(|()| "Built-in Mic".to_owned())
    }

    fn start_outgoing(&mut self) -> Result<Box<dyn AudioPath>, HostFault> {
        self.world.step("start_outgoing")?;
        Ok(Box::new(FakePath {
            name: "outgoing",
            world: self.world.clone(),
        }))
    }

    fn enter_call_mode(&mut self, _target: &BundleId) -> Result<EnteredRouting, HostFault> {
        self.world.step("enter")?;
        self.world.script.lock().in_call_mode = true;
        Ok(EnteredRouting {
            saved_input: Some("Studio Mic".to_owned()),
            physical_input: "Built-in Mic".to_owned(),
        })
    }

    fn start_incoming(&mut self, _target: &BundleId) -> Result<Box<dyn AudioPath>, HostFault> {
        self.world.step("start_incoming")?;
        Ok(Box::new(FakePath {
            name: "incoming",
            world: self.world.clone(),
        }))
    }

    fn start_activity_probe(&mut self, _target: &BundleId) -> Result<(), HostFault> {
        self.world.step("probe")
    }

    fn call_activity(&mut self) -> ActivityJudgement {
        self.world.script.lock().activity.clone()
    }

    fn stop_activity_probe(&mut self) {
        self.world.record("probe.stop");
    }

    fn poll_routing(&mut self) -> Vec<RoutingNotice> {
        self.world.record("poll");
        self.world.script.lock().notices.drain(..).collect()
    }

    fn reassert_routing(&mut self) -> Result<(), HostFault> {
        self.world.step("reassert")
    }

    fn leave_call_mode(&mut self) -> Result<LeftRouting, HostFault> {
        self.world.step("leave")?;
        self.world.script.lock().in_call_mode = false;
        Ok(LeftRouting {
            restored_to: Some("Studio Mic".to_owned()),
            left_alone: false,
            previous_gone: false,
            was_not_in_call_mode: false,
        })
    }

    fn prepare_providers(&mut self) -> Result<(), (ProviderRole, HostFault)> {
        self.world.record("providers");
        match self.world.script.lock().providers.clone() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

/// A coordinator with its fake machine, clock and collected snapshots.
pub struct Rig {
    pub coord: Coordinator,
    pub world: World,
    pub clock: ManualClock,
    pub snapshots: Arc<Mutex<Vec<CallSnapshot>>>,
    pub ownership: MicOwnership,
}

impl Rig {
    pub fn new() -> Self {
        Self::with(CallSettings::default())
    }

    pub fn with(settings: CallSettings) -> Self {
        let world = World::new();
        let clock = ManualClock::new();
        let snapshots: Arc<Mutex<Vec<CallSnapshot>>> = Arc::default();
        let ownership = MicOwnership::new();
        let sink = {
            let snapshots = Arc::clone(&snapshots);
            Box::new(move |s: &CallSnapshot| snapshots.lock().push(s.clone()))
        };
        let coord = Coordinator::new(CoordinatorDeps {
            host: Box::new(FakeHost {
                world: world.clone(),
            }),
            clock: Arc::new(clock.clone()),
            ownership: ownership.clone(),
            sink,
            settings: CallSettings {
                default_target: None,
                ..settings
            },
        });
        Self {
            coord,
            world,
            clock,
            snapshots,
            ownership,
        }
    }

    /// Start call mode (audio only) on the test application.
    pub fn start(&mut self) {
        self.coord.handle(CallCommand::Start {
            target_app: Some(BundleId::new(APP)),
            translate: false,
        });
    }

    /// Start with translation requested.
    pub fn start_translating(&mut self) {
        self.coord.handle(CallCommand::Start {
            target_app: Some(BundleId::new(APP)),
            translate: true,
        });
    }

    pub fn stop(&mut self) {
        self.coord.handle(CallCommand::Stop);
    }

    /// Advance the clock and run one tick.
    pub fn tick_after(&mut self, secs: u64) {
        self.clock.advance(std::time::Duration::from_secs(secs));
        self.coord.tick();
    }

    pub fn last(&self) -> CallSnapshot {
        self.snapshots.lock().last().cloned().expect("a snapshot was published")
    }

    pub fn log(&self) -> Vec<String> {
        self.world.log()
    }

    pub fn has_issue(&self, code: CallErrorCode) -> bool {
        self.last().issues.iter().any(|i| i.code == code)
    }

    pub fn hears(&self, d: Direction) -> OutputSource {
        self.last().direction(d).hears
    }
}

/// The log entries that name host steps, without the per-path and probe noise.
pub fn steps(log: &[String]) -> Vec<&str> {
    log.iter()
        .map(String::as_str)
        .filter(|e| !e.contains('.') && !e.starts_with("driver:") && *e != "poll")
        .collect()
}
