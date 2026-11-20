//! The coordinator thread: owns the job queue, schedules downloads under the
//! concurrency limit, handles commands, offline retries, preparation and
//! migration (design 4.2 rules 1, 2, 8, 9; 4.5; 4.7). Snapshot derivation is
//! in `snapshot.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::catalogue::{Catalogue, LangKey, ModelEntry, ModelId, Role};
use crate::control::{JobControl, JobProgress, Stop};
use crate::download::{DownloadEngine, JobOutcome, JobSpec};
use crate::net::Endpoint;
use crate::store::{MigrationProgress, MigrationReport, StorageUsage, StoreInstallState};
use crate::types::FailReason;

use super::api::{MAX_CONCURRENT_LIMIT, ManagerOptions, ModelCommand, ModelEvent};
use super::handle::{Msg, Shared};
use super::jobs_file::{self, JobRecord};

#[derive(Clone, Debug)]
pub(super) enum JobStatus {
    Queued,
    Running,
    Paused,
    Failed { reason: FailReason, retryable: bool },
}

pub(super) struct Job {
    pub entry: ModelEntry,
    pub status: JobStatus,
    pub control: Arc<JobControl>,
    pub progress: Arc<JobProgress>,
    pub token: u64,
    pub probe_at: Option<Instant>,
    pub probing: bool,
    pub speed_at: Instant,
    pub speed_done: u64,
    pub bps: u64,
    pub thread: Option<JoinHandle<()>>,
}

pub(super) struct StoreCache {
    pub states: HashMap<ModelId, StoreInstallState>,
    pub revisions: HashMap<ModelId, Vec<String>>,
    pub usage: StorageUsage,
    pub at: Instant,
}

/// Everything the coordinator is started with.
pub(super) struct CoordinatorParams {
    pub sh: Arc<Shared>,
    pub rx: Receiver<Msg>,
    pub tx: Sender<Msg>,
    pub engine: Arc<DownloadEngine>,
    pub opts: ManagerOptions,
    pub endpoint: Endpoint,
    pub max_concurrent: usize,
    pub legacy_dir: Option<PathBuf>,
}

pub(super) struct Coordinator {
    pub sh: Arc<Shared>,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    engine: Arc<DownloadEngine>,
    opts: ManagerOptions,
    endpoint: Endpoint,
    max_concurrent: usize,
    pub jobs: HashMap<ModelId, Job>,
    pub queue: Vec<ModelId>,
    preps: HashMap<(Role, LangKey), ModelId>,
    migrating: bool,
    shutting_down: bool,
    pub migration: Option<MigrationProgress>,
    next_token: u64,
    pub cache: Option<StoreCache>,
    pub cache_dirty: bool,
    pub last_build: Instant,
    pub dirty: bool,
}

impl Coordinator {
    pub fn new(p: CoordinatorParams) -> Coordinator {
        let CoordinatorParams {
            sh,
            rx,
            tx,
            engine,
            opts,
            endpoint,
            max_concurrent,
            legacy_dir,
        } = p;
        let mut c = Coordinator {
            sh,
            rx,
            tx,
            engine,
            opts,
            endpoint,
            max_concurrent: max_concurrent.clamp(1, MAX_CONCURRENT_LIMIT),
            jobs: HashMap::new(),
            queue: Vec::new(),
            preps: HashMap::new(),
            migrating: false,
            shutting_down: false,
            migration: None,
            next_token: 1,
            cache: None,
            cache_dirty: true,
            last_build: Instant::now(),
            dirty: true,
        };
        c.restore_jobs();
        match legacy_dir.filter(|d| d.is_dir()) {
            Some(dir) => c.start_migration(dir),
            None => c.startup_gc(),
        }
        c.build_snapshot();
        c
    }

    fn jobs_path(&self) -> PathBuf {
        self.sh.store.root().join("jobs.json")
    }

