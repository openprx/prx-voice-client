//! Builds the `ManagerSnapshot` from coordinator state (design 4.4, 4.7).
//! Single responsibility: derive what the UI shows.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::catalogue::{ModelEntry, ModelId};
use crate::control::Phase;
use crate::store::StoreInstallState;
use crate::types::{FailReason, InstallState, LoadState, UpdateAvailable};

use super::api::{JobView, ManagerSnapshot, ModelView};
use super::coordinator::{Coordinator, JobStatus, StoreCache};

const CACHE_MAX_AGE: Duration = Duration::from_secs(2);

impl Coordinator {
    pub(super) fn refresh_cache_if_needed(&mut self) {
        let stale = self
            .cache
            .as_ref()
            .is_none_or(|c| c.at.elapsed() > CACHE_MAX_AGE);
        if !(self.cache_dirty || stale) {
            return;
        }
        let catalogue = self.sh.catalogue.read().clone();
        let mut states = HashMap::new();
        for m in catalogue.models() {
            states.insert(m.id.clone(), self.sh.store.install_state(m));
        }
        let mut revisions: HashMap<ModelId, Vec<String>> = HashMap::new();
        for i in self.sh.store.installed_models() {
            revisions.entry(i.id).or_default().push(i.revision_key);
        }
        self.cache = Some(StoreCache {
            states,
            revisions,
            usage: self.sh.store.usage(),
            at: Instant::now(),
        });
        self.cache_dirty = false;
    }

    pub(super) fn build_snapshot(&mut self) {
        self.refresh_cache_if_needed();
        let catalogue = self.sh.catalogue.read().clone();
        let cache = self.cache.as_ref().expect("cache refreshed");
        let loads = self.sh.load_states.lock().clone();
        let runtimes = self.sh.runtime_refs();
        let now = Instant::now();

        // Job views (updates speed samples).
        let mut job_installs: HashMap<ModelId, InstallState> = HashMap::new();
        for (id, job) in self.jobs.iter_mut() {
            let total = if job.progress.total() > 0 {
                job.progress.total()
            } else {
                job.entry.total_size()
            };
            let done = job.progress.done();
            let st = match &job.status {
                JobStatus::Queued => InstallState::Queued,
                JobStatus::Paused => InstallState::Paused { done, total },
                JobStatus::Failed { reason, retryable } => InstallState::Failed {
                    reason: reason.clone(),
                    retryable: *retryable,
                },
                JobStatus::Running => match job.progress.phase() {
                    Phase::Verifying => {
                        let (d, t) = job.progress.verify();
                        InstallState::Verifying { done: d, total: t }
                    }
                    Phase::Downloading => {
                        let dt = now.duration_since(job.speed_at);
                        if dt >= Duration::from_millis(250) {
                            job.bps = (done.saturating_sub(job.speed_done) as f64
                                / dt.as_secs_f64()) as u64;
                            job.speed_at = now;
                            job.speed_done = done;
                        }
                        InstallState::Downloading {
                            done,
                            total,
                            bytes_per_s: job.bps,
                        }
                    }
                },
            };
            job_installs.insert(id.clone(), st);
        }

        let superseders: HashMap<&ModelId, &ModelEntry> = catalogue
            .models()
            .iter()
            .filter(|m| !m.is_hidden())
            .flat_map(|m| m.supersedes.iter().flatten().map(move |old| (old, m)))
            .collect();

        let mut models = Vec::new();
        let mut seen: HashSet<ModelId> = HashSet::new();
        for m in catalogue.models() {
            seen.insert(m.id.clone());
            let revs = cache.revisions.get(&m.id);
            let mut update = None;
            let stored = cache
                .states
                .get(&m.id)
                .cloned()
                .unwrap_or(StoreInstallState::NotInstalled);
            let mut install = match stored {
                StoreInstallState::Installed { revision_key } => {
                    if let Some(newer) = superseders.get(&m.id) {
                        update = Some(UpdateAvailable::Superseded {
                            by: newer.id.clone(),
                        });
                    }
                    InstallState::Installed { revision_key }
                }
                StoreInstallState::Partial { done } => InstallState::Paused {
                    done: done.min(m.total_size()),
                    total: m.total_size(),
                },
                StoreInstallState::Corrupt => InstallState::Failed {
                    reason: FailReason::Io {
                        message: "installed files changed on disk".into(),
                    },
                    retryable: true,
                },
                StoreInstallState::NotInstalled => {
                    let current = m.revision_key();
                    match revs.and_then(|r| r.iter().find(|k| **k != current)) {
                        Some(old) => {
                            update = Some(UpdateAvailable::NewRevision {
                                to_revision_key: current,
                            });
                            InstallState::Installed {
                                revision_key: old.clone(),
                            }
                        }
                        None => InstallState::NotInstalled,
                    }
                }
            };
            if let Some(js) = job_installs.get(&m.id) {
                install = js.clone();
            }
            models.push(ModelView {
                id: m.id.clone(),
                role: Some(m.role),
                install,
                load: loads.get(&m.id).cloned().unwrap_or(LoadState::Unloaded),
                update,
                size: m.total_size(),
                runtime_supported: runtimes.contains(&m.runtime.name.as_str()),
                in_catalogue: true,
            });
        }
        for (id, revs) in &cache.revisions {
            if seen.contains(id) {
                continue;
            }
            models.push(ModelView {
                id: id.clone(),
                role: None,
                install: InstallState::Installed {
                    revision_key: revs[0].clone(),
                },
                load: loads.get(id).cloned().unwrap_or(LoadState::Unloaded),
                update: None,
                size: 0,
                runtime_supported: true,
                in_catalogue: false,
            });
        }

        let jobs = self
            .queue
            .iter()
            .filter_map(|id| {
                job_installs.get(id).map(|st| JobView {
                    id: id.clone(),
                    install: st.clone(),
                })
            })
            .collect();

        let snap = ManagerSnapshot {
            models,
            jobs,
            storage: cache.usage,
            migration: self.migration.clone(),
            catalogue_version: catalogue.catalogue_version,
        };
        *self.sh.snapshot.write() = Arc::new(snap);
        self.last_build = now;
        self.dirty = false;
        self.sh.wake();
    }
}
