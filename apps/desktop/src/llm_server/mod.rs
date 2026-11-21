//! The managed local model server: the application owns the lifecycle of `llama-server` (the
//! built-in `local-llama-server` provider) for the language-model translation engine and the
//! assistant's dialogue (application layer: process management never runs in a device callback or
//! a pure crate).
//!
//! * `core`: the state machine and its rules (demand, adoption, stop, restart).
//! * `handle`: the worker thread, leases and blocking waits.
//! * `ports`, `real`: the operating-system seam and its real implementations.
//! * `record`: the record of the process we started and the sweep of a left-over one.
//! * `locate`, `problem`: finding the binary and the typed reasons for failure.
//!
//! Consumers: selecting the language-model engine pins the server (`set_pinned`); the assistant
//! (and a call that uses the language model while the pin is off) take a lease; with neither the
//! server stops, at once after the pin goes and after `llm_server.idle_stop_s` after the last
//! lease.

mod boot;
mod core;
mod handle;
#[cfg(test)]
mod live_tests;
mod ports;
mod problem;
mod real;
mod record;
#[cfg(test)]
mod tests;

pub(crate) use self::core::{ServerState, Snapshot};
pub(crate) use boot::{global, install, manages, shutdown};
pub(crate) use handle::ServerLease;
pub(crate) use problem::ServerProblem;

use std::time::Duration;

/// How long a consumer waits for the server: longer than its own start timeout, so that the
/// server's typed problem arrives first.
const READY_WAIT: Duration = Duration::from_secs(130);

/// Hold the managed server for the consumer of `provider` at `base_url` and wait until it is
/// ready. `Ok(None)` when that provider is not the managed one (a remote or other endpoint) or
/// when no manager is installed (tests, harnesses): nothing is started, the endpoint is only
/// probed as before.
///
/// # Errors
/// The typed reason the server cannot be provided.
pub(crate) fn hold_for(
    provider: &str,
    base_url: &str,
) -> Result<Option<ServerLease>, ServerProblem> {
    if manages(provider, base_url).is_none() {
        return Ok(None);
    }
    let Some(server) = global() else {
        return Ok(None);
    };
    let lease = server.acquire();
    lease.wait_ready(READY_WAIT)?;
    Ok(Some(lease))
}

/// The configured engine changed: the language model keeps the server running, system
/// translation lets it go (at once, unless another consumer still holds it).
pub(crate) fn on_engine_changed(engine: &str) {
    if let Some(server) = global() {
        server.set_pinned(
            prxvc_config::TranslationEngine::parse(engine)
                == Some(prxvc_config::TranslationEngine::Llm),
        );
    }
}
