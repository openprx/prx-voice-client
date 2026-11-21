//! The voice assistant in the application (work package CT-20): the host that connects the orb to the assistant's loop in `prxvc-session`.
//!
//! * `host`: commands in, events out, the session record, the microphone claim shared with call mode.
//! * `launch`: starting a real session (and the `Launcher` seam the tests replace).
//! * `kit`: preparing the providers from the registry, with every problem reported as an issue.
//! * `issues`: typed problems and faults as the issues the orb stage shows.
//! * `params`, `audio`: the configuration as session parameters; the microphone, output and gate.
//! * `run`: the host's thread.
//!
//! The assistant uses the speech models of call translation through the same registry and has no
//! backend of its own beyond the dialogue role; no stand-in is ever built.

pub(crate) mod audio;
pub(crate) mod host;
pub(crate) mod issues;
pub(crate) mod kit;
pub(crate) mod launch;
pub(crate) mod params;
pub(crate) mod run;
pub(crate) mod turn_log;
pub(crate) mod warm;

#[cfg(test)]
mod tests;
