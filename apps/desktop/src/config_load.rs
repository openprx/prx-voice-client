//! Configuration load at start-up: legacy-directory migration reporting and the resolved application directories.

/// Directories resolved once at start-up by [`initialize_config`].
static APP_PATHS: std::sync::OnceLock<prxvc_config::paths::AppPaths> = std::sync::OnceLock::new();

/// The resolved application directories (`None` before [`initialize_config`]
/// ran or when the platform has no home directory).
pub(crate) fn app_paths() -> Option<&'static prxvc_config::paths::AppPaths> {
    APP_PATHS.get()
}

/// Log what a legacy directory migration did.
fn log_migration(what: &str, report: &prxvc_config::legacy::MigrationReport) {
    if report.is_noop() {
        return;
    }
    tracing::info!(
        "{what}: copied {}, moved {}, removed {} empty folder(s)",
        report.copied.len(),
        report.moved.len(),
        report.removed_dirs.len()
    );
    for (path, reason) in &report.failures {
        tracing::warn!("{what}: could not migrate {}: {reason}", path.display());
    }
}

/// Migrate the legacy directories, load [`AppConfig`] and resolve the
/// application directories.
///
/// A settings file that cannot be read is kept under another name and
/// defaults are used (the user-facing notice is a later work package; for
/// now it is logged). Falls back to defaults if the directory cannot be
/// resolved.
pub(crate) fn initialize_config() -> prxvc_config::AppConfig {
    use prxvc_config::{legacy, paths::AppPaths};
    let legacy_dirs = legacy::LegacyDirs::resolve().ok();
    let default_paths = AppPaths::resolve(&prxvc_config::AppConfig::default()).ok();
    if let (Some(l), Some(p)) = (&legacy_dirs, &default_paths) {
        log_migration(
            "config and database migration",
            &legacy::migrate_legacy_config_and_database(&l.config_dir, &l.data_dir, &p.config_dir, &p.data_dir),
        );
    }
    let config = match prxvc_config::AppConfigManager::with_default_dir() {
        Ok(mgr) => mgr.load().unwrap_or_else(|e| {
            tracing::error!("Failed to load config: {e}");
            prxvc_config::AppConfig::default()
        }),
        Err(_) => prxvc_config::AppConfig::default(),
    };
    if let Ok(paths) = AppPaths::resolve(&config) {
        if let Err(e) = paths.ensure_dirs() {
            tracing::warn!("Failed to create application directories: {e}");
        }
        if let Some(l) = &legacy_dirs {
            log_migration(
                "model files migration",
                &legacy::migrate_legacy_models(&l.models_dir, &paths.models_dir),
            );
        }
        let _ = APP_PATHS.set(paths);
    }
    config
}
