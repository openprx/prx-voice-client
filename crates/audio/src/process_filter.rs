//! Which processes a per-application capture follows.
//!
//! The capture endpoints themselves implement the `AudioSource` port of
//! `prxvc-ports`; this type only names what the process tap should listen to.
//! macOS implementation: [`crate::core_audio_tap::CoreAudioTapSource`].
//! (Formerly `routing`; [`crate::routing`] re-exports [`ProcessFilter`] for the existing paths.)

/// Which process(es) a per-application capture should follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessFilter {
    /// Capture only the audio rendered by this process id (the meeting app).
    IncludeApp(i32),
    /// Capture only the audio rendered by the application with this bundle ID;
    /// the capture follows the application across restarts.
    IncludeBundle(String),
    // Future: ExcludeSelf — capture everything except our own process, using
    // CATapDescription's exclude mode / WASAPI EXCLUDE_TARGET_PROCESS_TREE.
}
