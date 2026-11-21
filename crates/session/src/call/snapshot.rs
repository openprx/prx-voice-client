//! The immutable snapshot the coordinator publishes after every change, and the commands that drive it (pipeline-design 7.2 and 7.3, data side).
//!
//! The snapshot is plain data with no behaviour: the engine maps it into the
//! UI's shared state. Commands are imperative and typed; the UI crate never
//! sees these types (the composition root maps both ways).

use std::sync::Arc;

use prxvc_media::{Direction, MonoTime};
use prxvc_ports::BundleId;

use super::error::CallIssue;
use super::host::{ActivityJudgement, PathStats};
use crate::call_state::CallState;
use crate::dir_set::DirSet;
use crate::direction_state::DirectionState;
use crate::output::{OutputCause, OutputSource};

/// What the local user hears of the call application (`call.local_output_mode`).
///
/// In translated calls the tap is muted while we are tapping, and this mode
/// decides what the incoming mixer plays when the direction machine says
/// "translation": the translation alone, the original lowered with the
/// translation on top, or the original alone (design 4.5). In an audio-only call
/// the mode is stored and shown and changes nothing audible: with the unmuted
/// tap the call application plays the original itself (see the crate docs of `call`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LocalOutputMode {
    /// The translation only.
    #[default]
    Translation,
    /// The original, lowered, with the translation on top.
    Both,
    /// The original only.
    Original,
}

/// A request to the coordinator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallCommand {
    /// Start call mode.
    Start {
        /// The call application; `None` uses the coordinator's configured default.
        target_app: Option<BundleId>,
        /// Translate (otherwise audio only, the M1 path).
        translate: bool,
    },
    /// Stop call mode.
    Stop,
    /// Pause these directions.
    Pause(DirSet),
    /// Resume every paused direction.
    Resume,
    /// Choose what the local user hears of the call application.
    SetLocalOutputMode(LocalOutputMode),
    /// Make our microphone the default input again.
    ReapplyVirtualMicrophone,
    /// Skip: cancel everything this direction has in flight and queued.
    Skip(Direction),
    /// Say the notice to the other party again.
    SayDisclosure,
}

/// One direction in the snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectionSnapshot {
    /// Which direction.
    pub direction: Direction,
    /// The direction machine's state.
    pub state: DirectionState,
    /// What the listener of this direction hears now: the remote party for
    /// outgoing, the local user for incoming. The direction machine's
    /// `ChangeOutput` is the only authority.
    pub hears: OutputSource,
    /// Counters and levels of the path; `None` while the path is not running.
    pub path: Option<PathStats>,
    /// The pipeline's own snapshot (captions, counters, backlog) while the direction translates.
    pub pipeline: Option<Arc<crate::pipeline::DirectionSnapshot>>,
    /// The language spoken and the language heard while the direction translates.
    pub languages: Option<(prxvc_media::Lang, prxvc_media::Lang)>,
}

/// The last change of what a listener hears (PD-4.3.1: never unannounced).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputChange {
    /// The listener's direction.
    pub direction: Direction,
    /// What was heard.
    pub from: OutputSource,
    /// What is heard now.
    pub to: OutputSource,
    /// Why.
    pub cause: OutputCause,
    /// When, on the coordinator's clock.
    pub at: MonoTime,
}

/// Routing facts for the UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutingSnapshot {
    /// Call mode is on: the default input was switched and not yet restored.
    pub in_call_mode: bool,
    /// The default input is our virtual microphone (false after a displacement).
    pub default_input_is_ours: bool,
    /// The default input that displaced ours.
    pub displaced_to: Option<String>,
    /// The default input before call mode.
    pub saved_input: Option<String>,
    /// What leaving call mode would restore.
    pub restore_target: Option<String>,
    /// The default input restored by the last stop.
    pub restored_to: Option<String>,
    /// The default output now, when it changed since start.
    pub default_output: Option<String>,
    /// The call application's activity.
    pub call_activity: ActivityJudgement,
}

/// The devices in use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceNames {
    /// The physical microphone captured.
    pub microphone: Option<String>,
    /// The device the outgoing path plays to (the virtual microphone's sink).
    pub virtual_sink: Option<String>,
    /// The call application tapped.
    pub call_app: Option<String>,
}

/// Everything the UI needs to show call mode.
#[derive(Clone, Debug, PartialEq)]
pub struct CallSnapshot {
    /// Counts publications; the first snapshot is 1.
    pub revision: u64,
    /// The call machine's state.
    pub state: CallState,
    /// When the state last changed.
    pub since: MonoTime,
    /// Translation was requested.
    pub translate_enabled: bool,
    /// What the local user hears of the call application (configured mode).
    pub local_output_mode: LocalOutputMode,
    /// Outgoing, then incoming.
    pub directions: [DirectionSnapshot; 2],
    /// Routing facts.
    pub routing: RoutingSnapshot,
    /// Devices in use.
    pub devices: DeviceNames,
    /// Conditions that hold now.
    pub issues: Vec<CallIssue>,
    /// The last failure, kept after it was resolved until the next start.
    pub last_error: Option<CallIssue>,
    /// The last change of what a listener hears.
    pub last_output_change: Option<OutputChange>,
    /// The spoken notice to the other party.
    pub disclosure: super::DisclosureSnapshot,
    /// A graceful stop is running: the last sentences are being played before call mode ends.
    /// The seconds left until the limit (`call.stop_grace_ms`), rounded up. A second Stop ends it
    /// at once. `None` when not stopping gracefully.
    pub finishing_s: Option<u32>,
}

impl CallSnapshot {
    /// The snapshot of one direction.
    pub fn direction(&self, d: Direction) -> &DirectionSnapshot {
        match d {
            Direction::Outgoing => &self.directions[0],
            Direction::Incoming => &self.directions[1],
        }
    }
}
