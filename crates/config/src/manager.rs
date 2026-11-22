//! Reading and writing `config.toml`: load with migration and unreadable-file
//! handling, atomic save, and read-modify-write updates.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::credentials::{self, CredentialStore, UnavailableCredentialStore};
use crate::migrate::{self, CURRENT_SCHEMA_VERSION};
use crate::{AppConfig, ConfigError, atomic, paths};

// ---------------------------------------------------------------------------
// Load report
// ---------------------------------------------------------------------------

/// Something the user should be told about after a load.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadNotice {
    /// The file could not be read. It was renamed to `kept_as` and defaults
    /// are in use for this run (design 2.3 rule 3).
    UnreadableFileKept { kept_as: PathBuf, reason: String },
    /// An older file was upgraded in memory. It is written back on the next
    /// save.
    Migrated { from: u32, to: u32 },
    /// API keys found in the file were handed to the credential store and
    /// the file was rewritten without them (design 2.5, 4.8.4).
    SecretsMoved { entry_ids: Vec<String> },
    /// API keys found in the file could not be handed to the credential
    /// store (none is available yet). They stay where they are.
    SecretsNotMoved { locations: Vec<String> },
}

/// The result of [`ConfigManager::load_report`].
#[derive(Debug, Clone)]
pub struct LoadReport {
    pub config: AppConfig,
    pub notices: Vec<LoadNotice>,
}

// ---------------------------------------------------------------------------
// Config manager
// ---------------------------------------------------------------------------

/// File name of the configuration inside the config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Manages reading and writing [`AppConfig`] from a TOML file on disk.
pub struct ConfigManager {
    config_dir: PathBuf,
    credentials: Arc<dyn CredentialStore>,
    /// Serialises `update` calls made through this manager.
    write_lock: std::sync::Mutex<()>,
}

/// What is on disk.
enum FileState {
    Missing,
    /// Parsed and migrated. `table` is the migrated TOML, `config` the typed
    /// view of it.
    Readable {
        table: toml::Table,
        config: Box<AppConfig>,
        file_version: u32,
        migrated_from: Option<u32>,
    },
    Unreadable(String),
}

impl ConfigManager {
    /// Create a new manager rooted at `config_dir`. API keys found in the file
    /// stay in place until a credential store is set with
    /// [`ConfigManager::with_credential_store`].
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            credentials: Arc::new(UnavailableCredentialStore),
            write_lock: std::sync::Mutex::new(()),
        }
    }

    /// Use `store` for API keys found in the file.
    pub fn with_credential_store(mut self, store: Arc<dyn CredentialStore>) -> Self {
        self.credentials = store;
        self
    }

    /// Path to the configuration file.
    fn config_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    fn inspect(&self) -> Result<FileState, ConfigError> {
        let bytes = match std::fs::read(self.config_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileState::Missing),
            Err(e) => return Err(e.into()),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(FileState::Unreadable("file is not valid UTF-8".to_owned()));
        };
        Ok(parse_config_text(&text))
    }

    /// Load configuration from disk.
    ///
    /// Returns [`AppConfig::default`] when the file does not exist, and also
    /// when it exists but cannot be read: the file is then kept under another
    /// name (see [`LoadNotice::UnreadableFileKept`]) and a warning is logged.
    /// Use [`ConfigManager::load_report`] to get the notices.
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        let report = self.load_report()?;
        for notice in &report.notices {
            match notice {
                LoadNotice::UnreadableFileKept { kept_as, reason } => tracing::warn!(
                    "settings file could not be read ({reason}); kept as {}, defaults in use",
                    kept_as.display()
                ),
                LoadNotice::Migrated { from, to } => {
                    tracing::info!("settings file migrated in memory from schema {from} to {to}")
                }
                LoadNotice::SecretsMoved { entry_ids } => {
                    tracing::info!(
                        "moved API keys of {} provider(s) out of the settings file",
                        entry_ids.len()
                    )
                }
                LoadNotice::SecretsNotMoved { locations } => tracing::warn!(
                    "API keys found in the settings file could not be moved to a credential store: {}",
                    locations.join(", ")
                ),
            }
        }
        Ok(report.config)
    }

    /// Load configuration and report what happened.
    pub fn load_report(&self) -> Result<LoadReport, ConfigError> {
        let mut notices = Vec::new();
        match self.inspect()? {
            FileState::Missing => {
                tracing::debug!("config file not found, returning defaults");
                Ok(LoadReport {
                    config: AppConfig::default(),
                    notices,
                })
            }
            FileState::Unreadable(reason) => {
                let kept_as = atomic::set_aside(&self.config_path(), "unreadable")?;
                notices.push(LoadNotice::UnreadableFileKept { kept_as, reason });
                Ok(LoadReport {
                    config: AppConfig::default(),
                    notices,
                })
            }
            FileState::Readable {
                mut table,
                mut config,
                migrated_from,
                ..
            } => {
                if let Some(from) = migrated_from {
                    notices.push(LoadNotice::Migrated {
                        from,
                        to: CURRENT_SCHEMA_VERSION,
                    });
                }
                let outcome = credentials::extract_secrets(&mut table, self.credentials.as_ref());
                if !outcome.moved.is_empty() || !outcome.kept.is_empty() {
                    // Re-read the typed view: `api_key` of an entry that moved
                    // is gone, one that stayed is held as a pending secret.
                    match toml::Value::Table(table).try_into::<AppConfig>() {
                        Ok(mut c) => {
                            c.normalize();
                            *config = c;
                        }
                        Err(e) => return Err(ConfigError::ParseError(e.to_string())),
                    }
                }
                if !outcome.kept.is_empty() {
                    notices.push(LoadNotice::SecretsNotMoved {
                        locations: outcome.kept.clone(),
                    });
                }
                if !outcome.stray_kept.is_empty() {
                    // A secret outside any provider entry cannot be held in
                    // the typed config, so a later save would drop it. Keep a
                    // copy of the file first.
                    atomic::copy_aside(&self.config_path(), "secrets")?;
                }
                let clean = outcome.kept.is_empty() && outcome.stray_kept.is_empty();
                if let Some(from) = migrated_from.filter(|_| clean) {
                    // Persist the migration once, so that the next load finds a current file and
                    // says nothing: the original is kept next to it first.
                    match atomic::copy_aside(&self.config_path(), &format!("bak-schema{from}"))
                        .map_err(ConfigError::from)
                        .and_then(|_| self.write(&config))
                    {
                        Ok(()) => tracing::debug!(from, "the migrated settings file was written"),
                        Err(e) => tracing::warn!(
                            from,
                            error = %e,
                            "the migrated settings file could not be written; it is migrated in memory again at the next load"
                        ),
                    }
                } else if !outcome.moved.is_empty() && outcome.kept.is_empty() {
                    self.write(&config)?;
                }
                if !outcome.moved.is_empty() && outcome.kept.is_empty() {
                    notices.push(LoadNotice::SecretsMoved {
                        entry_ids: outcome.moved,
                    });
                }
                Ok(LoadReport {
                    config: *config,
                    notices,
                })
            }
        }
    }

    /// Persist `config` to disk as TOML, atomically.
    ///
    /// A file on disk that cannot be read is first renamed aside, never
    /// overwritten. A file written by a newer schema is first copied aside.
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        match self.inspect()? {
            FileState::Unreadable(reason) => {
                let kept_as = atomic::set_aside(&self.config_path(), "unreadable")?;
                tracing::warn!(
                    "settings file could not be read ({reason}); kept as {}",
                    kept_as.display()
                );
            }
            FileState::Readable { file_version, .. } if file_version > CURRENT_SCHEMA_VERSION => {
                atomic::copy_aside(&self.config_path(), &format!("v{file_version}"))?;
            }
            _ => {}
        }
        self.write(config)
    }

    fn write(&self, config: &AppConfig) -> Result<(), ConfigError> {
        let mut config = config.clone();
        config.normalize();
        config.schema_version = CURRENT_SCHEMA_VERSION;
        let content =
            toml::to_string_pretty(&config).map_err(|e| ConfigError::ParseError(e.to_string()))?;
        let path = self.config_path();
        atomic::write_atomic(&path, content.as_bytes())?;
        tracing::debug!("configuration saved to {}", path.display());
        Ok(())
    }

    /// Re-read the file, apply `f`, write the result atomically and return it.
    ///
    /// Each settings control calls this with a closure that changes only its
    /// own key, so a field the control does not know about is never lost.
    pub fn update(&self, f: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, ConfigError> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut config = self.load()?;
        f(&mut config);
        config.normalize();
        self.save(&config)?;
        Ok(config)
    }

    /// Return the configuration directory path.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }
}

