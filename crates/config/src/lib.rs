//! Application configuration: schema, versioned migrations, atomic writes and
//! storage locations.
//!
//! This file only declares the modules and re-exports the public API.
//!
//! * [`app_config`], [`sections`] and [`providers`] hold the typed
//!   `config.toml` (design 2.5).
//! * [`migrate`] upgrades an older file before it is deserialised.
//! * [`manager`] reads and writes the file (atomic writes, unreadable files
//!   are renamed aside, never overwritten).
//! * [`paths`] holds [`paths::APP_ID`] and directory resolution; [`legacy`]
//!   the one-time migration of the legacy directories.

mod app_config;
mod atomic;
pub mod credentials;
mod error;
pub mod legacy;
mod manager;
pub mod net_policy;
pub mod migrate;
pub mod paths;
mod providers;
mod sections;
#[cfg(test)]
mod test_util;
#[cfg(test)]
mod tests;

pub use app_config::AppConfig;
pub use credentials::{CredentialError, CredentialStore, PendingSecret, UnavailableCredentialStore};
pub use error::ConfigError;
pub use manager::{AppConfigManager, ConfigManager, LoadNotice, LoadReport, CONFIG_FILE_NAME};
pub use migrate::CURRENT_SCHEMA_VERSION;
pub use net_policy::{Blocked, NetworkPolicy};
pub use providers::*;
pub use sections::*;

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

pub mod prelude {
    pub use crate::paths::{AppPaths, APP_ID};
    pub use crate::{
        AppConfig, AppConfigManager, AudioConfig, ConfigError, ConfigManager, DebugConfig,
        EchoMode, LoadNotice, LoadReport, NetworkConfig, PrivacyConfig, UiConfig,
    };
}
