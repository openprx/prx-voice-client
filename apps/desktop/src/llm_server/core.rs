//! The server's life as a state machine, advanced by `Core::step`. It decides from the demand
//! (the pin of the language-model engine and the leases of other consumers) what the server
//! should be doing and does one small piece of it per step, so a worker thread can watch demand
//! changes between steps and tests can drive it with a fake clock.
//!
//! Rules:
//! - Demand is `pinned || leases > 0`. Demand starts the server; no demand stops it: at once when
//!   the pin was just removed (selecting system translation stops the server), after
//!   `idle` when the last lease was released.
//! - Something already answering on the port as a compatible server (healthy, serving the alias)
//!   that we did not start is adopted and never stopped. Anything else on the port is a typed
//!   problem.
//! - Only a process we started is ever stopped: gracefully, then killed after the grace period.
//! - A crash restarts with a growing delay, at most `max_restarts` times in `restart_window`.
//! - A failed start is not retried until demand is raised again (a new lease, the pin, or retry).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::ports::{Child, Clock, LaunchSpec, Launcher, PortState, Probe};
use super::problem::ServerProblem;
use super::record::ServerRecord;

/// Where the server is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ServerState {
    /// Not running (or not ours to run).
    Stopped,
    /// Started, waiting for it to load the model and answer.
    Starting { waited: Duration },
    /// Answering. `adopted` is a server we did not start.
    Ready { adopted: bool },
    /// Being stopped.
    Stopping,
    /// Cannot be provided; the problem says why.
    Failed(ServerProblem),
}

/// What consumers ask for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Demand {
    /// The language-model engine is selected: keep the server running.
    pub pinned: bool,
    /// Consumers (the assistant, a call) that hold the server now.
    pub leases: u32,
    /// Raised by every request for the server; a failed start is retried when it grows.
    pub attempt: u64,
    /// Stop everything and end the worker.
    pub shutdown: bool,
}

impl Demand {
    fn wanted(&self) -> bool {
        self.pinned || self.leases > 0
    }
}

/// The settings of the manager.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub alias: String,
    pub port: u16,
    pub parallel: u32,
    pub context: u32,
    pub gpu_layers: u32,
    pub health_timeout: Duration,
    pub idle: Duration,
    pub stop_grace: Duration,
    pub max_restarts: u32,
    pub restart_window: Duration,
    pub log: PathBuf,
    pub record: PathBuf,
}

/// Resolves the binary and the model file: the part that reads configuration and the store.
pub(crate) type Resolve = Box<dyn Fn() -> Result<(PathBuf, PathBuf), ServerProblem> + Send + Sync>;

/// What the manager is made of.
pub(crate) struct Deps {
    pub launcher: Arc<dyn Launcher>,
    pub probe: Arc<dyn Probe>,
    pub clock: Arc<dyn Clock>,
    pub resolve: Resolve,
}

/// What is published after every step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub state: ServerState,
    pub pinned: bool,
    pub leases: u32,
    /// The newest request the state reflects.
    pub attempt_seen: u64,
    pub port: u16,
}

pub(crate) struct Core {
    deps: Deps,
    settings: Settings,
    demand: Arc<Mutex<Demand>>,
    state: ServerState,
    child: Option<Box<dyn Child>>,
    /// The request the last start attempt answered.
    last_attempt: u64,
    /// The newest request seen by a step.
    seen: u64,
    prev_pinned: bool,
    idle_since: Option<Duration>,
    adopted_checked: Duration,
    started_at: Duration,
    restarts: Vec<Duration>,
    backoff_until: Option<Duration>,
}

/// How often an adopted server is looked at.
const ADOPTED_CHECK: Duration = Duration::from_secs(5);
/// The poll interval while something is changing.
const BUSY: Duration = Duration::from_millis(200);
/// The poll interval while a server runs.
const STEADY: Duration = Duration::from_millis(500);
/// The poll interval while there is nothing to do (a demand change wakes the worker anyway).
const QUIET: Duration = Duration::from_secs(5);

