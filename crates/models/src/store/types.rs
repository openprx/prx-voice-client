//! Plain data types of the store: states, receipts, plans and reports.
//! Single responsibility: the store's vocabulary, no behaviour beyond small accessors.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalogue::{ModelId, Role};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreInstallState {
    NotInstalled,
    /// Some bytes are on disk (partial downloads or adopted files) but the
    /// model is not complete.
    Partial {
        done: u64,
    },
    Installed {
        revision_key: String,
    },
    /// A receipt exists for the entry's revision but it does not hold up
    /// (a file changed, or the receipt does not match the catalogue pin).
    Corrupt,
}

#[derive(Clone, Debug)]
pub struct ReadyModel {
    pub id: ModelId,
    pub revision_key: String,
    pub role: Role,
    pub runtime: String,
    pub runtime_params: serde_json::Value,
    /// In catalogue order.
    pub files: Vec<ReadyFile>,
}

#[derive(Clone, Debug)]
pub struct ReadyFile {
    pub purpose: String,
    pub path: PathBuf,
}

impl ReadyModel {
    pub fn file(&self, purpose: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|f| f.purpose == purpose)
            .map(|f| f.path.as_path())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledModel {
    pub id: ModelId,
    pub revision_key: String,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageUsage {
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptFile {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub mtime_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub schema: u32,
    pub model_id: ModelId,
    pub revision_key: String,
    pub catalogue_version: u64,
    pub files: Vec<ReceiptFile>,
    pub installed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Sidecar {
    pub(super) size: u64,
    pub(super) sha256: String,
    pub(super) url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcKind {
    /// `<revision>.tmp` directory or set-aside leftovers.
    TmpDir,
    /// Partial file whose sidecar matches no catalogue entry.
    OrphanPartial,
    /// Partial file older than 30 days that belongs to no job.
    StalePartial,
    /// An older revision of a model whose newer revision is installed.
    OldRevision,
    /// An installed model that is neither active nor loaded (manual only).
    UnusedModel,
}

#[derive(Clone, Debug)]
pub struct GcItem {
    pub kind: GcKind,
    pub path: PathBuf,
    pub bytes: u64,
    pub model_id: Option<ModelId>,
}

impl GcItem {
    /// Automatic items are things the user cannot want; `UnusedModel` is manual.
    pub fn is_automatic(&self) -> bool {
        self.kind != GcKind::UnusedModel
    }
}

#[derive(Clone, Debug, Default)]
pub struct GcPlan {
    pub items: Vec<GcItem>,
}

impl GcPlan {
    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }
    pub fn automatic(&self) -> GcPlan {
        GcPlan {
            items: self
                .items
                .iter()
                .filter(|i| i.is_automatic())
                .cloned()
                .collect(),
        }
    }
    pub fn unused_models(&self) -> GcPlan {
        GcPlan {
            items: self
                .items
                .iter()
                .filter(|i| i.kind == GcKind::UnusedModel)
                .cloned()
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MigrationProgress {
    pub file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MigrationReport {
    /// (legacy file name, model it was adopted into)
    pub moved: Vec<(String, ModelId)>,
    /// Models that became complete (receipt written) during this run.
    pub installed: Vec<ModelId>,
    /// Files no catalogue entry claims (name+size+hash); left in place.
    pub unrecognised: Vec<PathBuf>,
    /// Files that match an already installed model; left in place.
    pub duplicates: Vec<PathBuf>,
    pub errors: Vec<String>,
    pub legacy_dir_removed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovePhase {
    Copying,
    Verifying,
}

#[derive(Clone, Copy, Debug)]
pub struct MoveProgress {
    pub phase: MovePhase,
    pub done: u64,
    pub total: u64,
}

/// Steps of the atomic promotion, for crash-injection tests.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromoteStep {
    Synced,
    TmpCreated,
    Renamed,
    ReceiptWritten,
    BeforeFinalRename,
}
