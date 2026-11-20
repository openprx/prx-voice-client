//! Shared state and error types (design 4.2, 4.4, 4.7).

use std::path::PathBuf;

use crate::catalogue::{LangKey, ModelId, Role};

/// Why a download or preparation failed (design 4.2 table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailReason {
    Offline,
    Network { message: String },
    SourceRejected { status: u16 },
    DiskFull { needed: u64, available: u64 },
    ChecksumMismatch { file: String },
    Io { message: String },
    LoadFailed { message: String },
    UnsupportedRuntime { name: String },
}

impl std::fmt::Display for FailReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailReason::Offline => f.write_str("offline"),
            FailReason::Network { message } => write!(f, "network error: {message}"),
            FailReason::SourceRejected { status } => {
                write!(f, "source refused the file ({status})")
            }
            FailReason::DiskFull { needed, available } => {
                write!(f, "needs {needed} bytes, {available} available")
            }
            FailReason::ChecksumMismatch { file } => {
                write!(f, "{file} failed checksum verification")
            }
            FailReason::Io { message } => write!(f, "i/o error: {message}"),
            FailReason::LoadFailed { message } => write!(f, "load failed: {message}"),
            FailReason::UnsupportedRuntime { name } => write!(f, "unsupported runtime {name}"),
        }
    }
}

/// Install state of a model as shown to the UI (design 4.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallState {
    NotInstalled,
    Queued,
    Downloading {
        done: u64,
        total: u64,
        bytes_per_s: u64,
    },
    Paused {
        done: u64,
        total: u64,
    },
    Verifying {
        done: u64,
        total: u64,
    },
    Installed {
        revision_key: String,
    },
    Failed {
        reason: FailReason,
        retryable: bool,
    },
}

/// Load state, reported by the runtime owner (design 4.4).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LoadState {
    #[default]
    Unloaded,
    Loading,
    WarmingUp,
    Ready,
    LoadFailed {
        message: String,
    },
}

impl LoadState {
    /// True while a runtime holds (or is acquiring) the model files.
    pub fn is_loaded(&self) -> bool {
        matches!(
            self,
            LoadState::Loading | LoadState::WarmingUp | LoadState::Ready
        )
    }
}

/// Derived: an installed model has a newer pin or a superseding entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateAvailable {
    /// Same id, different pin than the installed receipt.
    NewRevision { to_revision_key: String },
    /// Another entry lists this id in `supersedes`.
    Superseded { by: ModelId },
}

/// Why `resolve` has no model to return (design 4.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotReady {
    NoSelection,
    NotInstalled { id: ModelId, size: u64 },
    InProgress { id: ModelId, state: InstallState },
    Failed { id: ModelId, reason: FailReason },
    UnsupportedRuntime { id: ModelId, runtime: String },
}

impl std::fmt::Display for NotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotReady::NoSelection => f.write_str("no model selected"),
            NotReady::NotInstalled { id, .. } => write!(f, "model {id} is not installed"),
            NotReady::InProgress { id, .. } => write!(f, "model {id} is still being installed"),
            NotReady::Failed { id, reason } => write!(f, "model {id} failed: {reason}"),
            NotReady::UnsupportedRuntime { id, runtime } => {
                write!(
                    f,
                    "model {id} needs runtime {runtime}, which this build lacks"
                )
            }
        }
    }
}

impl std::error::Error for NotReady {}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the model store is open read-only (another instance holds the lock)")]
    ReadOnly,
    #[error("model {0} is being downloaded or otherwise in use")]
    Busy(ModelId),
    #[error("operation cancelled")]
    Cancelled,
    #[error("{file} does not match its recorded SHA-256")]
    ChecksumMismatch { file: String },
    #[error("not enough free space: needs {needed} bytes, {available} available")]
    DiskFull { needed: u64, available: u64 },
    #[error("{0}")]
    Other(String),
}

impl StoreError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        StoreError::Io {
            path: path.into(),
            source,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not start the download client: {0}")]
    Client(String),
    #[error("could not start manager threads: {0}")]
    Thread(String),
}

/// Selection of active models, keyed by role and language key (from config
/// `providers.active`, local-model entries only).
#[derive(Clone, Debug, Default)]
pub struct ActiveSet {
    pub entries: Vec<(Role, LangKey, ModelId)>,
}

impl ActiveSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, role: Role, key: LangKey, id: ModelId) -> Self {
        self.entries.push((role, key, id));
        self
    }

    pub fn contains(&self, id: &ModelId) -> bool {
        self.entries.iter().any(|(_, _, i)| i == id)
    }

    pub fn get(&self, role: Role, key: &LangKey) -> Option<&ModelId> {
        self.entries
            .iter()
            .find(|(r, k, _)| *r == role && k == key)
            .map(|(_, _, i)| i)
    }
}
