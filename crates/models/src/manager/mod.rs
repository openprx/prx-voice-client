//! The manager facade: starts the coordinator thread and hands out the
//! `ModelHandle` (design 4.7). Module map: `api` (value types), `handle`
//! (shared state and `ModelHandle`), `event_channel` (bounded subscription), `coordinator` (queue and scheduling),
//! `snapshot` (derived UI state), `jobs_file` (persistence).

mod api;
mod coordinator;
mod event_channel;
mod handle;
mod jobs_file;
mod snapshot;

use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use crate::download::DownloadEngine;
use crate::hardware::HardwareInfo;
use crate::store::ModelStore;
use crate::types::ManagerError;

pub use api::{
    JobView, MAX_CONCURRENT_LIMIT, ManagerConfig, ManagerOptions, ManagerSnapshot, ModelCommand,
    ModelEvent, ModelView,
};
pub use event_channel::{EVENT_CAPACITY, EventReceiver};
pub use handle::ModelHandle;

use coordinator::{Coordinator, CoordinatorParams};
use handle::{Msg, Shared};

/// Owns the coordinator thread. Dropping it pauses running jobs and joins.
pub struct ModelManager {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    grace: Duration,
}

impl ModelManager {
    pub fn start(cfg: ManagerConfig) -> Result<(ModelManager, ModelHandle), ManagerError> {
        let store = ModelStore::open_with(
            cfg.models_dir.clone(),
            cfg.options
                .engine
                .free_space
                .clone()
                .unwrap_or_else(crate::store::default_free_space),
        )?;
        let engine =
            DownloadEngine::new(cfg.options.engine.clone()).map_err(ManagerError::Client)?;
        let (tx, rx) = channel();
        let hardware = cfg.options.hardware.unwrap_or_else(HardwareInfo::detect);
        let shared = Arc::new(Shared {
            tx: Mutex::new(tx.clone()),
            snapshot: RwLock::new(Arc::new(ManagerSnapshot::default())),
            subscribers: Default::default(),
            waker: RwLock::new(None),
            store: Arc::new(store),
            catalogue: RwLock::new(Arc::new(cfg.catalogue)),
            load_states: Mutex::new(Default::default()),
            active: RwLock::new(cfg.active),
            hardware,
            runtimes: cfg.runtimes,
        });
        let grace = cfg.options.shutdown_grace;
        let coordinator = Coordinator::new(CoordinatorParams {
            sh: shared.clone(),
            rx,
            tx: tx.clone(),
            engine: Arc::new(engine),
            opts: cfg.options,
            endpoint: cfg.endpoint,
            max_concurrent: cfg.max_concurrent,
            legacy_dir: cfg.legacy_models_dir,
        });
        let thread = std::thread::Builder::new()
            .name("prxvc-models".into())
            .spawn(move || coordinator.run())
            .map_err(|e| ManagerError::Thread(e.to_string()))?;
        Ok((
            ModelManager {
                tx,
                thread: Some(thread),
                grace,
            },
            ModelHandle { inner: shared },
        ))
    }

    /// Pause running jobs, persist `jobs.json`, join the threads (bounded).
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some(t) = self.thread.take() else { return };
        let (ack_tx, ack_rx) = channel();
        if self.tx.send(Msg::Shutdown(ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(self.grace + Duration::from_secs(1));
        }
        let _ = t.join();
    }
}

impl Drop for ModelManager {
    fn drop(&mut self) {
        self.stop();
    }
}
