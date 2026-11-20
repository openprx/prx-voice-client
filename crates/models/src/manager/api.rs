//! Public value types of the manager: configuration, commands, events and the
//! snapshot the UI reads. Single responsibility: the manager's vocabulary.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::catalogue::{Catalogue, LangKey, ModelId, Role};
use crate::download::EngineSettings;
use crate::hardware::HardwareInfo;
use crate::net::Endpoint;
use crate::store::{MigrationProgress, MigrationReport, StorageUsage};
use crate::types::{ActiveSet, FailReason, InstallState, LoadState, UpdateAvailable};

/// Largest number of simultaneous downloads (design 4.2 rule 2).
pub const MAX_CONCURRENT_LIMIT: usize = 3;

/// Knobs with design defaults that tests and the application may override.
#[derive(Clone)]
pub struct ManagerOptions {
    pub engine: EngineSettings,
    /// How often a job that failed as offline is probed again.
    pub offline_retry_interval: Duration,
    /// Minimum time between snapshots while jobs make progress.
    pub snapshot_interval: Duration,
    /// Detected when `None`.
    pub hardware: Option<HardwareInfo>,
    /// Longest wait for running jobs to stop on shutdown.
    pub shutdown_grace: Duration,
}

impl Default for ManagerOptions {
    fn default() -> Self {
        ManagerOptions {
            engine: EngineSettings::default(),
            offline_retry_interval: Duration::from_secs(30),
            snapshot_interval: Duration::from_millis(100),
            hardware: None,
            shutdown_grace: Duration::from_secs(2),
        }
    }
}

pub struct ManagerConfig {
    pub models_dir: PathBuf,
    pub legacy_models_dir: Option<PathBuf>,
    pub endpoint: Endpoint,
    pub max_concurrent: usize,
    pub app_version: semver::Version,
    /// Runtimes compiled into this build.
    pub runtimes: Vec<String>,
    pub catalogue: Catalogue,
    pub active: ActiveSet,
    pub options: ManagerOptions,
}

impl ManagerConfig {
    /// Defaults: Hugging Face, two downloads at a time, no legacy folder, the
    /// runtimes of the embedded catalogue's known set.
    pub fn new(models_dir: PathBuf, catalogue: Catalogue, app_version: semver::Version) -> Self {
        ManagerConfig {
            models_dir,
            legacy_models_dir: None,
            endpoint: Endpoint::default(),
            max_concurrent: 2,
            app_version,
            runtimes: ["whisper.cpp", "llama.cpp", "qwentts.cpp", "onnxruntime"]
                .into_iter()
                .map(String::from)
                .collect(),
            catalogue,
            active: ActiveSet::default(),
            options: ManagerOptions::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ModelCommand {
    Download(ModelId),
    DownloadNext(ModelId),
    Pause(ModelId),
    Resume(ModelId),
    Cancel(ModelId),
    Retry(ModelId),
    PauseAll,
    /// Preparation (design 4.5): make sure the model is installed and
    /// verified, then emit `PreparationDone`. Saving the selection is the
    /// application's job.
    Use {
        role: Role,
        key: LangKey,
        id: ModelId,
    },
    Remove(ModelId),
    RemoveUnused,
    SetEndpoint(Endpoint),
    SetMaxConcurrent(usize),
    ReplaceCatalogue(Catalogue),
}

#[derive(Clone, Debug)]
pub enum ModelEvent {
    StateChanged(ModelId),
    PreparationDone {
        role: Role,
        key: LangKey,
        id: ModelId,
    },
    PreparationFailed {
        role: Role,
        key: LangKey,
        id: ModelId,
        reason: FailReason,
    },
    MigrationFinished(MigrationReport),
}

#[derive(Clone, Debug)]
pub struct ModelView {
    pub id: ModelId,
    /// `None` for an installed model that is no longer in the catalogue.
    pub role: Option<Role>,
    pub install: InstallState,
    pub load: LoadState,
    pub update: Option<UpdateAvailable>,
    /// Total size of the files, 0 when unknown.
    pub size: u64,
    pub runtime_supported: bool,
    /// False for "No longer in the model list".
    pub in_catalogue: bool,
}

#[derive(Clone, Debug)]
pub struct JobView {
    pub id: ModelId,
    pub install: InstallState,
}

#[derive(Clone, Debug, Default)]
pub struct ManagerSnapshot {
    pub models: Vec<ModelView>,
    /// In queue order.
    pub jobs: Vec<JobView>,
    pub storage: StorageUsage,
    pub migration: Option<MigrationProgress>,
    pub catalogue_version: u64,
}

impl ManagerSnapshot {
    pub fn model(&self, id: &ModelId) -> Option<&ModelView> {
        self.models.iter().find(|m| &m.id == id)
    }
}

pub(crate) type Waker = Box<dyn Fn() + Send + Sync>;
pub(crate) type SharedSnapshot = Arc<ManagerSnapshot>;
