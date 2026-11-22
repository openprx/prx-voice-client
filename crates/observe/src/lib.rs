//! Observability utilities for the prx-voice-client system.
//!
//! Provides tracing initialization, diagnostics bundling, structured metrics,
//! span conventions, and export functionality for debugging and support
//! workflows.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

pub mod metrics;
pub mod panic_report;
pub mod spans;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors originating from the observability subsystem.
#[derive(Debug, thiserror::Error)]
pub enum ObserveError {
    /// Failed to initialize the tracing/logging subsystem.
    #[error("tracing initialization error: {0}")]
    InitError(String),

    /// Failed to export diagnostics data.
    #[error("diagnostics export error: {0}")]
    ExportError(String),

    /// An underlying I/O operation failed.
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the observability subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserveConfig {
    /// Tracing filter directive (e.g. `"info"`, `"debug"`, `"prxvc=trace"`).
    pub log_level: String,
    /// Whether to additionally write logs to a file.
    pub log_to_file: bool,
    /// Directory for log files. Required when `log_to_file` is `true`.
    pub log_dir: Option<PathBuf>,
    /// Emit log records in JSON format instead of human-readable text.
    pub json_format: bool,
    /// Redact potentially sensitive fields in log output.
    pub sanitize_sensitive: bool,
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_owned(),
            log_to_file: false,
            log_dir: None,
            json_format: false,
            sanitize_sensitive: true,
        }
    }
}

// ---------------------------------------------------------------------------
// System info
// ---------------------------------------------------------------------------

/// System environment information included in diagnostics bundles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Operating system name (e.g. `"macos"`, `"linux"`, `"windows"`).
    pub os: String,
    /// CPU architecture (e.g. `"aarch64"`, `"x86_64"`).
    pub arch: String,
    /// Rust toolchain version used to compile the binary.
    pub rust_version: String,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            rust_version: "unknown".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics bundle
// ---------------------------------------------------------------------------

/// A snapshot of application state collected for debugging or support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsBundle {
    /// Application version string.
    pub app_version: String,
    /// Operating system description.
    pub os_info: String,
    /// ISO-8601 timestamp when the bundle was created.
    pub created_at: String,
    /// Recent log lines relevant to the issue.
    pub log_excerpt: Vec<String>,
    /// Serialized configuration snapshot (optional).
    pub config_snapshot: Option<String>,
    /// Summary of recent errors.
    pub error_summary: Vec<String>,
    /// Point-in-time snapshot of application metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<metrics::AppMetrics>,
    /// Current transport layer state description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_state: Option<String>,
    /// Current active session state description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_state: Option<String>,
    /// System environment information.
    #[serde(default)]
    pub system_info: Option<SystemInfo>,
}

// ---------------------------------------------------------------------------
// Diagnostics bundle builder
// ---------------------------------------------------------------------------

/// Ergonomic builder for constructing a [`DiagnosticsBundle`].
///
/// # Example
///
/// ```
/// use prxvc_observe::DiagnosticsBundleBuilder;
///
/// let bundle = DiagnosticsBundleBuilder::new("0.1.0")
///     .with_metrics()
///     .with_transport_state("connected")
///     .build();
/// ```
pub struct DiagnosticsBundleBuilder {
    bundle: DiagnosticsBundle,
}

impl DiagnosticsBundleBuilder {
    /// Create a new builder pre-populated with the given application version
    /// and sensible defaults for all other fields.
    pub fn new(app_version: &str) -> Self {
        Self {
            bundle: DiagnosticsBundle {
                app_version: app_version.to_owned(),
                os_info: std::env::consts::OS.to_owned(),
                created_at: chrono::Utc::now().to_rfc3339(),
                log_excerpt: Vec::new(),
                config_snapshot: None,
                error_summary: Vec::new(),
                metrics: None,
                transport_state: None,
                active_session_state: None,
                system_info: Some(SystemInfo::default()),
            },
        }
    }

    /// Attach a point-in-time metrics snapshot.
    pub fn with_metrics(self) -> Self {
        self.with_metrics_from(metrics::global())
    }

    /// Attach a point-in-time snapshot taken from the given registry.
    pub fn with_metrics_from(mut self, registry: &metrics::Metrics) -> Self {
        self.bundle.metrics = Some(registry.snapshot());
        self
    }

    /// Record the current transport layer state.
    pub fn with_transport_state(mut self, state: &str) -> Self {
        self.bundle.transport_state = Some(state.to_owned());
        self
    }

