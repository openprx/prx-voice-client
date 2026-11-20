//! `ModelHandle`: the cheap, clonable face of the manager used by the UI and
//! other crates. Single responsibility: shared state, commands in, snapshot,
//! events and `resolve` out.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use parking_lot::{Mutex, RwLock};

use crate::catalogue::{Catalogue, LangKey, ModelId, Role};
use crate::hardware::HardwareInfo;
use crate::store::{ModelStore, ReadyModel};
use crate::types::{ActiveSet, InstallState, LoadState, NotReady};

use super::api::{ManagerSnapshot, ModelCommand, ModelEvent, SharedSnapshot, Waker};
use super::event_channel::{EventReceiver, Subscribers};

/// Messages for the coordinator thread.
pub(crate) enum Msg {
    Cmd(ModelCommand),
    JobDone {
        id: ModelId,
        token: u64,
        outcome: crate::download::JobOutcome,
    },
    Probe {
        id: ModelId,
        token: u64,
        reachable: bool,
    },
    MigrationProgress(crate::store::MigrationProgress),
    MigrationDone(crate::store::MigrationReport),
    LoadStateChanged,
    Shutdown(Sender<()>),
}

pub(crate) struct Shared {
    pub tx: Mutex<Sender<Msg>>,
    pub snapshot: RwLock<SharedSnapshot>,
    pub subscribers: Subscribers<ModelEvent>,
    pub waker: RwLock<Option<Waker>>,
    pub store: Arc<ModelStore>,
    pub catalogue: RwLock<Arc<Catalogue>>,
    pub load_states: Mutex<HashMap<ModelId, LoadState>>,
    pub active: RwLock<ActiveSet>,
    pub hardware: HardwareInfo,
    pub runtimes: Vec<String>,
}

impl Shared {
    pub fn emit(&self, ev: ModelEvent) {
        self.subscribers.emit(ev);
    }

    pub fn wake(&self) {
        if let Some(w) = self.waker.read().as_ref() {
            w();
        }
    }

    pub fn runtime_refs(&self) -> Vec<&str> {
        self.runtimes.iter().map(String::as_str).collect()
    }
}

#[derive(Clone)]
pub struct ModelHandle {
    pub(crate) inner: Arc<Shared>,
}

impl ModelHandle {
    /// Queue a command; never blocks.
    pub fn send(&self, cmd: ModelCommand) {
        let _ = self.inner.tx.lock().send(Msg::Cmd(cmd));
    }

    /// The latest snapshot (cheap clone of an `Arc`).
    pub fn snapshot(&self) -> Arc<ManagerSnapshot> {
        self.inner.snapshot.read().clone()
    }

    /// Subscribe to manager events. The queue is bounded (`EVENT_CAPACITY`)
    /// and drops the oldest event when the subscriber lags; the snapshot is
    /// the source of truth, so after `dropped_count` grows, re-read it.
    pub fn subscribe(&self) -> EventReceiver<ModelEvent> {
        self.inner.subscribers.subscribe()
    }

    /// Called (from any thread) whenever the snapshot or an event changed;
    /// the desktop app passes `ctx.request_repaint`.
    pub fn set_waker(&self, waker: Box<dyn Fn() + Send + Sync>) {
        *self.inner.waker.write() = Some(waker);
    }

    /// Called by runtime owners.
    pub fn report_load_state(&self, id: &ModelId, state: LoadState) {
        self.inner.load_states.lock().insert(id.clone(), state);
        let _ = self.inner.tx.lock().send(Msg::LoadStateChanged);
    }

    /// Replace the active selection (after a settings change).
    pub fn set_active(&self, active: ActiveSet) {
        *self.inner.active.write() = active;
    }

    /// The model to use for `role` and `key`: the active one, else the
    /// recommendation. Paths are returned only for a fully verified install.
    pub fn resolve(&self, role: Role, key: &LangKey) -> Result<ReadyModel, NotReady> {
        let sh = &self.inner;
        let catalogue = sh.catalogue.read().clone();
        let runtimes = sh.runtime_refs();
        let id = match sh.active.read().get(role, key).cloned() {
            Some(id) => id,
            None => catalogue
                .recommended(role, key, &sh.hardware, &runtimes)
                .map(|e| e.id.clone())
                .ok_or(NotReady::NoSelection)?,
        };
        let entry = catalogue.get(&id).ok_or(NotReady::NoSelection)?;
        if !runtimes.contains(&entry.runtime.name.as_str()) {
            return Err(NotReady::UnsupportedRuntime {
                id,
                runtime: entry.runtime.name.clone(),
            });
        }
        if let Some(ready) = sh.store.ready(entry) {
            return Ok(ready);
        }
        let snap = self.snapshot();
        match snap.model(&id).map(|m| m.install.clone()) {
            Some(
                st @ (InstallState::Queued
                | InstallState::Downloading { .. }
                | InstallState::Paused { .. }
                | InstallState::Verifying { .. }),
            ) => Err(NotReady::InProgress { id, state: st }),
            Some(InstallState::Failed { reason, .. }) => Err(NotReady::Failed { id, reason }),
            _ => Err(NotReady::NotInstalled {
                id,
                size: entry.total_size(),
            }),
        }
    }
}
