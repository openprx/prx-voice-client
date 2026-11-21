//! What both the application and the call harness need to run a translated call: configuration, models, providers, parameters and the trace sink. Free of any other module of the application so that the harness binary can include this tree as it is.
//!
//! * `config`: the configuration and directories, read fresh.
//! * `models`: the model store as the registry's source of models.
//! * `providers`: resolving, checking and building the providers of a call.
//! * `disclosure`: the spoken notice to the other party, from the configuration.
//! * `params`: `call.*` and `[pipeline]` as pipeline parameters.
//! * `problems`: registry problems as typed call issues.
//! * `trace`: the sink for stage timestamps.
//! * `route` (macOS): what the local output device is, and whether the echo gate applies.
//! * `readiness` (macOS): what a first translated call needs from the machine, as report lines.

#![allow(dead_code)] // the harness includes this tree and uses a different part of it

pub(crate) mod config;
pub(crate) mod disclosure;
pub(crate) mod models;
pub(crate) mod params;
pub(crate) mod problems;
pub(crate) mod providers;
#[cfg(target_os = "macos")]
pub(crate) mod readiness;
#[cfg(target_os = "macos")]
pub(crate) mod route;
pub(crate) mod trace;
