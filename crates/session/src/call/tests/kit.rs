//! Test kit: a scripted host and paths that log every call in one ordered log, and a rig that owns a coordinator, a manual clock and the published snapshots.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_media::{AudioFrame, ClauseId, Direction, Lang, ManualClock, PcmChunk};
use prxvc_ports::fakes::{
    AsrScript, FakeSpeechSynth, FakeStreamingAsr, FakeTraceSink, FakeTranslator, FakeVadProvider,
    MtScript, TtsScript, VadScript,
};
use prxvc_ports::{BundleId, VoiceSelection};

use crate::call::{
    ActivityJudgement, AudioPath, CallCommand, CallErrorCode, CallHost, CallSettings, CallSnapshot,
    Coordinator, CoordinatorDeps, DirectionBuild, EnteredRouting, HostFault, LeftRouting,
    LocalOutputMode, MicOwnership, PathEvent, PathLink, PathStats, PlaybackReport, Playout,
    PreparedProviders, ProvidersFailure, RoutingNotice,
};
use crate::dir_set::DirSet;
use crate::fault::ProviderRole;
use crate::output::OutputSource;
use crate::pipeline::{PipelineParams, PipelineProviders};

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
    providers: Option<(ProviderRole, Vec<HostFault>)>,
    stop_fails: Vec<&'static str>,
    in_call_mode: bool,
    /// Directions the providers are prepared for.
    translated: Option<DirSet>,
    /// Refuse to connect this path to a pipeline.
    connect_fails: Vec<&'static str>,
}

/// What a connected path hands out and takes in, for the tests to drive.
pub struct LinkRecord {
    pub on_frame: Box<dyn FnMut(AudioFrame) + Send>,
    pub on_playback: Box<dyn FnMut(PlaybackReport) + Send>,
    pub tap_rate: prxvc_media::SampleRate,
}

/// What a path's playout received.
#[derive(Clone, Debug, PartialEq)]
pub enum Played {
    Chunk(ClauseId, usize),
    End(ClauseId),
    Flush,
}

/// The scripts and fakes the next `prepare_providers` builds its pipelines from.
pub struct Translation {
    pub vad: HashMap<Direction, VadScript>,
    pub asr: HashMap<Direction, AsrScript>,
    pub mt: FakeTranslator,
    pub tts: FakeSpeechSynth,
    pub trace: FakeTraceSink,
    pub params: Box<dyn Fn(&mut PipelineParams) + Send>,
    /// The spoken notice the outgoing direction carries.
    pub disclosure: Option<crate::call::DisclosureSpec>,
}

#[derive(Default)]
struct PathWorld {
    events: HashMap<&'static str, Vec<PathEvent>>,
    stats: HashMap<&'static str, PathStats>,
    links: HashMap<&'static str, LinkRecord>,
    played: HashMap<&'static str, Vec<Played>>,
    modes: HashMap<&'static str, Vec<LocalOutputMode>>,
}

/// Shared state of the fake machine: the ordered log and the script.
#[derive(Clone)]
pub struct World {
    log: Arc<Mutex<Vec<String>>>,
    script: Arc<Mutex<Script>>,
    paths: Arc<Mutex<PathWorld>>,
    clock: ManualClock,
    translation: Arc<Mutex<Translation>>,
    live_providers: Arc<std::sync::atomic::AtomicUsize>,
}

impl World {
    pub fn new() -> Self {
        Self::with_clock(ManualClock::new())
    }

    pub fn with_clock(clock: ManualClock) -> Self {
        let script = Script {
            in_bundle: true,
            ..Script::default()
        };
        let translation = Translation {
            vad: HashMap::new(),
            asr: HashMap::new(),
            mt: FakeTranslator::new(clock.clone())
                .with_default_script(MtScript::tokens(["Hello."])),
            tts: FakeSpeechSynth::new(clock.clone())
                .with_default_script(TtsScript::chunks(2, 2400)),
            trace: FakeTraceSink::new(false),
            params: Box::new(|_| {}),
            disclosure: None,
        };
        Self {
            log: Arc::default(),
            script: Arc::new(Mutex::new(script)),
            paths: Arc::default(),
            clock,
            translation: Arc::new(Mutex::new(translation)),
            live_providers: Arc::default(),
        }
    }

