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

/// The port of the server a managed provider talks to: `Some` when `provider` is the built-in
/// `local-llama-server` and `base_url` is on this computer. Such a server is started and stopped
/// by the application; any other endpoint is only probed.
pub(crate) fn manages(provider: &str, base_url: &str) -> Option<u16> {
    if provider != prxvc_providers::DEFAULT_MT_PROVIDER {
        return None;
    }
    let url = url::Url::parse(base_url).ok()?;
    let local = matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    );
    if local {
        url.port_or_known_default()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_built_in_local_server_is_managed() {
        assert_eq!(
            manages("local-llama-server", "http://127.0.0.1:8080/v1"),
            Some(8080)
        );
        assert_eq!(
            manages("local-llama-server", "http://localhost:9000/v1"),
            Some(9000)
        );
        assert_eq!(
            manages("local-llama-server", "https://api.example.com/v1"),
            None
        );
        assert_eq!(manages("my-openai", "http://127.0.0.1:8080/v1"), None);
        assert_eq!(manages("local-llama-server", "not a url"), None);
    }
}
