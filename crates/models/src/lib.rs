//! Model catalogue, on-disk model store and resumable download engine for the
//! PRX voice client (design: `ux-and-model-management.md`, section 4).
//!
//! Module map:
//! - [`catalogue`], [`hardware`]: what models exist and what suits this machine.
//! - [`store`]: where installed models live and whether they are ready.
//! - [`download`]: fetching, verifying and promoting a model.
//! - [`manager`]: the job queue, snapshots and events used by the application.
//! - [`net`], [`types`], [`control`]: endpoints and policy, shared state types,
//!   cross-thread flags.

pub mod catalogue;
pub mod control;
pub mod download;
pub mod hardware;
pub mod manager;
pub mod net;
pub mod store;
pub mod types;

pub use catalogue::{
    Badge, Candidate, Catalogue, CatalogueError, CatalogueWarning, FileSource, FileSpec, LangKey,
    LanguageSpec, ModelEntry, ModelId, NotSelectable, Role, Streaming,
};
pub use hardware::{Arch, HardwareInfo, Os};
pub use manager::{
    EVENT_CAPACITY, EventReceiver, JobView, ManagerConfig, ManagerOptions, ManagerSnapshot,
    ModelCommand, ModelEvent, ModelHandle, ModelManager, ModelView,
};
pub use net::{Blocked, Endpoint, NetworkPolicy};
pub use store::{
    GcPlan, MigrationProgress, MigrationReport, ModelStore, ReadyFile, ReadyModel, StorageUsage,
    StoreInstallState,
};
pub use types::{
    ActiveSet, FailReason, InstallState, LoadState, ManagerError, NotReady, StoreError,
    UpdateAvailable,
};
