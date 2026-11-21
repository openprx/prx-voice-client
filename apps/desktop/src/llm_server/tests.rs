//! The manager's rules against a fake launcher, fake probe and a clock that only moves when told.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use super::core::{Core, Demand, Deps, ServerState, Settings};
use super::handle::LlmServer;
use super::ports::{Child, Clock, LaunchSpec, Launcher, PortState, Probe, ProcInfo, Processes};
use super::problem::ServerProblem;
use super::record::{ServerRecord, Sweep, sweep};

const ALIAS: &str = "gemma-4-e4b-it-qat-q4-0";

#[derive(Default)]
struct World {
    now_ms: AtomicU64,
    /// What the port holds before we start anything.
    port: Mutex<Option<PortState>>,
    /// Becomes true when a spawned server finished loading.
    healthy: Mutex<bool>,
    spawned: Mutex<Vec<LaunchSpec>>,
    terminated: Mutex<u32>,
    killed: Mutex<u32>,
    /// A child ends by itself when asked to terminate; one that ignores it lives until killed.
    exits_on_terminate: Mutex<bool>,
    crash_now: Mutex<bool>,
    exit_while_starting: Mutex<bool>,
    spawn_error: Mutex<bool>,
}

struct FakeClock(Arc<World>);

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.now_ms.load(Ordering::SeqCst))
    }
    fn unix_ms(&self) -> u64 {
        1_000_000 + self.0.now_ms.load(Ordering::SeqCst)
    }
    fn sleep(&self, d: Duration) {
        self.0
            .now_ms
            .fetch_add(d.as_millis() as u64, Ordering::SeqCst);
    }
}

struct FakeChild {
    world: Arc<World>,
    ended: bool,
}

impl Child for FakeChild {
    fn pid(&self) -> u32 {
        4242
    }
    fn try_exit(&mut self) -> Option<String> {
        if self.ended {
            return Some("exit status: 0".into());
        }
        if *self.world.exit_while_starting.lock()
            || std::mem::take(&mut *self.world.crash_now.lock())
        {
            self.ended = true;
            *self.world.healthy.lock() = false;
            return Some("signal: 11".into());
        }
        None
    }
    fn terminate(&mut self) {
        *self.world.terminated.lock() += 1;
        if *self.world.exits_on_terminate.lock() {
            self.ended = true;
        }
    }
    fn kill(&mut self) {
        *self.world.killed.lock() += 1;
        self.ended = true;
    }
}

struct FakeLauncher(Arc<World>);

impl Launcher for FakeLauncher {
    fn spawn(&self, spec: &LaunchSpec) -> io::Result<Box<dyn Child>> {
        if *self.0.spawn_error.lock() {
            return Err(io::Error::other("no such file"));
        }
        self.0.spawned.lock().push(spec.clone());
        // Once we run, the port is ours.
        *self.0.port.lock() = None;
        Ok(Box::new(FakeChild {
            world: Arc::clone(&self.0),
            ended: false,
        }))
    }
}

struct FakeProbe(Arc<World>);

impl Probe for FakeProbe {
    fn probe(&self, _port: u16, _alias: &str) -> PortState {
        self.0.port.lock().unwrap_or(PortState::Closed)
    }
    fn healthy(&self, _port: u16) -> bool {
        *self.0.healthy.lock()
    }
}

type Resolved = Result<(PathBuf, PathBuf), ServerProblem>;

struct Rig {
    world: Arc<World>,
    demand: Arc<Mutex<Demand>>,
    core: Core,
    dir: tempfile::TempDir,
    resolve_result: Arc<Mutex<Resolved>>,
}

fn settings(dir: &tempfile::TempDir) -> Settings {
    Settings {
        alias: ALIAS.into(),
        port: 8080,
        parallel: 2,
        context: 8192,
        gpu_layers: 99,
        health_timeout: Duration::from_secs(120),
        idle: Duration::from_secs(60),
        stop_grace: Duration::from_secs(10),
        max_restarts: 3,
        restart_window: Duration::from_secs(300),
        log: dir.path().join("llm-server.log"),
        record: dir.path().join("llm-server/server.json"),
    }
}