    /// Provider objects handed to pipelines and not yet dropped.
    pub fn live_providers(&self) -> usize {
        self.live_providers.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Change the fakes the pipelines are built from (before the providers are prepared).
    pub fn with_translation(&self, f: impl FnOnce(&mut Translation)) {
        f(&mut self.translation.lock());
    }

    pub fn trace(&self) -> FakeTraceSink {
        self.translation.lock().trace.clone()
    }

    /// Prepare the providers for these directions only (default: both).
    pub fn translate_only(&self, dirs: DirSet) {
        self.script.lock().translated = Some(dirs);
    }

    /// The next connect of the path called `name` is refused.
    pub fn connect_fails(&self, name: &'static str) {
        self.script.lock().connect_fails.push(name);
    }

    /// Frames from the path called `name`, as its source would deliver them.
    pub fn push_frame(&self, name: &'static str, frame: AudioFrame) {
        let mut paths = self.paths.lock();
        let link = paths.links.get_mut(name).expect("the path is connected");
        (link.on_frame)(frame);
    }

    pub fn report_playback(&self, name: &'static str, report: PlaybackReport) {
        let mut paths = self.paths.lock();
        let link = paths.links.get_mut(name).expect("the path is connected");
        (link.on_playback)(report);
    }

    pub fn is_connected(&self, name: &'static str) -> bool {
        self.paths.lock().links.contains_key(name)
    }

    pub fn tap_rate(&self, name: &'static str) -> Option<prxvc_media::SampleRate> {
        self.paths.lock().links.get(name).map(|l| l.tap_rate)
    }

    pub fn played(&self, name: &'static str) -> Vec<Played> {
        self.paths.lock().played.get(name).cloned().unwrap_or_default()
    }

    pub fn modes(&self, name: &'static str) -> Vec<LocalOutputMode> {
        self.paths.lock().modes.get(name).cloned().unwrap_or_default()
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
        self.script.lock().fail.insert(
            step,
            HostFault::new(code, format!("{step} failed (scripted)")),
        );
    }

    /// Fail every call of `step` until [`World::heal`].
    pub fn fail_always(&self, step: &'static str, code: CallErrorCode) {
        self.script.lock().fail_always.insert(
            step,
            HostFault::new(code, format!("{step} failed (scripted)")),
        );
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
        self.fail_providers_with(
            role,
            vec![HostFault::new(
                CallErrorCode::TranslationUnavailable,
                "no providers (scripted)",
            )],
        );
    }

    pub fn fail_providers_with(&self, role: ProviderRole, faults: Vec<HostFault>) {
        self.script.lock().providers = Some((role, faults));
    }

    /// The next `stop` of the path called `name` ("outgoing" or "incoming") reports a leak.
    pub fn path_stop_leaks(&self, name: &'static str) {
        self.script.lock().stop_fails.push(name);
    }

    pub fn push_path_event(&self, name: &'static str, event: PathEvent) {
        self.paths
            .lock()
            .events
            .entry(name)
            .or_default()
            .push(event);
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

/// A translator that counts how many of it are alive, to prove the providers are released.
struct CountedTranslator {
    inner: FakeTranslator,
    live: Arc<std::sync::atomic::AtomicUsize>,
}

impl CountedTranslator {
    fn new(inner: FakeTranslator, live: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        live.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self { inner, live }
    }
}

impl Drop for CountedTranslator {
    fn drop(&mut self) {
        self.live.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl prxvc_ports::Translator for CountedTranslator {
    fn descriptor(&self) -> prxvc_ports::ProviderDescriptor {
        self.inner.descriptor()
    }

    fn translate(
        &self,
        req: prxvc_ports::MtRequest,
    ) -> Result<Box<dyn prxvc_ports::MtStream>, prxvc_ports::MtError> {
        self.inner.translate(req)
    }
}

/// The playout of a fake path: records what it was given.
struct FakePlayout {
    name: &'static str,
    world: World,
}

impl FakePlayout {
    fn note(&self, p: Played) {
        self.world
            .paths
            .lock()
            .played
            .entry(self.name)
            .or_default()
            .push(p);
    }
}

impl Playout for FakePlayout {
    fn push(&self, clause: ClauseId, chunk: &PcmChunk) {
        self.note(Played::Chunk(clause, chunk.samples.len()));
    }

    fn end_clause(&self, clause: ClauseId) {
        self.note(Played::End(clause));
    }

    fn flush(&self) {
        self.note(Played::Flush);
    }
}

impl AudioPath for FakePath {
    fn connect(&mut self, link: PathLink) -> Result<Arc<dyn Playout>, HostFault> {
        self.world.record(format!("{}.connect", self.name));
        {
            let mut s = self.world.script.lock();
            if let Some(i) = s.connect_fails.iter().position(|n| *n == self.name) {
                s.connect_fails.remove(i);
                return Err(HostFault::new(
                    CallErrorCode::TranslationUnavailable,
                    "cannot connect (scripted)",
                ));
            }
        }
        self.world.paths.lock().links.insert(
            self.name,
            LinkRecord {
                on_frame: link.on_frame,
                on_playback: link.on_playback,
                tap_rate: link.tap_rate,
            },
        );
        Ok(Arc::new(FakePlayout {
            name: self.name,
            world: self.world.clone(),
        }))
    }

    fn disconnect(&mut self) {
        self.world.record(format!("{}.disconnect", self.name));
        self.world.paths.lock().links.remove(self.name);
    }

    fn set_local_output_mode(&mut self, mode: LocalOutputMode) {
        self.world
            .paths
            .lock()
            .modes
            .entry(self.name)
            .or_default()
            .push(mode);
    }

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
            return Err(HostFault::new(
                CallErrorCode::StopLeak,
                "did not end (scripted)",
            ));
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
        self.world
            .step("mic_check")
            .map(|()| "Built-in Mic".to_owned())
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

    fn start_incoming(
        &mut self,
        _target: &BundleId,
        translate: bool,
    ) -> Result<Box<dyn AudioPath>, HostFault> {
        self.world.step("start_incoming")?;
        self.world
            .record(format!("incoming.mode:{}", if translate { "translated" } else { "audio_only" }));
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

    fn prepare_providers(&mut self) -> Result<PreparedProviders, ProvidersFailure> {
        self.world.record("providers");
        if let Some((role, faults)) = self.world.script.lock().providers.clone() {
            return Err(ProvidersFailure { role, faults });
        }
        let dirs = self.world.script.lock().translated.unwrap_or(DirSet::BOTH);
        let t = self.world.translation.lock();
        let clock = self.world.clock.clone();
        let directions = dirs
            .iter()
            .map(|d| {
                let (source, target) = match d {
                    Direction::Outgoing => (Lang::Zh, Lang::En),
                    Direction::Incoming => (Lang::En, Lang::Zh),
                };
                let mut params = PipelineParams::new(
                    d,
                    source,
                    target,
                    VoiceSelection::Preset {
                        name: "preset-a".to_owned(),
                    },
                );
                (t.params)(&mut params);
                let vad = t
                    .vad
                    .get(&d)
                    .cloned()
                    .unwrap_or_else(|| VadScript::probabilities(std::iter::repeat_n(0.0, 4000)));
                let asr = t
                    .asr
                    .get(&d)
                    .cloned()
                    .unwrap_or_else(|| AsrScript::new([], ""));
                DirectionBuild {
                    params,
                    providers: PipelineProviders {
                        vad: Arc::new(FakeVadProvider::new(vad)),
                        asr: Arc::new(FakeStreamingAsr::new(clock.clone(), asr)),
                        refine: None,
                        mt: Arc::new(CountedTranslator::new(
                            t.mt.clone(),
                            Arc::clone(&self.world.live_providers),
                        )),
                        tts: Arc::new(t.tts.clone()),
                    },
                    disclosure: (d == Direction::Outgoing)
                        .then(|| t.disclosure.clone())
                        .flatten(),
                }
            })
            .collect();
        Ok(PreparedProviders {
            directions,
            trace: Arc::new(t.trace.clone()),
        })
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
        let clock = ManualClock::new();
        let world = World::with_clock(clock.clone());
        // The real host builds each direction's output policy from the same configuration.
        let (outgoing, incoming) = (settings.outgoing, settings.incoming);
        world.with_translation(|t| {
            t.params = Box::new(move |p| {
                p.output = match p.direction {
                    Direction::Outgoing => outgoing,
                    Direction::Incoming => incoming,
                };
            });
        });
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

    /// Tick until `ready` holds for the newest snapshot. The pipelines run on their own threads, so
    /// their reports reach the snapshot a little after the command; the loop yields and never sleeps.
    /// The wall-clock deadline is a safety net against a hung test, never part of what is waited for.
    pub fn until(&mut self, what: &str, ready: impl Fn(&CallSnapshot) -> bool) -> CallSnapshot {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            self.coord.tick();
            let s = self.last();
            if ready(&s) {
                return s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for: {what}\n{s:#?}"
            );
            std::thread::yield_now();
        }
    }

    /// Tick until `ready` holds for the fake machine (what the paths received, for instance).
    pub fn until_world(&mut self, what: &str, ready: impl Fn(&World) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !ready(&self.world) {
            self.coord.tick();
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for: {what}\nlog: {:?}\nsnapshot: {:#?}",
                self.world.log(),
                self.last()
            );
            std::thread::yield_now();
        }
    }

    pub fn last(&self) -> CallSnapshot {
        self.snapshots
            .lock()
            .last()
            .cloned()
            .expect("a snapshot was published")
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
