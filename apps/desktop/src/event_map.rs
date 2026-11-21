//! The debug log: a line the debug window shows for something that went wrong.

use prxvc_desktop_ui::{EventSnapshot, SharedAppState};

/// Push an error line to the shared state for the debug window.
pub(crate) fn push_error(shared: &SharedAppState, msg: &str) {
    shared.push_event(EventSnapshot {
        event_type: "Error".into(),
        detail: msg.to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}
