//! Diagnostics export: builds the diagnostics bundle and writes it to the temp directory.

use prxvc_desktop_ui::{EventSnapshot, SharedAppState};

use crate::event_map::push_error;

/// Build and export a diagnostics JSON bundle to the system temp directory.
pub(crate) fn export_diagnostics(shared: &SharedAppState) {
    let bundle = prxvc_observe::DiagnosticsBundle {
        app_version: "0.1.0".into(),
        os_info: std::env::consts::OS.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        log_excerpt: Vec::new(),
        config_snapshot: None,
        error_summary: Vec::new(),
        metrics: Some(prxvc_observe::metrics::snapshot()),
        transport_state: None,
        active_session_state: None,
        system_info: Some(prxvc_observe::SystemInfo::default()),
    };

    let export_dir = std::env::temp_dir().join("prx-voice-client");
    let _ = std::fs::create_dir_all(&export_dir);
    let export_path = export_dir.join(format!(
        "diagnostics-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));

    match prxvc_observe::export_diagnostics(&bundle, &export_path) {
        Ok(path) => {
            shared.push_event(EventSnapshot {
                event_type: "DiagnosticsExported".into(),
                detail: format!("Exported to: {}", path.display()),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        Err(e) => {
            push_error(shared, &format!("Export failed: {e}"));
        }
    }
}
