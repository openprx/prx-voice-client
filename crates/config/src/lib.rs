use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Configuration sections
// ---------------------------------------------------------------------------

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub audio: AudioConfig,
    pub ui: UiConfig,
    pub network: NetworkConfig,
    pub privacy: PrivacyConfig,
    pub debug: DebugConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            audio: AudioConfig::default(),
            ui: UiConfig::default(),
            network: NetworkConfig::default(),
            privacy: PrivacyConfig::default(),
            debug: DebugConfig::default(),
        }
    }
}

/// Audio-related settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub auto_play_reply: bool,
    pub echo_control: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            auto_play_reply: true,
            echo_control: true,
        }
    }
}

/// UI presentation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub language: String,
    pub show_transcript: bool,
    pub compact_mode: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: String::from("en"),
            show_transcript: true,
            compact_mode: false,
        }
    }
}

/// Network / connectivity settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub connect_timeout_ms: u64,
    pub reconnect_attempts: u32,
    pub reconnect_delay_ms: u64,
    /// PRX Voice server base URL.
    pub server_url: String,
    /// Whether to use the mock gateway (offline) instead of a live server.
    pub use_mock: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 5_000,
            reconnect_attempts: 3,
            reconnect_delay_ms: 1_000,
            server_url: "http://localhost:3000".to_owned(),
            use_mock: true,
        }
    }
}

/// Privacy / data-retention settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub save_history: bool,
    pub save_transcript: bool,
    pub log_sensitive: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            save_history: true,
            save_transcript: true,
            log_sensitive: false,
        }
    }
}

/// Debug / developer settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugConfig {
    pub enabled: bool,
    pub log_level: String,
    pub show_debug_panel: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_level: String::from("info"),
            show_debug_panel: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the configuration subsystem.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("validation error: {0}")]
    ValidationError(String),
    #[error("migration error: {0}")]
    MigrationError(String),
}

// ---------------------------------------------------------------------------
// Config manager
// ---------------------------------------------------------------------------

/// Manages reading and writing [`AppConfig`] from a TOML file on disk.
pub struct ConfigManager {
    config_dir: PathBuf,
}

impl ConfigManager {
    /// Create a new manager rooted at `config_dir`.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// Path to the configuration file.
    fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Load configuration from disk.
    ///
    /// Returns [`AppConfig::default`] when the file does not yet exist.
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        let path = self.config_path();
        if !path.exists() {
            tracing::debug!("config file not found, returning defaults");
            return Ok(AppConfig::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let config: AppConfig =
            toml::from_str(&content).map_err(|e| ConfigError::ParseError(e.to_string()))?;
        Ok(config)
    }

    /// Persist `config` to disk as TOML.
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        let path = self.config_path();

        // Ensure the parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content =
            toml::to_string_pretty(config).map_err(|e| ConfigError::ParseError(e.to_string()))?;
        std::fs::write(&path, content)?;
        tracing::debug!("configuration saved to {}", path.display());
        Ok(())
    }
}

impl ConfigManager {
    /// Return the configuration directory path.
    pub fn config_dir(&self) -> &std::path::Path {
        &self.config_dir
    }
}

// ---------------------------------------------------------------------------
// AppConfigManager
// ---------------------------------------------------------------------------

/// App-level config manager with platform-aware path resolution.
///
/// Wraps [`ConfigManager`] with convenience constructors that locate the
/// platform-specific configuration directory automatically.
pub struct AppConfigManager {
    manager: ConfigManager,
}

impl AppConfigManager {
    /// Create using the platform default configuration directory.
    ///
    /// Uses `directories::ProjectDirs` to resolve `~/.config/<app_name>` (Linux),
    /// `~/Library/Application Support/<app_name>` (macOS), etc.
    pub fn with_default_dir(app_name: &str) -> Result<Self, ConfigError> {
        let dirs = directories::ProjectDirs::from("com", "prx", app_name).ok_or_else(|| {
            ConfigError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not determine config directory",
            ))
        })?;
        let config_dir = dirs.config_dir().to_path_buf();
        Ok(Self {
            manager: ConfigManager::new(config_dir),
        })
    }

    /// Create with an explicit directory.
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            manager: ConfigManager::new(dir),
        }
    }

    /// Load configuration, returning defaults if the file does not exist.
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        self.manager.load()
    }

    /// Save configuration to disk.
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        self.manager.save(config)
    }

    /// Return the configuration directory path.
    pub fn config_dir(&self) -> &std::path::Path {
        self.manager.config_dir()
    }
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