/// Parse, migrate and type-check the text of a configuration file.
fn parse_config_text(text: &str) -> FileState {
    let mut table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => return FileState::Unreadable(e.to_string()),
    };
    let file_version = match migrate::file_version(&table) {
        Ok(v) => v,
        Err(e) => return FileState::Unreadable(e.to_string()),
    };
    let migrated_from = match migrate::migrate(&mut table) {
        Ok(Some(from)) => Some(from),
        Ok(None) => None,
        Err(e) => return FileState::Unreadable(e.to_string()),
    };
    match toml::Value::Table(table.clone()).try_into::<AppConfig>() {
        Ok(mut config) => {
            config.normalize();
            FileState::Readable {
                table,
                config: Box::new(config),
                file_version,
                migrated_from,
            }
        }
        Err(e) => FileState::Unreadable(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// AppConfigManager
// ---------------------------------------------------------------------------

/// App-level config manager with platform-aware path resolution.
///
/// Wraps [`ConfigManager`] with a convenience constructor that locates the
/// configuration directory from [`paths::APP_ID`].
pub struct AppConfigManager {
    manager: ConfigManager,
}

impl AppConfigManager {
    /// Create using the platform default configuration directory
    /// (see [`paths::AppPaths`]).
    pub fn with_default_dir() -> Result<Self, ConfigError> {
        Ok(Self {
            manager: ConfigManager::new(paths::default_config_dir()?),
        })
    }

    /// Create with an explicit directory.
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            manager: ConfigManager::new(dir),
        }
    }

    /// Use `store` for API keys found in the file.
    pub fn with_credential_store(mut self, store: Arc<dyn CredentialStore>) -> Self {
        self.manager = self.manager.with_credential_store(store);
        self
    }

    /// Load configuration, returning defaults if the file does not exist or
    /// cannot be read (in which case the file is kept aside).
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        self.manager.load()
    }

    /// Load configuration and report what happened.
    pub fn load_report(&self) -> Result<LoadReport, ConfigError> {
        self.manager.load_report()
    }

    /// Save configuration to disk.
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        self.manager.save(config)
    }

    /// Re-read, change and atomically write the configuration.
    pub fn update(&self, f: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, ConfigError> {
        self.manager.update(f)
    }

    /// Return the configuration directory path.
    pub fn config_dir(&self) -> &Path {
        self.manager.config_dir()
    }
}