impl Core {
    pub(crate) fn new(deps: Deps, settings: Settings, demand: Arc<Mutex<Demand>>) -> Self {
        Self {
            deps,
            settings,
            demand,
            state: ServerState::Stopped,
            child: None,
            last_attempt: 0,
            seen: 0,
            prev_pinned: false,
            idle_since: None,
            adopted_checked: Duration::ZERO,
            started_at: Duration::ZERO,
            restarts: Vec::new(),
            backoff_until: None,
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        let d = *self.demand.lock();
        Snapshot {
            state: self.state.clone(),
            pinned: d.pinned,
            leases: d.leases,
            attempt_seen: self.seen,
            port: self.settings.port,
        }
    }

    /// One step: look at the demand and the world, do the next piece of work. Returns how long
    /// to wait before the next step (a change of demand ends the wait early).
    pub(crate) fn step(&mut self) -> Duration {
        let now = self.deps.clock.now();
        let d = *self.demand.lock();
        self.seen = d.attempt;
        let unpinned_now = self.prev_pinned && !d.pinned;
        self.prev_pinned = d.pinned;
        let wanted = d.wanted();
        if wanted {
            self.idle_since = None;
        }
        match self.state.clone() {
            ServerState::Stopped => {
                if wanted && self.backoff_until.is_none_or(|t| now >= t) {
                    self.begin_start(now, d.attempt);
                    return BUSY;
                }
                if wanted { BUSY } else { QUIET }
            }
            ServerState::Failed(_) => {
                if !wanted {
                    self.state = ServerState::Stopped;
                } else if d.attempt > self.last_attempt {
                    self.restarts.clear();
                    self.backoff_until = None;
                    self.begin_start(now, d.attempt);
                }
                if matches!(self.state, ServerState::Failed(_) | ServerState::Stopped) {
                    QUIET
                } else {
                    BUSY
                }
            }
            ServerState::Starting { .. } => {
                if wanted {
                    self.poll_starting(now);
                } else {
                    self.stop_owned();
                }
                BUSY
            }
            ServerState::Ready { adopted } => self.poll_ready(now, adopted, wanted, unpinned_now),
            ServerState::Stopping => BUSY,
        }
    }

    fn fail(&mut self, problem: ServerProblem) {
        tracing::warn!(%problem, "the local model server is not available");
        self.state = ServerState::Failed(problem);
    }

    fn spec(&self, binary: PathBuf, model: PathBuf) -> LaunchSpec {
        let s = &self.settings;
        LaunchSpec {
            binary,
            model,
            alias: s.alias.clone(),
            port: s.port,
            parallel: s.parallel,
            context: s.context,
            gpu_layers: s.gpu_layers,
            log: s.log.clone(),
        }
    }

    fn begin_start(&mut self, now: Duration, attempt: u64) {
        self.last_attempt = attempt;
        // Adopt a compatible server that is already there; refuse anything else on the port.
        match self
            .deps
            .probe
            .probe(self.settings.port, &self.settings.alias)
        {
            PortState::Compatible => {
                tracing::info!(
                    port = self.settings.port,
                    "using the model server already running"
                );
                self.adopted_checked = now;
                self.state = ServerState::Ready { adopted: true };
                return;
            }
            PortState::Foreign => {
                self.fail(ServerProblem::PortTaken {
                    port: self.settings.port,
                });
                return;
            }
            PortState::Closed => {}
        }
        let (binary, model) = match (self.deps.resolve)() {
            Ok(found) => found,
            Err(problem) => return self.fail(problem),
        };
        let spec = self.spec(binary.clone(), model);
        match self.deps.launcher.spawn(&spec) {
            Ok(child) => {
                let record = ServerRecord {
                    pid: child.pid(),
                    started_unix_ms: self.deps.clock.unix_ms(),
                    port: spec.port,
                    alias: spec.alias.clone(),
                    binary,
                };
                if let Err(e) = record.write(&self.settings.record) {
                    tracing::warn!(error = %e, "cannot write the model server record");
                }
                tracing::info!(pid = record.pid, port = spec.port, "model server started");
                self.child = Some(child);
                self.state = ServerState::Starting {
                    waited: Duration::ZERO,
                };
                self.started_at = now;
            }
            Err(e) => self.fail(ServerProblem::SpawnFailed {
                detail: e.to_string(),
            }),
        }
    }

    fn poll_starting(&mut self, now: Duration) {
        let exit = self.child.as_mut().and_then(|c| c.try_exit());
        if let Some(exit) = exit {
            self.child = None;
            ServerRecord::remove(&self.settings.record);
            return self.fail(ServerProblem::ExitedWhileStarting {
                exit,
                log: self.settings.log.clone(),
            });
        }
        if self.deps.probe.healthy(self.settings.port) {
            tracing::info!(port = self.settings.port, "model server ready");
            self.state = ServerState::Ready { adopted: false };
            return;
        }
        let waited = now.saturating_sub(self.started_at);
        if waited >= self.settings.health_timeout {
            self.kill_owned();
            return self.fail(ServerProblem::HealthTimeout {
                seconds: self.settings.health_timeout.as_secs(),
                log: self.settings.log.clone(),
            });
        }
        self.state = ServerState::Starting { waited };
    }

    fn poll_ready(
        &mut self,
        now: Duration,
        adopted: bool,
        wanted: bool,
        unpinned_now: bool,
    ) -> Duration {
        if adopted {
            // Never stopped by us; when nobody wants it we simply stop counting on it.
            if !wanted {
                self.state = ServerState::Stopped;
                return QUIET;
            }
            if now.saturating_sub(self.adopted_checked) >= ADOPTED_CHECK {
                self.adopted_checked = now;
                if self
                    .deps
                    .probe
                    .probe(self.settings.port, &self.settings.alias)
                    == PortState::Closed
                {
                    tracing::warn!("the adopted model server went away");
                    self.state = ServerState::Stopped;
                }
            }
            return STEADY;
        }
        let exit = self.child.as_mut().and_then(|c| c.try_exit());
        if let Some(exit) = exit {
            tracing::warn!(%exit, "the model server stopped on its own");
            self.child = None;
            ServerRecord::remove(&self.settings.record);
            self.state = ServerState::Stopped;
            if wanted {
                self.after_crash(now);
            }
            return BUSY;
        }
        if wanted {
            return STEADY;
        }
        // Nobody needs it. Selecting system translation stops it now; the last consumer's
        // release stops it after the idle period.
        let since = *self.idle_since.get_or_insert(now);
        if unpinned_now || now.saturating_sub(since) >= self.settings.idle {
            self.stop_owned();
            return BUSY;
        }
        STEADY
    }

    fn after_crash(&mut self, now: Duration) {
        let window = self.settings.restart_window;
        self.restarts.retain(|t| now.saturating_sub(*t) < window);
        self.restarts.push(now);
        let count = u32::try_from(self.restarts.len()).unwrap_or(u32::MAX);
        if count > self.settings.max_restarts {
            return self.fail(ServerProblem::Crashed {
                restarts: count - 1,
                log: self.settings.log.clone(),
            });
        }
        // 1 s, 2 s, 4 s, ...
        let delay = Duration::from_secs(1 << (count - 1).min(5));
        self.backoff_until = Some(now + delay);
    }

    /// Stop the process we started: ask, wait up to the grace period, then kill. Blocks.
    fn stop_owned(&mut self) {
        let Some(mut child) = self.child.take() else {
            self.state = ServerState::Stopped;
            return;
        };
        self.state = ServerState::Stopping;
        child.terminate();
        let deadline = self.deps.clock.now() + self.settings.stop_grace;
        while child.try_exit().is_none() {
            if self.deps.clock.now() >= deadline {
                child.kill();
                self.deps.clock.sleep(Duration::from_millis(200));
                let _ = child.try_exit();
                break;
            }
            self.deps.clock.sleep(Duration::from_millis(100));
        }
        ServerRecord::remove(&self.settings.record);
        tracing::info!("model server stopped");
        self.state = ServerState::Stopped;
        self.idle_since = None;
    }

    fn kill_owned(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill();
            let _ = child.try_exit();
        }
        ServerRecord::remove(&self.settings.record);
    }

    /// The application is closing: stop the process we started, if any.
    pub(crate) fn shutdown(&mut self) {
        self.stop_owned();
    }
}