    /// Record the current active session state.
    pub fn with_active_session(mut self, state: &str) -> Self {
        self.bundle.active_session_state = Some(state.to_owned());
        self
    }

    /// Include recent log lines in the bundle.
    pub fn with_log_excerpt(mut self, lines: Vec<String>) -> Self {
        self.bundle.log_excerpt = lines;
        self
    }

    /// Include a serialized configuration snapshot.
    pub fn with_config(mut self, config_json: &str) -> Self {
        self.bundle.config_snapshot = Some(config_json.to_owned());
        self
    }

    /// Include error summaries.
    pub fn with_errors(mut self, errors: Vec<String>) -> Self {
        self.bundle.error_summary = errors;
        self
    }

    /// Consume the builder and return the finished [`DiagnosticsBundle`].
    pub fn build(self) -> DiagnosticsBundle {
        self.bundle
    }
}

// ---------------------------------------------------------------------------
// Tracing initialization
// ---------------------------------------------------------------------------

/// Initialize the global tracing subscriber based on [`ObserveConfig`].
///
/// This should be called once during application startup. Subsequent calls
/// will return an error because the global subscriber can only be set once.
pub fn init_tracing(config: &ObserveConfig) -> Result<(), ObserveError> {
    let filter = EnvFilter::try_new(&config.log_level).map_err(|e| {
        ObserveError::InitError(format!("invalid log level '{}': {e}", config.log_level))
    })?;

    if config.json_format {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(true)
            .finish();

        tracing::subscriber::set_global_default(subscriber).map_err(|e| {
            ObserveError::InitError(format!("failed to set global subscriber: {e}"))
        })?;
    } else {
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(true)
            .finish();

        tracing::subscriber::set_global_default(subscriber).map_err(|e| {
            ObserveError::InitError(format!("failed to set global subscriber: {e}"))
        })?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Diagnostics export
// ---------------------------------------------------------------------------

/// Serialize a [`DiagnosticsBundle`] as JSON and write it to `output_path`.
///
/// Returns the canonical path of the written file on success.
pub fn export_diagnostics(
    bundle: &DiagnosticsBundle,
    output_path: &Path,
) -> Result<PathBuf, ObserveError> {
    let json = serde_json::to_string_pretty(bundle)
        .map_err(|e| ObserveError::ExportError(format!("serialization failed: {e}")))?;

    if let Some(parent) = output_path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(output_path, json.as_bytes())?;

    // Return the path as provided; canonicalize only if possible.
    let result = output_path
        .canonicalize()
        .unwrap_or_else(|_| output_path.to_path_buf());
    Ok(result)
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for downstream crates.
pub mod prelude {
    pub use crate::metrics::{
        self, AppMetrics, Metrics, inc_audio_frames_in, inc_audio_frames_out, inc_errors,
        inc_reconnect_attempts, inc_reconnect_successes, inc_sessions_completed,
        inc_sessions_created, inc_sessions_failed, inc_total_interrupts, inc_total_turns, reset,
        snapshot,
    };
    pub use crate::spans::{field_names, span_names};
    pub use crate::{
        DiagnosticsBundle, DiagnosticsBundleBuilder, ObserveConfig, ObserveError, SystemInfo,
        export_diagnostics, init_tracing,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Create a unique temp directory path.
    fn temp_dir_unique(prefix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{ts}"))
    }

    /// Clean up a path (best-effort).
    fn cleanup(path: &Path) {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Create a sample diagnostics bundle for testing.
    fn sample_bundle() -> DiagnosticsBundle {
        DiagnosticsBundle {
            app_version: "0.1.0-test".to_owned(),
            os_info: "TestOS 1.0".to_owned(),
            created_at: "2026-03-27T12:00:00Z".to_owned(),
            log_excerpt: vec!["line 1".to_owned(), "line 2".to_owned()],
            config_snapshot: Some(r#"{"key":"value"}"#.to_owned()),
            error_summary: vec!["error A".to_owned()],
            metrics: None,
            transport_state: None,
            active_session_state: None,
            system_info: None,
        }
    }

    #[test]
    fn test_observe_config_defaults() {
        let cfg = ObserveConfig::default();
        assert_eq!(cfg.log_level, "info");
        assert!(!cfg.log_to_file);
        assert!(cfg.log_dir.is_none());
        assert!(!cfg.json_format);
        assert!(cfg.sanitize_sensitive);
    }

    #[test]
    fn test_diagnostics_bundle_serialization() {
        let bundle = sample_bundle();
        let json = serde_json::to_string(&bundle).expect("serialize failed");

        assert!(json.contains("app_version"));
        assert!(json.contains("0.1.0-test"));
        assert!(json.contains("os_info"));
        assert!(json.contains("TestOS 1.0"));
        assert!(json.contains("created_at"));
        assert!(json.contains("log_excerpt"));
        assert!(json.contains("line 1"));
        assert!(json.contains("config_snapshot"));
        assert!(json.contains("error_summary"));
        assert!(json.contains("error A"));

        // New optional fields should not appear when None.
        assert!(!json.contains("metrics"));
        assert!(!json.contains("transport_state"));
        assert!(!json.contains("active_session_state"));

        // Verify round-trip
        let restored: DiagnosticsBundle = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(restored.app_version, "0.1.0-test");
        assert_eq!(restored.log_excerpt.len(), 2);
        assert_eq!(restored.error_summary.len(), 1);
        assert!(restored.metrics.is_none());
        assert!(restored.transport_state.is_none());
        assert!(restored.active_session_state.is_none());
    }

    #[test]
    fn test_export_diagnostics_writes_file() {
        let dir = temp_dir_unique("prxvc_obs_export");
        std::fs::create_dir_all(&dir).expect("create dir");
        let file_path = dir.join("diag.json");

        let bundle = sample_bundle();
        let result = export_diagnostics(&bundle, &file_path);
        assert!(result.is_ok());

        // Verify the file exists and contains valid JSON
        assert!(file_path.exists());
        let content = std::fs::read_to_string(&file_path).expect("read failed");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("invalid JSON");
        assert_eq!(parsed["app_version"], "0.1.0-test");
        assert_eq!(parsed["os_info"], "TestOS 1.0");

        cleanup(&dir);
    }

    #[test]
    fn test_export_diagnostics_creates_parent_dirs() {
        let dir = temp_dir_unique("prxvc_obs_nested");
        let nested_path = dir.join("a").join("b").join("c").join("diag.json");

        // The nested dirs should not exist yet
        assert!(!dir.exists());

        let bundle = sample_bundle();
        let result = export_diagnostics(&bundle, &nested_path);
        assert!(result.is_ok());
        assert!(nested_path.exists());

        let content = std::fs::read_to_string(&nested_path).expect("read failed");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("invalid JSON");
        assert_eq!(parsed["app_version"], "0.1.0-test");

        cleanup(&dir);
    }

    #[test]
    fn test_diagnostics_builder() {
        let bundle = DiagnosticsBundleBuilder::new("1.2.3")
            .with_transport_state("connected")
            .with_active_session("Listening")
            .with_log_excerpt(vec!["log line 1".to_owned()])
            .with_config(r#"{"key":"val"}"#)
            .with_errors(vec!["oops".to_owned()])
            .build();

        assert_eq!(bundle.app_version, "1.2.3");
        assert_eq!(bundle.transport_state.as_deref(), Some("connected"));
        assert_eq!(bundle.active_session_state.as_deref(), Some("Listening"));
        assert_eq!(bundle.log_excerpt.len(), 1);
        assert!(bundle.config_snapshot.is_some());
        assert_eq!(bundle.error_summary.len(), 1);
        assert!(bundle.metrics.is_none());
        assert!(bundle.system_info.is_some());

        let sys = bundle.system_info.as_ref().expect("system_info missing");
        assert!(!sys.os.is_empty());
        assert!(!sys.arch.is_empty());
    }

    #[test]
    fn test_diagnostics_builder_with_metrics() {
        let registry = metrics::Metrics::new();
        registry.inc_sessions_created();
        registry.inc_errors(true);

        let bundle = DiagnosticsBundleBuilder::new("0.0.1")
            .with_metrics_from(&registry)
            .build();

        let m = bundle.metrics.as_ref().expect("metrics missing");
        assert_eq!(m.sessions_created, 1);
        assert_eq!(m.errors_recoverable, 1);
    }

    #[test]
    fn test_backward_compat_deserialization() {
        // Simulate JSON from the old format (without new fields).
        let old_json = r#"{
            "app_version": "0.0.1",
            "os_info": "linux",
            "created_at": "2026-01-01T00:00:00Z",
            "log_excerpt": [],
            "config_snapshot": null,
            "error_summary": []
        }"#;

        let bundle: DiagnosticsBundle =
            serde_json::from_str(old_json).expect("old format should deserialize");
        assert_eq!(bundle.app_version, "0.0.1");
        assert!(bundle.metrics.is_none());
        assert!(bundle.transport_state.is_none());
        assert!(bundle.active_session_state.is_none());
        assert!(bundle.system_info.is_none());
    }
}