impl Rig {
    fn new() -> Self {
        let world = Arc::new(World::default());
        *world.exits_on_terminate.lock() = true;
        let dir = tempfile::tempdir().unwrap();
        let resolve_result: Arc<Mutex<Resolved>> = Arc::new(Mutex::new(Ok((
            PathBuf::from("/opt/homebrew/bin/llama-server"),
            PathBuf::from("/models/gemma.gguf"),
        ))));
        let demand = Arc::new(Mutex::new(Demand::default()));
        let r = Arc::clone(&resolve_result);
        let deps = Deps {
            launcher: Arc::new(FakeLauncher(Arc::clone(&world))),
            probe: Arc::new(FakeProbe(Arc::clone(&world))),
            clock: Arc::new(FakeClock(Arc::clone(&world))),
            resolve: Box::new(move || r.lock().clone()),
        };
        let core = Core::new(deps, settings(&dir), Arc::clone(&demand));
        Self {
            world,
            demand,
            core,
            dir,
            resolve_result,
        }
    }

    fn advance(&self, ms: u64) {
        self.world.now_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// Step and move time by `ms` each time, `n` times.
    fn run(&mut self, n: usize, ms: u64) {
        for _ in 0..n {
            self.core.step();
            self.advance(ms);
        }
    }

    fn state(&self) -> ServerState {
        self.core.snapshot().state
    }

    fn state_text(&self) -> String {
        match self.state() {
            ServerState::Failed(p) => p.to_string(),
            other => format!("{other:?}"),
        }
    }

    fn pin(&self, on: bool) {
        let mut d = self.demand.lock();
        d.pinned = on;
        if on {
            d.attempt += 1;
        }
    }

    fn lease(&self, delta: i32) {
        let mut d = self.demand.lock();
        d.leases = (i64::from(d.leases) + i64::from(delta)) as u32;
        if delta > 0 {
            d.attempt += 1;
        }
    }

    fn record_path(&self) -> PathBuf {
        self.dir.path().join("llm-server/server.json")
    }

    fn start_ready(&mut self) {
        self.pin(true);
        self.core.step();
        assert!(matches!(self.state(), ServerState::Starting { .. }));
        *self.world.healthy.lock() = true;
        self.core.step();
        assert_eq!(self.state(), ServerState::Ready { adopted: false });
    }
}

#[test]
fn selecting_the_language_model_starts_the_server_with_the_scripts_arguments() {
    let mut rig = Rig::new();
    assert_eq!(rig.state(), ServerState::Stopped);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped, "no demand, no server");
    rig.start_ready();
    let spec = rig.world.spawned.lock()[0].clone();
    assert_eq!(
        spec.args(),
        [
            "-m",
            "/models/gemma.gguf",
            "--alias",
            ALIAS,
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "--parallel",
            "2",
            "-c",
            "8192",
            "-ngl",
            "99"
        ]
    );
    let record = ServerRecord::read(&rig.record_path()).expect("a record of our process");
    assert_eq!(
        (record.pid, record.port, record.alias.as_str()),
        (4242, 8080, ALIAS)
    );
}

#[test]
fn progress_is_visible_while_the_model_loads() {
    let mut rig = Rig::new();
    rig.pin(true);
    rig.core.step();
    rig.advance(3_000);
    rig.core.step();
    assert_eq!(
        rig.state(),
        ServerState::Starting {
            waited: Duration::from_secs(3)
        }
    );
}

#[test]
fn selecting_system_translation_stops_the_server_at_once_and_removes_the_record() {
    let mut rig = Rig::new();
    rig.start_ready();
    rig.pin(false);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped);
    assert_eq!(*rig.world.terminated.lock(), 1);
    assert_eq!(*rig.world.killed.lock(), 0, "a graceful stop is enough");
    assert!(!rig.record_path().exists());
}

#[test]
fn a_server_that_ignores_the_request_to_stop_is_killed_after_the_grace_period() {
    let mut rig = Rig::new();
    *rig.world.exits_on_terminate.lock() = false;
    rig.start_ready();
    rig.pin(false);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped);
    assert_eq!(*rig.world.killed.lock(), 1);
    assert!(
        rig.world.now_ms.load(Ordering::SeqCst) >= 10_000,
        "the grace period passed first"
    );
}

#[test]
fn a_lease_starts_the_server_on_demand_and_it_stops_after_the_idle_period() {
    let mut rig = Rig::new();
    rig.lease(1);
    rig.core.step();
    *rig.world.healthy.lock() = true;
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Ready { adopted: false });
    rig.lease(-1);
    rig.core.step();
    rig.advance(59_000);
    rig.core.step();
    assert_eq!(
        rig.state(),
        ServerState::Ready { adopted: false },
        "still inside the idle period"
    );
    // A new consumer inside the period keeps it, and the countdown starts again afterwards.
    rig.lease(1);
    rig.core.step();
    rig.lease(-1);
    rig.advance(1_000);
    rig.core.step();
    rig.advance(59_000);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Ready { adopted: false });
    rig.advance(2_000);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped);
    assert_eq!(
        rig.world.spawned.lock().len(),
        1,
        "one process the whole time"
    );
}

