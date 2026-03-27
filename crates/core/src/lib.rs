//! Application-level shared entry point for the prx-voice-client system.
//!
//! Provides unified initialization via [`AppContext`], lifecycle state via
//! [`AppState`], and the top-level [`PrxvcError`] boundary that all subsystem
//! errors can be mapped into.

pub mod bus;
pub mod engine;
pub mod runtime;

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Unified error boundary
// ---------------------------------------------------------------------------

/// Top-level error type that unifies all subsystem errors.
#[derive(Debug, thiserror::Error)]
pub enum PrxvcError {
    /// Configuration loading or validation failure.
    #[error("config error: {0}")]
    Config(String),

    /// Session lifecycle or state-machine error.
    #[error("session error: {0}")]
    Session(String),

    /// Audio subsystem error (device, codec, pipeline).
    #[error("audio error: {0}")]
    Audio(String),

    /// Protocol serialization or contract mismatch.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Persistent storage error.
    #[error("storage error: {0}")]
    Storage(String),

    /// Security or credential error.
    #[error("security error: {0}")]
    Security(String),

    /// FFI boundary error.
    #[error("ffi error: {0}")]
    Ffi(String),

    /// Catch-all for unexpected internal failures.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Alias for results using [`PrxvcError`].
pub type Result<T> = std::result::Result<T, PrxvcError>;

// ---------------------------------------------------------------------------
// Application context
// ---------------------------------------------------------------------------

/// Runtime context shared across subsystems.
///
/// Created once during application startup and threaded through the system
/// to provide paths and debug configuration.
#[derive(Debug, Clone)]
pub struct AppContext {
    /// Path to the configuration file or directory.
    pub config_path: PathBuf,
    /// Path to the persistent data directory.
    pub data_path: PathBuf,
    /// Whether the application is running in debug mode.
    pub is_debug: bool,
}

// ---------------------------------------------------------------------------
// Application lifecycle state
// ---------------------------------------------------------------------------

/// High-level application lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    /// Application is initializing subsystems.
    Booting,
    /// All subsystems initialized; ready for user interaction.
    Ready,
    /// Application is in the background with minimal resource usage.
    BackgroundIdle,
    /// Application is in the foreground but idle.
    ForegroundIdle,
    /// A required OS permission (e.g. microphone) is not granted.
    PermissionBlocked,
    /// Recovering from a transient error.
    Recovering,
    /// An unrecoverable error occurred; the application cannot continue.
    FatalError,
}

impl AppState {
    /// Returns the i18n translation key for this state.
    ///
    /// The core crate does not depend on i18n — it only returns key strings.
    /// The UI layer is responsible for calling `t()` on the returned key.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            Self::Booting => "state.starting",
            Self::Ready => "state.idle",
            Self::BackgroundIdle => "state.idle",
            Self::ForegroundIdle => "state.idle",
            Self::PermissionBlocked => "error.permission_denied",
            Self::Recovering => "state.recovering",
            Self::FatalError => "state.failed",
        }
    }
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for downstream crates.
pub mod prelude {
    pub use crate::bus::{
        CommandReceiver, CommandSender, EventBroadcaster, EventSubscriber, command_channel,
        event_channel,
    };
    pub use crate::engine::Engine;
    pub use crate::runtime::AppRuntime;
    pub use crate::{AppContext, AppState, PrxvcError, Result};
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_i18n_keys() {
        let all_states = [
            AppState::Booting,
            AppState::Ready,
            AppState::BackgroundIdle,
            AppState::ForegroundIdle,
            AppState::PermissionBlocked,
            AppState::Recovering,
            AppState::FatalError,
        ];

        for state in &all_states {
            let key = state.i18n_key();
            assert!(!key.is_empty(), "i18n key for {state:?} must not be empty");
            assert!(
                key.starts_with("state.") || key.starts_with("error."),
                "i18n key for {state:?} must start with \"state.\" or \"error.\", got \"{key}\""
            );
        }
    }
}
