//! Persistence helpers: the History window's list from the store at start.

use prxvc_desktop_ui::{HistoryEntry, SharedAppState};

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