#[test]
fn the_pin_and_a_lease_together_keep_it_until_both_are_gone() {
    let mut rig = Rig::new();
    rig.start_ready();
    rig.lease(1);
    rig.pin(false);
    rig.core.step();
    assert_eq!(
        rig.state(),
        ServerState::Ready { adopted: false },
        "the assistant still needs it"
    );
    rig.lease(-1);
    rig.core.step();
    rig.advance(61_000);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped);
}

#[test]
fn a_compatible_server_that_is_already_running_is_adopted_and_never_stopped() {
    let mut rig = Rig::new();
    *rig.world.port.lock() = Some(PortState::Compatible);
    rig.pin(true);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Ready { adopted: true });
    assert!(rig.world.spawned.lock().is_empty());
    assert!(!rig.record_path().exists(), "nothing of ours to record");
    rig.pin(false);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped);
    assert_eq!(*rig.world.terminated.lock() + *rig.world.killed.lock(), 0);
}

#[test]
fn an_adopted_server_that_goes_away_is_replaced_by_ours_while_it_is_wanted() {
    let mut rig = Rig::new();
    *rig.world.port.lock() = Some(PortState::Compatible);
    rig.pin(true);
    rig.core.step();
    *rig.world.port.lock() = Some(PortState::Closed);
    rig.advance(6_000);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Stopped);
    rig.core.step();
    assert!(matches!(rig.state(), ServerState::Starting { .. }));
    assert_eq!(rig.world.spawned.lock().len(), 1);
}

#[test]
fn another_program_on_the_port_is_a_typed_problem_and_nothing_is_started() {
    let mut rig = Rig::new();
    *rig.world.port.lock() = Some(PortState::Foreign);
    rig.pin(true);
    rig.core.step();
    assert_eq!(
        rig.state(),
        ServerState::Failed(ServerProblem::PortTaken { port: 8080 })
    );
    assert!(rig.world.spawned.lock().is_empty());
    rig.run(5, 1_000);
    assert!(rig.world.spawned.lock().is_empty(), "no retry on its own");
}

#[test]
fn a_missing_binary_is_typed_with_guidance_and_retried_only_when_asked() {
    let mut rig = Rig::new();
    let missing = ServerProblem::BinaryMissing {
        searched: vec!["/opt/homebrew/bin/llama-server".into()],
    };
    *rig.resolve_result.lock() = Err(missing.clone());
    rig.pin(true);
    rig.core.step();
    assert_eq!(rig.state(), ServerState::Failed(missing));
    assert!(
        rig.state_text().contains("brew install llama.cpp"),
        "guidance in the message"
    );
    rig.run(3, 1_000);
    assert!(matches!(rig.state(), ServerState::Failed(_)));
    // Fixed: the user presses "Try again".
    *rig.resolve_result.lock() = Ok(("/b/llama-server".into(), "/m/gemma.gguf".into()));
    rig.demand.lock().attempt += 1;
    rig.core.step();
    assert!(matches!(rig.state(), ServerState::Starting { .. }));
}

#[test]
fn a_spawn_error_a_death_while_starting_and_a_health_timeout_are_typed() {
    let mut rig = Rig::new();
    *rig.world.spawn_error.lock() = true;
    rig.pin(true);
    rig.core.step();
    assert!(matches!(
        rig.state(),
        ServerState::Failed(ServerProblem::SpawnFailed { .. })
    ));

    let mut rig = Rig::new();
    rig.pin(true);
    rig.core.step();
    *rig.world.exit_while_starting.lock() = true;
    rig.core.step();
    assert!(matches!(
        rig.state(),
        ServerState::Failed(ServerProblem::ExitedWhileStarting { .. })
    ));
    assert!(!rig.record_path().exists());

    let mut rig = Rig::new();
    rig.pin(true);
    rig.core.step();
    rig.advance(121_000);
    rig.core.step();
    assert!(matches!(
        rig.state(),
        ServerState::Failed(ServerProblem::HealthTimeout { seconds: 120, .. })
    ));
    assert_eq!(
        *rig.world.killed.lock(),
        1,
        "the stuck process does not stay behind"
    );
}