    fn restore_jobs(&mut self) {
        let catalogue = self.sh.catalogue.read().clone();
        for rec in jobs_file::load(&self.jobs_path()) {
            let Some(entry) = catalogue.get(&rec.model_id) else {
                continue;
            };
            if self.sh.store.ready(entry).is_some() || self.jobs.contains_key(&rec.model_id) {
                continue;
            }
            self.add_job(
                entry.clone(),
                if rec.paused {
                    JobStatus::Paused
                } else {
                    JobStatus::Queued
                },
            );
        }
    }

    fn persist_jobs(&self) {
        if self.sh.store.is_read_only() {
            return;
        }
        let recs: Vec<JobRecord> = self
            .queue
            .iter()
            .filter_map(|id| {
                let j = self.jobs.get(id)?;
                match j.status {
                    JobStatus::Failed { .. } => None,
                    JobStatus::Paused => Some(JobRecord {
                        model_id: id.clone(),
                        paused: true,
                    }),
                    _ => Some(JobRecord {
                        model_id: id.clone(),
                        paused: false,
                    }),
                }
            })
            .collect();
        jobs_file::save(&self.jobs_path(), &recs);
    }

    // ---- migration and start-up gc -----------------------------------------

    fn start_migration(&mut self, dir: PathBuf) {
        self.migrating = true;
        self.migration = Some(MigrationProgress::default());
        let (sh, tx) = (self.sh.clone(), self.tx.clone());
        let catalogue = self.sh.catalogue.read().clone();
        let spawned = std::thread::Builder::new()
            .name("prxvc-migrate".into())
            .spawn(move || {
                let tx2 = tx.clone();
                let report = sh.store.migrate_legacy(&dir, &catalogue, &move |p| {
                    let _ = tx2.send(Msg::MigrationProgress(p));
                });
                let _ = tx.send(Msg::MigrationDone(report));
            });
        if spawned.is_err() {
            self.migrating = false;
            self.migration = None;
        }
    }

    fn startup_gc(&mut self) {
        if self.sh.store.is_read_only() {
            return;
        }
        let catalogue = self.sh.catalogue.read().clone();
        let active = self.sh.active.read().clone();
        let plan = self.sh.store.gc_plan(&catalogue, &active, &[]).automatic();
        if let Err(e) = self.sh.store.gc_apply(&plan) {
            tracing::warn!("start-up garbage collection failed: {e}");
        }
        self.cache_dirty = true;
    }

    // ---- main loop ---------------------------------------------------------------

    pub fn run(mut self) {
        loop {
            let any_running = self
                .jobs
                .values()
                .any(|j| matches!(j.status, JobStatus::Running));
            let due = if any_running || self.dirty {
                self.opts
                    .snapshot_interval
                    .saturating_sub(self.last_build.elapsed())
            } else {
                Duration::from_millis(250)
            };
            match self.rx.recv_timeout(due.max(Duration::from_millis(5))) {
                Ok(Msg::Shutdown(ack)) => {
                    self.shutdown();
                    let _ = ack.send(());
                    return;
                }
                Ok(msg) => {
                    self.handle(msg);
                    // Drain whatever else is queued before scheduling.
                    while let Ok(m) = self.rx.try_recv() {
                        if let Msg::Shutdown(ack) = m {
                            self.shutdown();
                            let _ = ack.send(());
                            return;
                        }
                        self.handle(m);
                    }
                    self.dirty = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.shutdown();
                    return;
                }
            }
            self.schedule();
            self.offline_probes();
            let any_running = self
                .jobs
                .values()
                .any(|j| matches!(j.status, JobStatus::Running));
            if self.dirty
                && (self.last_build.elapsed() >= self.opts.snapshot_interval || !any_running)
                || any_running && self.last_build.elapsed() >= self.opts.snapshot_interval
            {
                self.build_snapshot();
            }
        }
    }

