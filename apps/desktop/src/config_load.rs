//! Configuration load at start-up: legacy-directory migration reporting and the resolved application directories.

/// Directories resolved once at start-up by [`initialize_config`].
static APP_PATHS: std::sync::OnceLock<prxvc_config::paths::AppPaths> = std::sync::OnceLock::new();

/// What the settings file load reported (unreadable file kept aside, migration, secrets).
static LOAD_NOTICES: std::sync::OnceLock<Vec<prxvc_config::LoadNotice>> = std::sync::OnceLock::new();

/// The notices of the start-up load, for the settings window to show.
pub(crate) fn load_notices() -> &'static [prxvc_config::LoadNotice] {
    LOAD_NOTICES.get().map_or(&[], Vec::as_slice)
}

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

/// Log what the load reported (the settings window shows the unreadable-file notice).
fn log_notices(notices: &[prxvc_config::LoadNotice]) {
    use prxvc_config::LoadNotice;
    for notice in notices {
        match notice {
            LoadNotice::UnreadableFileKept { kept_as, reason } => tracing::warn!(
                "settings file could not be read ({reason}); kept as {}, defaults in use",
                kept_as.display()
            ),
            LoadNotice::Migrated { from, to } => {
                tracing::info!("settings file migrated in memory from schema {from} to {to}");
            }
            LoadNotice::SecretsMoved { entry_ids } => {
                tracing::info!("moved API keys of {} provider(s) out of the settings file", entry_ids.len());
            }
            LoadNotice::SecretsNotMoved { locations } => tracing::warn!(
                "API keys found in the settings file could not be moved to a credential store: {}",
                locations.join(", ")
            ),
        }
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
        Ok(mgr) => match mgr.load_report() {
            Ok(report) => {
                log_notices(&report.notices);
                let _ = LOAD_NOTICES.set(report.notices);
                report.config
            }
            Err(e) => {
                tracing::error!("Failed to load config: {e}");
                prxvc_config::AppConfig::default()
            }
        },
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