#[test]
fn a_crash_restarts_with_a_growing_delay_and_gives_up_after_the_bound() {
    let mut rig = Rig::new();
    rig.start_ready();
    for expected_spawns in 2..=4 {
        *rig.world.crash_now.lock() = true;
        rig.core.step();
        assert_eq!(rig.state(), ServerState::Stopped);
        // Not before the backoff.
        rig.core.step();
        assert_eq!(rig.state(), ServerState::Stopped, "waiting out the delay");
        rig.advance(10_000);
        rig.core.step();
        assert!(matches!(rig.state(), ServerState::Starting { .. }));
        assert_eq!(rig.world.spawned.lock().len(), expected_spawns);
        *rig.world.healthy.lock() = true;
        rig.core.step();
        assert_eq!(rig.state(), ServerState::Ready { adopted: false });
    }
    *rig.world.crash_now.lock() = true;
    rig.core.step();
    assert_eq!(
        rig.state(),
        ServerState::Failed(ServerProblem::Crashed {
            restarts: 3,
            log: rig.dir.path().join("llm-server.log")
        })
    );
    assert_eq!(rig.world.spawned.lock().len(), 4, "no fifth start");
}

#[test]
fn shutdown_stops_what_we_started_and_nothing_else() {
    let mut rig = Rig::new();
    rig.start_ready();
    rig.core.shutdown();
    assert_eq!(rig.state(), ServerState::Stopped);
    assert_eq!(*rig.world.terminated.lock(), 1);
    assert!(!rig.record_path().exists());

    let mut rig = Rig::new();
    *rig.world.port.lock() = Some(PortState::Compatible);
    rig.pin(true);
    rig.core.step();
    rig.core.shutdown();
    assert_eq!(
        *rig.world.terminated.lock() + *rig.world.killed.lock(),
        0,
        "an adopted server is left alone"
    );
}

// ---- the sweep of a server left behind ----------------------------------------------------

#[derive(Default)]
struct FakeProcesses {
    table: Mutex<HashMap<u32, ProcInfo>>,
    signals: Mutex<Vec<(u32, &'static str)>>,
}

impl Processes for FakeProcesses {
    fn describe(&self, pid: u32) -> Option<ProcInfo> {
        self.table.lock().get(&pid).cloned()
    }
    fn terminate(&self, pid: u32) {
        self.signals.lock().push((pid, "term"));
        self.table.lock().remove(&pid);
    }
    fn kill(&self, pid: u32) {
        self.signals.lock().push((pid, "kill"));
        self.table.lock().remove(&pid);
    }
}

fn record() -> ServerRecord {
    ServerRecord {
        pid: 777,
        started_unix_ms: 1_000_000,
        port: 8080,
        alias: ALIAS.into(),
        binary: "/opt/homebrew/bin/llama-server".into(),
    }
}

fn ours() -> ProcInfo {
    ProcInfo {
        command: format!(
            "/opt/homebrew/bin/llama-server -m /m/gemma.gguf --alias {ALIAS} --host 127.0.0.1 --port 8080 --parallel 2"
        ),
        age_secs: 3_600,
    }
}

const LATER: u64 = 1_000_000 + 3_600_000;

#[test]
fn a_left_behind_server_of_ours_is_stopped_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.json");
    record().write(&path).unwrap();
    let procs = FakeProcesses::default();
    procs.table.lock().insert(777, ours());
    assert_eq!(sweep(&path, &procs, LATER), Sweep::Stopped);
    assert_eq!(*procs.signals.lock(), vec![(777, "term")]);
    assert!(!path.exists());
}

#[test]
fn a_stale_record_never_kills_a_foreign_process() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.json");
    let procs = FakeProcesses::default();
    assert_eq!(sweep(&path, &procs, LATER), Sweep::Nothing);
    // The pid was reused by an unrelated program, or by the owner's own server.
    for foreign in [
        ProcInfo {
            command: "/usr/bin/vim notes.txt".into(),
            age_secs: 3_600,
        },
        ProcInfo {
            command: "/opt/homebrew/bin/llama-server -m x.gguf --alias other --port 8080".into(),
            age_secs: 3_600,
        },
        // Same command line, but started long after the record.
        ProcInfo {
            command: ours().command,
            age_secs: 30,
        },
    ] {
        record().write(&path).unwrap();
        procs.table.lock().insert(777, foreign);
        assert_eq!(sweep(&path, &procs, LATER), Sweep::Stale);
        assert!(procs.signals.lock().is_empty(), "nothing was signalled");
        assert!(!path.exists());
    }
    // The process is gone.
    record().write(&path).unwrap();
    procs.table.lock().clear();
    assert_eq!(sweep(&path, &procs, LATER), Sweep::Stale);
    // A garbled record is removed, not trusted.
    std::fs::write(&path, "{not json").unwrap();
    assert_eq!(sweep(&path, &procs, LATER), Sweep::Stale);
    assert!(procs.signals.lock().is_empty());
}

