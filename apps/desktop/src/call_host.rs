//! The real call host and audio paths for the call coordinator (macOS): what the composition root plugs into `prxvc-session`'s `CallHost` seam.
//!
//! * `host`: [`RealCallHost`], the checks, consent, routing controller and activity probe.
//! * `paths`: the outgoing and incoming pumps behind `AudioPath`.
//! * `map`: typed mappings from the audio crate's errors and events to the coordinator's codes.

mod host;
mod map;
mod paths;

pub(crate) use host::{HostConfig, RealCallHost};
