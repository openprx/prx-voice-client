//! What the settings window asks the app to do besides changing a value.
//!
//! Value changes travel as edits of [`UiSettings`](crate::UiSettings) that the app diffs and
//! writes through the configuration's atomic update. Everything else (probe again, ask for a
//! permission, open a System Settings pane, run a driver operation, show a folder) is a typed
//! command, collected during the frame and applied by the app after drawing (CS-2.6).

/// A pane of the operating system's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPane {
    /// Privacy: microphone.
    Microphone,
    /// Privacy: system audio recording.
    AudioCapture,
    /// Where the languages of system translation are downloaded (General, Language & Region,
    /// Translation Languages).
    TranslationLanguages,
}

/// A folder or file the app can show in the system's file manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealTarget {
    /// The data directory.
    DataDir,
    /// The model store.
    ModelsDir,
    /// The log file (or the log directory when there is none).
    LogFile,
    /// The configuration file.
    ConfigFile,
    /// The file with the third-party notices.
    Notices,
}

/// A request from the settings window to the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsCommand {
    /// Probe devices, permissions, the driver, the pipeline and the voice profile again.
    Refresh,
    /// Show the system prompt for the microphone (only when it was not asked yet).
    RequestMicrophone,
    /// Create a short capture so the system asks for audio-capture consent.
    RequestCallAudio,
    /// Open a pane of System Settings.
    OpenSystemPane(SystemPane),
    /// Install the virtual microphone driver, or replace it with the bundled one.
    InstallDriver,
    /// Remove the virtual microphone driver.
    UninstallDriver,
    /// Show a folder or file in the file manager.
    Reveal(RevealTarget),
    /// Write a diagnostics bundle.
    ExportDiagnostics,
    /// Try to start the local model server again after it failed.
    RetryModelServer,
}