pub mod prelude {
    pub use crate::{
        AppConfig, AppConfigManager, AudioConfig, ConfigError, ConfigManager, DebugConfig,
        NetworkConfig, PrivacyConfig, UiConfig,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Create a temporary directory with a unique name and return its path.
    fn temp_dir_unique(prefix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}_{ts}"));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    /// Clean up a temporary directory (best-effort).
    fn cleanup(dir: &PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.audio.sample_rate, 16_000);
        assert_eq!(cfg.audio.channels, 1);
        assert!(cfg.audio.auto_play_reply);
        assert!(cfg.audio.echo_control);
        assert_eq!(cfg.ui.language, "en");
        assert!(cfg.ui.show_transcript);
        assert!(!cfg.ui.compact_mode);
        assert!(!cfg.debug.enabled);
        assert_eq!(cfg.debug.log_level, "info");
    }

    #[test]
    fn test_config_manager_save_and_load() {
        let dir = temp_dir_unique("prxvc_cfg_save_load");
        let mgr = ConfigManager::new(dir.clone());

        let mut cfg = AppConfig::default();
        cfg.audio.sample_rate = 48_000;
        cfg.ui.language = String::from("zh");
        cfg.debug.enabled = true;

        mgr.save(&cfg).expect("save failed");
        let loaded = mgr.load().expect("load failed");

        assert_eq!(loaded.audio.sample_rate, 48_000);
        assert_eq!(loaded.ui.language, "zh");
        assert!(loaded.debug.enabled);

        cleanup(&dir);
    }

    #[test]
    fn test_config_manager_load_missing_returns_default() {
        let dir = temp_dir_unique("prxvc_cfg_missing");
        let mgr = ConfigManager::new(dir.clone());

        let cfg = mgr.load().expect("load failed");
        assert_eq!(cfg.audio.sample_rate, 16_000);
        assert_eq!(cfg.ui.language, "en");

        cleanup(&dir);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let original = AppConfig::default();
        let toml_str = toml::to_string_pretty(&original).expect("serialize failed");
        let restored: AppConfig = toml::from_str(&toml_str).expect("deserialize failed");

        assert_eq!(restored.audio.sample_rate, original.audio.sample_rate);
        assert_eq!(restored.audio.channels, original.audio.channels);
        assert_eq!(restored.ui.language, original.ui.language);
        assert_eq!(
            restored.network.connect_timeout_ms,
            original.network.connect_timeout_ms
        );
        assert_eq!(restored.privacy.save_history, original.privacy.save_history);
        assert_eq!(restored.debug.log_level, original.debug.log_level);
    }

    #[test]
    fn test_audio_config_defaults() {
        let cfg = AudioConfig::default();
        assert_eq!(cfg.sample_rate, 16_000);
        assert_eq!(cfg.channels, 1);
        assert!(cfg.auto_play_reply);
        assert!(cfg.echo_control);
    }

    #[test]
    fn test_network_config_defaults() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.connect_timeout_ms, 5_000);
        assert_eq!(cfg.reconnect_attempts, 3);
        assert_eq!(cfg.reconnect_delay_ms, 1_000);
    }

    #[test]
    fn test_privacy_config_defaults() {
        let cfg = PrivacyConfig::default();
        assert!(cfg.save_history);
        assert!(cfg.save_transcript);
        assert!(!cfg.log_sensitive);
    }

    #[test]
    fn test_debug_config_defaults() {
        let cfg = DebugConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.log_level, "info");
        assert!(!cfg.show_debug_panel);
    }

    #[test]
    fn test_app_config_manager_with_dir() {
        let dir = temp_dir_unique("prxvc_acm_dir");
        let acm = AppConfigManager::with_dir(dir.clone());

        // config_dir should match what we passed in
        assert_eq!(acm.config_dir(), dir.as_path());

        // Save and load round-trip
        let mut cfg = AppConfig::default();
        cfg.audio.sample_rate = 44_100;
        cfg.ui.language = String::from("ja");

        acm.save(&cfg).expect("save failed");
        let loaded = acm.load().expect("load failed");
        assert_eq!(loaded.audio.sample_rate, 44_100);
        assert_eq!(loaded.ui.language, "ja");

        cleanup(&dir);
    }

    #[test]
    fn test_app_config_manager_default_dir() {
        // with_default_dir should succeed on supported platforms (macOS/Linux/Windows)
        let result = AppConfigManager::with_default_dir("prxvc-test");
        assert!(result.is_ok());

        let acm = result.expect("with_default_dir failed");
        // The path should contain "prxvc-test" somewhere
        let dir_str = acm.config_dir().to_string_lossy();
        assert!(
            dir_str.contains("prxvc-test") || dir_str.contains("prx"),
            "config_dir should reference the app name: {dir_str}"
        );
    }
}
