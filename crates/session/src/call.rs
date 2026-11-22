//! The call coordinator (layer L2): the single writer of the call machine and both direction machines, and the typed vocabulary around it.
//!
//! # Structure
//!
//! * [`Coordinator`] (`coordinator`, with `start`, `actions`, `directions`,
//!   `events`): no thread of its own; `handle(command)` and `tick()` do the
//!   work. It holds the call machine, one direction machine per direction, the
//!   two audio paths and the issues, and publishes a [`CallSnapshot`] to its
//!   sink after every change.
//! * [`CallHandle`] (`runner`): the one thread that owns a coordinator,
//!   a bounded command channel in, a bounded join out.
//! * [`CallHost`] and [`AudioPath`] (`host`): the seams to the machine it runs
//!   on. The composition root implements them over the real endpoints and the
//!   routing controller; the coordinator names no adapter.
//! * [`CallSnapshot`], [`CallCommand`] (`snapshot`): what the UI reads and sends.
//! * [`CallErrorCode`], [`CallIssue`], [`HostFault`] (`error`): the typed errors.
//! * [`MicOwnership`] (`ownership`): call mode and an assistant session never
//!   share the microphone path.
//!
//! # Threads
//!
//! The coordinator thread is the only writer of call state. Each path is a
//! pump on its own non-real-time thread (in `prxvc-audio`, behind
//! [`AudioPath`]); device callbacks run on real-time threads of the endpoints
//! and never touch the coordinator. The UI thread sends commands and copies
//! snapshots.
//!
//! # Milestone M1 (audio only)
//!
//! Both directions are passthrough: the call goes `Idle`, `Initializing`,
//! `AudioReady` and stays there. The outgoing path plays the physical
//! microphone into the virtual microphone. The incoming path **captures and
//! meters the tap but plays nothing**: the call application already plays the
//! remote party on the user's output, so the tap stays unmuted and the user
//! hears the original from the application itself ("original only"). Playing the
//! tapped audio as well would double it. A muted tap is never enabled here.
//! Translation (providers, stage workers) arrives with CT-13 and later; the
//! coordinator already executes every machine action, with the stage workers
//! replaced by "ready at once" (see `directions`).

mod actions;
mod coordinator;
mod directions;
mod disclosure;
mod error;
mod events;
mod finishing;
mod host;
mod ownership;
mod runner;
mod snapshot;
mod start;
mod summary;
mod translation;

#[cfg(test)]
mod tests;

pub use coordinator::{CallSettings, Coordinator, CoordinatorDeps, SnapshotSink};
pub use disclosure::{DisclosureSnapshot, DisclosureSpec, DisclosureState};
pub use error::{CallErrorCode, CallIssue, HostFault, ProviderIssue, Severity};
pub use finishing::direction_in_flight;
pub use host::{
    ActivityJudgement, AudioPath, CallHost, DirectionBuild, EnteredRouting, LeftRouting,
    OutputRoute, PathEvent, PathLink, PathStats, PlaybackReport, Playout, PreparedProviders,
    ProvidersFailure, RoutingNotice,
};
pub use ownership::{MicBusy, MicClaim, MicOwner, MicOwnership};
pub use runner::{CallHandle, SendError, ShutdownTimeout, TICK_INTERVAL};
pub use snapshot::{
    CallCommand, CallSnapshot, DeviceNames, DirectionSnapshot, LocalOutputMode, OutputChange,
    RoutingSnapshot,
};
