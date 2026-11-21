//! The public face of the manager: a cheap handle, the worker thread that steps the core, leases
//! for consumers and blocking waits for readiness.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use super::core::{Core, Demand, ServerState, Settings, Snapshot};
use super::problem::ServerProblem;

struct Shared {
    snapshot: Mutex<Snapshot>,
    changed: Condvar,
}

struct Inner {
    demand: Arc<Mutex<Demand>>,
    wake: Condvar,
    shared: Shared,
    log: std::path::PathBuf,
    worker: Mutex<Option<JoinHandle<()>>>,
}

/// The managed local model server.
#[derive(Clone)]
pub(crate) struct LlmServer {
    inner: Arc<Inner>,
}

/// A consumer's hold on the server. While any lease or the pin exists the server runs; dropping
/// the last one starts the idle countdown.
pub(crate) struct ServerLease {
    server: LlmServer,
    attempt: u64,
}

impl LlmServer {
    /// Start the worker over `core`. The core's demand is the one returned handles change.
    pub(crate) fn spawn(core: Core, demand: Arc<Mutex<Demand>>, settings: &Settings) -> Self {
        let first = core.snapshot();
        let inner = Arc::new(Inner {
            demand,
            wake: Condvar::new(),
            shared: Shared {
                snapshot: Mutex::new(first),
                changed: Condvar::new(),
            },
            log: settings.log.clone(),
            worker: Mutex::new(None),
        });
        let worker_inner = Arc::clone(&inner);
        let handle = std::thread::Builder::new()
            .name("llm-server".into())
            .spawn(move || run(core, &worker_inner))
            .map_err(|e| tracing::error!("model server thread: {e}"))
            .ok();
        *inner.worker.lock() = handle;
        Self { inner }
    }

    /// The language-model engine is (or is no longer) selected. Returns the request number to
    /// wait on with [`LlmServer::wait_ready`].
    pub(crate) fn set_pinned(&self, on: bool) -> u64 {
        let mut d = self.inner.demand.lock();
        if d.pinned != on {
            d.pinned = on;
            if on {
                d.attempt += 1;
            }
            self.inner.wake.notify_all();
        }
        d.attempt
    }

    /// Hold the server for a consumer: starts it when it is not running.
    pub(crate) fn acquire(&self) -> ServerLease {
        let mut d = self.inner.demand.lock();
        d.leases += 1;
        d.attempt += 1;
        let attempt = d.attempt;
        self.inner.wake.notify_all();
        drop(d);
        ServerLease {
            server: self.clone(),
            attempt,
        }
    }

    /// Try again after a failure.
    pub(crate) fn retry(&self) -> u64 {
        let mut d = self.inner.demand.lock();
        d.attempt += 1;
        self.inner.wake.notify_all();
        d.attempt
    }

    /// The state now.
    pub(crate) fn snapshot(&self) -> Snapshot {
        self.inner.shared.snapshot.lock().clone()
    }

    /// Block until the server is ready for request `attempt`, or says why it cannot be.
    ///
    /// # Errors
    /// The typed problem, or `HealthTimeout` when `timeout` passes first.
    pub(crate) fn wait_ready(&self, attempt: u64, timeout: Duration) -> Result<(), ServerProblem> {
        let deadline = Instant::now() + timeout;
        let mut snap = self.inner.shared.snapshot.lock();
        loop {
            if snap.attempt_seen >= attempt {
                match &snap.state {
                    ServerState::Ready { .. } => return Ok(()),
                    ServerState::Failed(problem) => return Err(problem.clone()),
                    _ => {}
                }
            }
            if self
                .inner
                .shared
                .changed
                .wait_until(&mut snap, deadline)
                .timed_out()
            {
                return Err(ServerProblem::HealthTimeout {
                    seconds: timeout.as_secs(),
                    log: self.inner.log.clone(),
                });
            }
        }
    }

    /// Stop the server we started and end the worker. Blocks for the stop (at most its grace
    /// period). Safe to call more than once.
    pub(crate) fn shutdown(&self) {
        self.inner.demand.lock().shutdown = true;
        self.inner.wake.notify_all();
        if let Some(worker) = self.inner.worker.lock().take() {
            let _ = worker.join();
        }
    }
}

impl ServerLease {
    /// Block until the server is ready (see [`LlmServer::wait_ready`]).
    ///
    /// # Errors
    /// The typed problem, or a timeout.
    pub(crate) fn wait_ready(&self, timeout: Duration) -> Result<(), ServerProblem> {
        self.server.wait_ready(self.attempt, timeout)
    }
}

impl Drop for ServerLease {
    fn drop(&mut self) {
        let mut d = self.server.inner.demand.lock();
        d.leases = d.leases.saturating_sub(1);
        self.server.inner.wake.notify_all();
    }
}

fn publish(inner: &Inner, snap: Snapshot) {
    let mut current = inner.shared.snapshot.lock();
    if *current != snap {
        *current = snap;
        inner.shared.changed.notify_all();
    }
}

fn run(mut core: Core, inner: &Inner) {
    loop {
        let before = *inner.demand.lock();
        if before.shutdown {
            core.shutdown();
            publish(inner, core.snapshot());
            return;
        }
        let wait = core.step();
        publish(inner, core.snapshot());
        let mut d = inner.demand.lock();
        // A change that came in during the step is handled at once.
        if *d == before {
            inner.wake.wait_for(&mut d, wait);
        }
    }
}
