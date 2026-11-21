//! The seam to the managed local model server. The application installs a hook that starts the
//! server for a consumer and waits for it; a harness or a test installs none, and the endpoint is
//! then only probed (nothing is started, nothing falls back). Keeps this tree free of the
//! application's process management.

use std::sync::OnceLock;

/// What a consumer holds while it uses the server; dropping it releases the server.
pub(crate) type Hold = Box<dyn Send>;

/// Starts the server for the consumer of `provider` at `base_url` and waits until it is ready.
/// `Ok(None)` when that provider is not a managed one; `Err` is the manager's sentence.
pub(crate) type HoldFn = fn(&str, &str) -> Result<Option<Hold>, String>;

static HOOK: OnceLock<HoldFn> = OnceLock::new();

/// Install the hook (once per process).
pub(crate) fn install(hook: HoldFn) {
    let _ = HOOK.set(hook);
}

/// Hold the server for a consumer, when a hook is installed and the provider is managed.
///
/// # Errors
/// The typed reason the server cannot be provided, as a sentence.
pub(crate) fn hold(provider: &str, base_url: &str) -> Result<Option<Hold>, String> {
    match HOOK.get() {
        Some(hook) => hook(provider, base_url),
        None => Ok(None),
    }
}