// ---- the handle: a worker thread over the same fakes --------------------------------------

/// A clock that follows real time, so that the worker thread's waits and the idle period are real.
struct RealTimeClock(std::time::Instant);

impl Clock for RealTimeClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    fn unix_ms(&self) -> u64 {
        1
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

fn threaded(world: &Arc<World>, dir: &tempfile::TempDir, idle_ms: u64) -> LlmServer {
    let mut s = settings(dir);
    s.idle = Duration::from_millis(idle_ms);
    s.stop_grace = Duration::from_millis(500);
    let demand = Arc::new(Mutex::new(Demand::default()));
    let deps = Deps {
        launcher: Arc::new(FakeLauncher(Arc::clone(world))),
        probe: Arc::new(FakeProbe(Arc::clone(world))),
        clock: Arc::new(RealTimeClock(std::time::Instant::now())),
        resolve: Box::new(|| Ok(("/b/llama-server".into(), "/m/gemma.gguf".into()))),
    };
    let core = Core::new(deps, s.clone(), Arc::clone(&demand));
    LlmServer::spawn(core, demand, &s)
}

fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < until {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}

fn world_that_loads() -> Arc<World> {
    let world = Arc::new(World::default());
    *world.exits_on_terminate.lock() = true;
    *world.healthy.lock() = true;
    world
}

#[test]
fn the_handle_starts_on_pin_waits_for_ready_and_stops_on_unpin() {
    let world = world_that_loads();
    let dir = tempfile::tempdir().unwrap();
    let server = threaded(&world, &dir, 100);
    let attempt = server.set_pinned(true);
    server
        .wait_ready(attempt, Duration::from_secs(5))
        .expect("ready");
    assert_eq!(
        server.snapshot().state,
        ServerState::Ready { adopted: false }
    );
    assert!(server.snapshot().pinned);
    server.set_pinned(false);
    eventually("the server stops", || {
        server.snapshot().state == ServerState::Stopped
    });
    assert_eq!(*world.terminated.lock(), 1);
    server.shutdown();
}

#[test]
fn leases_start_on_demand_and_the_last_release_stops_after_the_idle_period() {
    let world = world_that_loads();
    let dir = tempfile::tempdir().unwrap();
    let server = threaded(&world, &dir, 150);
    let a = server.acquire();
    let b = server.acquire();
    a.wait_ready(Duration::from_secs(5))
        .expect("ready for the first");
    b.wait_ready(Duration::from_secs(5))
        .expect("ready for the second");
    assert_eq!(server.snapshot().leases, 2);
    drop(a);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        server.snapshot().state,
        ServerState::Ready { adopted: false },
        "b still holds it"
    );
    drop(b);
    eventually("idle stop", || {
        server.snapshot().state == ServerState::Stopped
    });
    assert_eq!(world.spawned.lock().len(), 1);
    server.shutdown();
}

#[test]
fn a_failure_reaches_the_waiter_and_a_new_request_tries_again() {
    let world = Arc::new(World::default());
    *world.port.lock() = Some(PortState::Foreign);
    let dir = tempfile::tempdir().unwrap();
    let server = threaded(&world, &dir, 100);
    let lease = server.acquire();
    let err = lease.wait_ready(Duration::from_secs(5)).unwrap_err();
    assert_eq!(err, ServerProblem::PortTaken { port: 8080 });
    // The port is free now; a new consumer is a new attempt, not the old failure.
    *world.port.lock() = None;
    *world.healthy.lock() = true;
    let second = server.acquire();
    second
        .wait_ready(Duration::from_secs(5))
        .expect("ready after the cause is gone");
    drop((lease, second));
    server.shutdown();
}

#[test]
fn shutdown_through_the_handle_stops_the_process_we_started() {
    let world = world_that_loads();
    let dir = tempfile::tempdir().unwrap();
    let server = threaded(&world, &dir, 10_000);
    let lease = server.acquire();
    lease.wait_ready(Duration::from_secs(5)).unwrap();
    server.shutdown();
    assert_eq!(*world.terminated.lock(), 1);
    assert!(!dir.path().join("llm-server/server.json").exists());
    drop(lease);
}
