//! Call-mode routing: makes our virtual microphone the system default input for the length of a call and always puts the previous default back (pipeline-design section 4, decision D-P4).
//!
//! * [`controller`]: [`RoutingController`] enters and leaves call mode,
//!   recovers after a crash and reports changes while call mode is on.
//! * [`machine`]: the pure rules for those changes (state, change and a world
//!   snapshot in; new state and events out).
//! * [`recovery`]: what recovery at start does, decided from the journal and the default input.
//! * [`restore`]: which device the default input returns to.
//! * [`journal`]: the file that survives a crash (`<data_dir>/routing-journal.json`).
//! * [`events`], [`report`]: what is reported to the coordinator.
//! * [`system`]: the machine facts the controller reads besides the default input.
//! * [`changes`]: the bounded queue the native listeners post to.
//! * macOS only: [`core_audio`] (the [`SystemAudioRouting`](prxvc_ports::routing::SystemAudioRouting)
//!   implementation), [`hal_listener`] and [`power`] (native notifications).
//!
//! The pre-existing [`ProcessFilter`] lives in [`crate::process_filter`] and is
//! re-exported here so that the old paths keep working.

pub mod changes;
pub mod controller;
#[cfg(target_os = "macos")]
pub mod core_audio;
pub mod events;
#[cfg(target_os = "macos")]
pub mod hal_listener;
pub mod journal;
pub mod machine;
#[cfg(target_os = "macos")]
pub mod power;
pub mod recovery;
pub mod report;
pub mod restore;
pub mod system;
#[cfg(test)]
mod testkit;

pub use crate::process_filter::ProcessFilter;
pub use controller::{EnterRequest, RoutingController};
#[cfg(target_os = "macos")]
pub use core_audio::CoreAudioRouting;
pub use events::{DeviceRef, RoutingEvent};
pub use journal::{FileJournalStore, Journal, JournalRead, JournalStore};
pub use machine::Phase;
pub use recovery::RecoveryPlan;
pub use report::{CallModeError, EnterOutcome, LeaveOutcome, RecoveryReport, Restored};
pub use restore::RestoreReason;
pub use system::{OsSystem, SystemProbe};
