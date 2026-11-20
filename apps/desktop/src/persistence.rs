//! Persistence helpers: history load, session save after stop, and debounced settings write to config.toml.

use prxvc_desktop_ui::{HistoryEntry, SharedAppState};

use crate::settings;

/// Persist UI settings to config.toml (debounced: only writes when a persisted
/// value differs from the last write).
///
/// Starts from the configuration in effect and changes only the fields the
/// popup edits, so provider names and other hidden fields are preserved.
pub(crate) fn save_settings_to_config(
    config: &mut prxvc_config::AppConfig,
    last_saved: &mut settings::SettingsSnapshot,
    ui: &prxvc_desktop_ui::UiSettings,
) {
    let current = settings::snapshot(ui);
    if *last_saved == current {
        return;
    }
    settings::apply_ui_settings(config, ui);
    if let Ok(mgr) = prxvc_config::AppConfigManager::with_default_dir() {
        if let Err(e) = mgr.save(config) {
            tracing::warn!("Failed to save config: {}", e);
        } else {
            tracing::debug!("Config saved to disk");
        }
    }
    *last_saved = current;
}

/// Populate the UI history list from persisted session records.
pub(crate) fn load_history_from_storage(storage: &prxvc_storage::StorageManager, shared: &SharedAppState) {
    match storage.recent_sessions(20) {
        Ok(records) => {
            for record in &records {
                shared.add_history_entry(HistoryEntry {
                    session_id: record.id.clone(),
                    title: record.summary_title(),
                    created_at: record.created_at.clone(),
                    duration: record.duration_display(),
                    state: record.state.clone(),
                });
            }
            tracing::info!("Loaded {} history entries from storage", records.len());
        }
        Err(e) => {
            tracing::warn!("Failed to load history: {e}");
        }
    }
}

/// Persist a session to storage after it has been stopped.
///
/// Looks up the session by id in the session manager (it is still there
/// even though the engine cleared `active_session`).
pub(crate) fn persist_session(
    engine: &prxvc_core::engine::Engine,
    session_id: &prxvc_session::SessionId,
    storage: &prxvc_storage::StorageManager,
    shared: &SharedAppState,
) {
    if let Some(session) = engine.session_manager().get_session(session_id) {
        match storage.save_session(session, None) {
            Ok(()) => {
                tracing::info!("Session persisted: {}", session_id.0);
                // Also add to the UI history.
                if let Ok(record) = prxvc_storage::SessionRecord::from_session(session) {
                    shared.add_history_entry(HistoryEntry {
                        session_id: record.id.clone(),
                        title: record.summary_title(),
                        created_at: record.created_at.clone(),
                        duration: record.duration_display(),
                        state: record.state.clone(),
                    });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to persist session: {e}");
            }
        }
    }
}
