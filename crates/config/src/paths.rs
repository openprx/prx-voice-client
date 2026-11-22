//! The application identifier and the resolution of every storage directory
//! (design section 4.3).
//!
//! The application identifier is defined once, as [`APP_ID`]. Every directory
//! comes from `ProjectDirs::from(APP_ID.0, APP_ID.1, APP_ID.2)`. On macOS that
//! is `~/Library/Application Support/org.openprx.prx-voice-client/`, the same
//! string as the bundle identifier.
//!
//! Only [`AppPaths::resolve`] looks at the real user directories.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::{AppConfig, ConfigError};

/// Qualifier, organisation and application name. The macOS bundle identifier
/// and the storage directory name are both `org.openprx.prx-voice-client`.
pub const APP_ID: (&str, &str, &str) = ("org", "openprx", "prx-voice-client");

/// Environment variable that overrides the data and models roots (tests and
/// isolated runs).
pub const DATA_DIR_ENV: &str = "PRXVC_DATA_DIR";

/// The identifier as one string, `org.openprx.prx-voice-client`. The packaging
/// derives `CFBundleIdentifier` from the same constant.
pub fn bundle_identifier() -> String {
    format!("{}.{}.{}", APP_ID.0, APP_ID.1, APP_ID.2)
}

pub(crate) fn project_dirs(id: (&str, &str, &str)) -> Result<ProjectDirs, ConfigError> {
    ProjectDirs::from(id.0, id.1, id.2).ok_or_else(|| {
        ConfigError::IoError(io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine the application directories",
        ))
    })
}

/// The platform configuration directory for [`APP_ID`].
pub fn default_config_dir() -> Result<PathBuf, ConfigError> {
    Ok(project_dirs(APP_ID)?.config_dir().to_path_buf())
}

// ---------------------------------------------------------------------------
// AppPaths
// ---------------------------------------------------------------------------

/// Every directory the application uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    /// Database, voices and logs.
    pub data_dir: PathBuf,
    /// Honours `models.dir` when it is set.
    pub models_dir: PathBuf,
    pub voices_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl AppPaths {
    /// Resolve from the real platform directories. `PRXVC_DATA_DIR`, when set
    /// and not empty, replaces the data and models roots.
    pub fn resolve(config: &AppConfig) -> Result<Self, ConfigError> {
        let over = std::env::var_os(DATA_DIR_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        Self::resolve_with(config, over.as_deref())
    }

    /// [`AppPaths::resolve`] with the override passed in.
    pub fn resolve_with(
        config: &AppConfig,
        data_override: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        let dirs = project_dirs(APP_ID)?;
        let (data_dir, default_models) = match data_override {
            Some(root) => (root.to_path_buf(), root.join("models")),
            None => (
                dirs.data_dir().to_path_buf(),
                dirs.data_local_dir().join("models"),
            ),
        };
        Ok(Self::from_roots(
            dirs.config_dir().to_path_buf(),
            data_dir,
            default_models,
            config,
        ))
    }

    /// Build from explicit roots; `models.dir` of `config` overrides
    /// `default_models_dir`.
    pub fn from_roots(
        config_dir: PathBuf,
        data_dir: PathBuf,
        default_models_dir: PathBuf,
        config: &AppConfig,
    ) -> Self {
        let models_dir = if config.models.dir.is_empty() {
            default_models_dir
        } else {
            PathBuf::from(&config.models.dir)
        };
        Self {
            voices_dir: data_dir.join("voices"),
            logs_dir: data_dir.join("logs"),
            config_dir,
            data_dir,
            models_dir,
        }
    }

    /// Create every directory, private to the user (0700 on Unix).
    pub fn ensure_dirs(&self) -> io::Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.models_dir,
            &self.voices_dir,
            &self.logs_dir,
        ] {
            create_private_dir_all(dir)?;
        }
        Ok(())
    }
}

/// `create_dir_all` with mode 0700 on Unix for the directories it creates.
pub fn create_private_dir_all(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

#[cfg(test)]
mod tests;