    fn shutdown(&mut self) {
        self.shutting_down = true;
        for j in self.jobs.values() {
            if matches!(j.status, JobStatus::Running) {
                j.control.request(Stop::Pause);
            }
        }
        let deadline = Instant::now() + self.opts.shutdown_grace;
        while self
            .jobs
            .values()
            .any(|j| matches!(j.status, JobStatus::Running))
        {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            if let Ok(Msg::JobDone { id, token, outcome }) = self.rx.recv_timeout(left) {
                self.job_done(id, token, outcome);
            }
        }
        // Whatever was running resumes next start.
        self.persist_jobs();
        for j in self.jobs.values_mut() {
            if let Some(t) = j.thread.take()
                && t.is_finished()
            {
                let _ = t.join();
            }
        }
        self.sh.store.close();
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Cmd(c) => self.command(c),
            Msg::JobDone { id, token, outcome } => self.job_done(id, token, outcome),
            Msg::Probe {
                id,
                token,
                reachable,
            } => {
                let interval = self.opts.offline_retry_interval;
                if let Some(j) = self.jobs.get_mut(&id)
                    && j.token == token
                {
                    j.probing = false;
                    if reachable
                        && matches!(
                            j.status,
                            JobStatus::Failed {
                                reason: FailReason::Offline,
                                ..
                            }
                        )
                    {
                        j.status = JobStatus::Queued;
                        j.probe_at = None;
                    } else {
                        j.probe_at = Some(Instant::now() + interval);
                    }
                    self.state_changed(&id);
                }
            }
            Msg::MigrationProgress(p) => self.migration = Some(p),
            Msg::MigrationDone(report) => self.migration_done(report),
            Msg::LoadStateChanged => {}
            Msg::Shutdown(_) => {}
        }
    }

    fn migration_done(&mut self, report: MigrationReport) {
        self.migrating = false;
        self.migration = None;
        self.cache_dirty = true;
        // Jobs for models that migration just completed are no longer needed.
        let catalogue = self.sh.catalogue.read().clone();
        let done: Vec<ModelId> = self
            .jobs
            .iter()
            .filter(|(id, j)| {
                !matches!(j.status, JobStatus::Running)
                    && catalogue
                        .get(id)
                        .is_some_and(|e| self.sh.store.ready(e).is_some())
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in done {
            self.remove_job(&id);
        }
        self.startup_gc();
        self.sh.emit(ModelEvent::MigrationFinished(report));
        self.persist_jobs();
    }

    fn state_changed(&mut self, id: &ModelId) {
        self.dirty = true;
        self.sh.emit(ModelEvent::StateChanged(id.clone()));
    }

    // ---- jobs ------------------------------------------------------------------------

    fn add_job(&mut self, entry: ModelEntry, status: JobStatus) {
        let id = entry.id.clone();
        self.sh.store.claim(&id);
        self.next_token += 1;
        self.jobs.insert(
            id.clone(),
            Job {
                entry,
                status,
                control: Arc::new(JobControl::new()),
                progress: Arc::new(JobProgress::new()),
                token: self.next_token,
                probe_at: None,
                probing: false,
                speed_at: Instant::now(),
                speed_done: 0,
                bps: 0,
                thread: None,
            },
        );
        self.queue.push(id);
    }

    fn remove_job(&mut self, id: &ModelId) {
        if let Some(mut j) = self.jobs.remove(id) {
            if let Some(t) = j.thread.take()
                && t.is_finished()
            {
                let _ = t.join();
            }
        }
        self.queue.retain(|q| q != id);
        self.sh.store.release(id);
        self.cache_dirty = true;
    }

    /// Download / Retry / Resume semantics: make sure a queued job exists.
    fn enqueue(&mut self, id: &ModelId, front: bool) {
        let catalogue = self.sh.catalogue.read().clone();
        let Some(entry) = catalogue.get(id) else {
            tracing::warn!(%id, "download requested for an unknown model");
            return;
        };
        if !self.sh.runtimes.contains(&entry.runtime.name) {
            tracing::warn!(%id, runtime = %entry.runtime.name, "runtime not available; not downloading");
            return;
        }
        if self.sh.store.ready(entry).is_some() {
            return;
        }
        match self.jobs.get_mut(id) {
            Some(j) => {
                if matches!(j.status, JobStatus::Paused | JobStatus::Failed { .. }) {
                    j.status = JobStatus::Queued;
                    j.probe_at = None;
                }
            }
            None => self.add_job(entry.clone(), JobStatus::Queued),
        }
        if front {
            self.queue.retain(|q| q != id);
            self.queue.insert(0, id.clone());
        }
        self.state_changed(id);
    }

    fn command(&mut self, cmd: ModelCommand) {
        match cmd {
            ModelCommand::Download(id) | ModelCommand::Retry(id) | ModelCommand::Resume(id) => {
                self.enqueue(&id, false)
            }
            ModelCommand::DownloadNext(id) => self.enqueue(&id, true),
            ModelCommand::Pause(id) => self.pause(&id),
            ModelCommand::PauseAll => {
                let ids: Vec<ModelId> = self.queue.clone();
                for id in ids {
                    self.pause(&id);
                }
            }
            ModelCommand::Cancel(id) => self.cancel(&id),
            ModelCommand::Use { role, key, id } => self.use_model(role, key, id),
            ModelCommand::Remove(id) => self.remove_model(&id),
            ModelCommand::RemoveUnused => self.remove_unused(),
            ModelCommand::SetEndpoint(e) => self.endpoint = e,
            ModelCommand::SetMaxConcurrent(n) => {
                self.max_concurrent = n.clamp(1, MAX_CONCURRENT_LIMIT)
            }
            ModelCommand::ReplaceCatalogue(c) => self.replace_catalogue(c),
        }
        self.persist_jobs();
    }

    fn pause(&mut self, id: &ModelId) {
        let Some(j) = self.jobs.get_mut(id) else {
            return;
        };
        match j.status {
            JobStatus::Queued => j.status = JobStatus::Paused,
            JobStatus::Running => j.control.request(Stop::Pause),
            _ => return,
        }
        self.state_changed(id);
    }

    fn cancel(&mut self, id: &ModelId) {
        let running = matches!(
            self.jobs.get(id).map(|j| &j.status),
            Some(JobStatus::Running)
        );
        if running {
            self.jobs[id].control.request(Stop::Cancel);
        } else {
            let catalogue = self.sh.catalogue.read().clone();
            if let Some(entry) = self
                .jobs
                .get(id)
                .map(|j| j.entry.clone())
                .or_else(|| catalogue.get(id).cloned())
            {
                self.sh.store.delete_partials(&entry);
            }
            self.remove_job(id);
            self.preps.retain(|_, v| v != id);
            self.state_changed(id);
        }
    }

    fn use_model(&mut self, role: Role, key: LangKey, id: ModelId) {
        // A newer choice for the same key supersedes a pending one.
        self.preps.remove(&(role, key.clone()));
        let catalogue = self.sh.catalogue.read().clone();
        let Some(entry) = catalogue.get(&id) else {
            let reason = FailReason::LoadFailed {
                message: format!("unknown model {id}"),
            };
            self.sh.emit(ModelEvent::PreparationFailed {
                role,
                key,
                id,
                reason,
            });
            return;
        };
        if !self.sh.runtimes.contains(&entry.runtime.name) {
            let reason = FailReason::UnsupportedRuntime {
                name: entry.runtime.name.clone(),
            };
            self.sh.emit(ModelEvent::PreparationFailed {
                role,
                key,
                id,
                reason,
            });
            return;
        }
        if self.sh.store.ready(entry).is_some() {
            self.sh.emit(ModelEvent::PreparationDone { role, key, id });
            return;
        }
        self.preps.insert((role, key), id.clone());
        self.enqueue(&id, false);
    }

    fn remove_model(&mut self, id: &ModelId) {
        let loaded = self
            .sh
            .load_states
            .lock()
            .get(id)
            .is_some_and(|s| s.is_loaded());
        if self.jobs.contains_key(id) || loaded {
            tracing::warn!(%id, "refusing to remove a model that is in use");
            return;
        }
        match self.sh.store.remove(id) {
            Ok(_) => {
                self.cache_dirty = true;
                self.state_changed(id);
            }
            Err(e) => tracing::warn!(%id, "remove failed: {e}"),
        }
    }

    fn remove_unused(&mut self) {
        let catalogue = self.sh.catalogue.read().clone();
        let active = self.sh.active.read().clone();
        let loaded: Vec<ModelId> = self
            .sh
            .load_states
            .lock()
            .iter()
            .filter(|(_, s)| s.is_loaded())
            .map(|(i, _)| i.clone())
            .collect();
        let plan = self
            .sh
            .store
            .gc_plan(&catalogue, &active, &loaded)
            .unused_models();
        let ids: Vec<ModelId> = plan
            .items
            .iter()
            .filter_map(|i| i.model_id.clone())
            .collect();
        match self.sh.store.gc_apply(&plan) {
            Ok(_) => {
                self.cache_dirty = true;
                for id in ids {
                    self.state_changed(&id);
                }
            }
            Err(e) => tracing::warn!("removing unused models failed: {e}"),
        }
    }

    fn replace_catalogue(&mut self, c: Catalogue) {
        let c = Arc::new(c);
        *self.sh.catalogue.write() = c.clone();
        self.cache_dirty = true;
        let ids: Vec<ModelId> = self.jobs.keys().cloned().collect();
        for id in ids {
            let running = matches!(self.jobs[&id].status, JobStatus::Running);
            match c.get(&id) {
                Some(e) if !running => {
                    self.jobs.get_mut(&id).expect("job exists").entry = e.clone()
                }
                None if !running => {
                    self.remove_job(&id);
                    self.preps.retain(|_, v| *v != id);
                }
                _ => {}
            }
        }
        self.dirty = true;
    }

    // ---- scheduling ---------------------------------------------------------------------

    fn schedule(&mut self) {
        if self.migrating {
            return;
        }
        let mut running = self
            .jobs
            .values()
            .filter(|j| matches!(j.status, JobStatus::Running))
            .count();
        let next: Vec<ModelId> = self
            .queue
            .iter()
            .filter(|id| matches!(self.jobs[*id].status, JobStatus::Queued))
            .cloned()
            .collect();
        for id in next {
            if running >= self.max_concurrent {
                break;
            }
            if self.start_job(&id) {
                running += 1;
            }
        }
    }

    fn start_job(&mut self, id: &ModelId) -> bool {
        let catalogue_version = self.sh.catalogue.read().catalogue_version;
        let endpoint = self.endpoint.clone();
        let (engine, sh, tx) = (self.engine.clone(), self.sh.clone(), self.tx.clone());
        self.next_token += 1;
        let token = self.next_token;
        let job = self.jobs.get_mut(id).expect("job exists");
        job.token = token;
        job.control = Arc::new(JobControl::new());
        job.progress = Arc::new(JobProgress::new());
        job.speed_at = Instant::now();
        job.speed_done = 0;
        job.bps = 0;
        let (entry, ctl, progress) = (job.entry.clone(), job.control.clone(), job.progress.clone());
        let jid = id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("prxvc-dl-{id}"))
            .spawn(move || {
                let outcome = engine.run_job(
                    &sh.store,
                    JobSpec {
                        entry: &entry,
                        catalogue_version,
                        endpoint: &endpoint,
                    },
                    &ctl,
                    &progress,
                );
                let _ = tx.send(Msg::JobDone {
                    id: jid,
                    token,
                    outcome,
                });
            });
        match spawned {
            Ok(h) => {
                job.thread = Some(h);
                job.status = JobStatus::Running;
                self.state_changed(id);
                true
            }
            Err(e) => {
                job.status = JobStatus::Failed {
                    reason: FailReason::Io {
                        message: format!("could not start a download thread: {e}"),
                    },
                    retryable: true,
                };
                self.state_changed(id);
                false
            }
        }
    }

    fn job_done(&mut self, id: ModelId, token: u64, outcome: JobOutcome) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        if job.token != token {
            return;
        }
        if let Some(t) = job.thread.take() {
            let _ = t.join();
        }
        match outcome {
            JobOutcome::Installed => {
                self.remove_job(&id);
                self.cache_dirty = true;
                let done: Vec<(Role, LangKey)> = self
                    .preps
                    .iter()
                    .filter(|(_, v)| **v == id)
                    .map(|(k, _)| k.clone())
                    .collect();
                for (role, key) in done {
                    self.preps.remove(&(role, key.clone()));
                    self.sh.emit(ModelEvent::PreparationDone {
                        role,
                        key,
                        id: id.clone(),
                    });
                }
            }
            // Paused by quitting: it resumes at the next start, so it is
            // recorded as waiting, not as paused by the user.
            JobOutcome::Paused if self.shutting_down => job.status = JobStatus::Queued,
            JobOutcome::Paused => job.status = JobStatus::Paused,
            JobOutcome::Cancelled => {
                self.remove_job(&id);
                self.preps.retain(|_, v| *v != id);
            }
            JobOutcome::Failed(reason) => {
                let offline = reason == FailReason::Offline;
                job.status = JobStatus::Failed {
                    reason: reason.clone(),
                    retryable: true,
                };
                job.probe_at = offline.then(|| Instant::now() + self.opts.offline_retry_interval);
                job.probing = false;
                if !offline {
                    let failed: Vec<(Role, LangKey)> = self
                        .preps
                        .iter()
                        .filter(|(_, v)| **v == id)
                        .map(|(k, _)| k.clone())
                        .collect();
                    for (role, key) in failed {
                        self.preps.remove(&(role, key.clone()));
                        self.sh.emit(ModelEvent::PreparationFailed {
                            role,
                            key,
                            id: id.clone(),
                            reason: reason.clone(),
                        });
                    }
                }
            }
        }
        self.state_changed(&id);
        self.persist_jobs();
    }

    /// Jobs that failed as offline are probed every `offline_retry_interval`;
    /// when the endpoint answers they are queued again.
    fn offline_probes(&mut self) {
        let now = Instant::now();
        let due: Vec<ModelId> = self
            .jobs
            .iter()
            .filter(|(_, j)| {
                matches!(
                    j.status,
                    JobStatus::Failed {
                        reason: FailReason::Offline,
                        ..
                    }
                ) && !j.probing
                    && j.probe_at.is_some_and(|t| t <= now)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            let endpoint = self.endpoint.clone();
            let (engine, tx) = (self.engine.clone(), self.tx.clone());
            let job = self.jobs.get_mut(&id).expect("job exists");
            let token = job.token;
            let url = job
                .entry
                .files
                .first()
                .map(|f| endpoint.url_for(&f.source))
                .and_then(|u| url::Url::parse(&u).ok())
                .and_then(|u| u.join("/").ok())
                .map(|u| u.to_string());
            let Some(root) = url else { continue };
            job.probing = true;
            let jid = id.clone();
            let ok = std::thread::Builder::new()
                .name("prxvc-probe".into())
                .spawn(move || {
                    let reachable = engine.probe(&root);
                    let _ = tx.send(Msg::Probe {
                        id: jid,
                        token,
                        reachable,
                    });
                });
            if ok.is_err() {
                job.probing = false;
            }
        }
    }
}
