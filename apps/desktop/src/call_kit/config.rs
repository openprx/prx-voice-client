//! Reading the configuration and the application directories fresh, so that a change made between two calls (by hand or by a set-up script) takes effect at the next start.

use std::sync::Arc;

use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;

/// Where the call reads its configuration from.
pub(crate) type ConfigSource = Arc<dyn Fn() -> AppConfig + Send + Sync>;

/// The configuration file as it is now; defaults when it is missing or unreadable (the loader
/// keeps an unreadable file aside and logs it).
pub(crate) fn load_current() -> AppConfig {
    match prxvc_config::AppConfigManager::with_default_dir() {
        Ok(manager) => manager.load().unwrap_or_else(|e| {
            tracing::error!("could not load the configuration for the call: {e}");
            AppConfig::default()
        }),
        Err(e) => {
            tracing::error!("the configuration directory is unknown: {e}");
            AppConfig::default()
        }
    }
}

/// The source that reads the real configuration file.
pub(crate) fn current_source() -> ConfigSource {
    Arc::new(load_current)
}

/// The application directories for `config` (honours `PRXVC_DATA_DIR` and `models.dir`).
pub(crate) fn paths_for(config: &AppConfig) -> Result<AppPaths, String> {
    AppPaths::resolve(config).map_err(|e| e.to_string())
}
