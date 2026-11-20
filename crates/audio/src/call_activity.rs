//! Call-activity detection: what a call application is doing in the audio system, and whether that means a call is active on our virtual microphone.
//!
//! Core Audio keeps a process object per application with whether its input and
//! output run and which devices it uses (macOS 14.4+). This module reads that
//! as plain data ([`AppAudioActivity`], from `native/prxvc_procs.m`, a port of
//! `bench/callapp/procaudio.c`), judges it per pipeline-design PD-4.1.2
//! ([`judge_call`]) and offers a polling watcher that emits debounced change
//! events for the call coordinator ([`CallActivityWatcher`]).
//!
//! * [`model`]: the plain data records and their merge.
//! * [`judge`]: the pure judgement, including the echo-reference rule.
//! * [`watcher`]: the source trait, the debouncer and the polling thread.
//! * `native` (macOS): the Core Audio reads.

pub mod judge;
pub mod model;
#[cfg(target_os = "macos")]
pub mod native;
pub mod watcher;

pub use judge::{CallJudgement, judge_call};
pub use model::{AppAudioActivity, ProcessDevice};
pub use watcher::{ActivityError, ActivitySource, CallActivityEvent, CallActivityWatcher, WatcherConfig};

#[cfg(test)]
mod tests;
