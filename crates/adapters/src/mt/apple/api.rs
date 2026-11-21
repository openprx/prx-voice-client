//! The seam between the adapter and the operating system: two small traits, so that every rule of
//! the adapter is tested against a fake and the real implementation (`ffi`) stays a thin layer.

use std::sync::Arc;
use std::time::Duration;

use super::fault::SystemFault;

/// Translation strategy of a session (the framework's `preferredStrategy`; macOS 26.4 and later,
/// older systems use their default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Strategy {
    /// The framework's own choice.
    #[default]
    Default,
    /// Prefer speed.
    LowLatency,
    /// Prefer quality.
    HighFidelity,
}

/// Availability of a language pair on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Installed and ready.
    Installed,
    /// Supported, but the language pack is not downloaded.
    PackNotInstalled,
    /// Not supported by the framework.
    Unsupported,
}

/// The operating system's translation service.
pub trait SystemTranslationApi: Send + Sync {
    /// Whether this system can run the framework (macOS 26 or later).
    ///
    /// # Errors
    /// `OsTooOld`.
    fn os_support(&self) -> Result<(), SystemFault>;

    /// Whether the pack for `source` to `target` (BCP 47 tags) is installed. Blocks.
    ///
    /// # Errors
    /// The framework failed or did not answer.
    fn availability(&self, source: &str, target: &str) -> Result<Availability, SystemFault>;

    /// Open a session for an installed pair. Blocks.
    ///
    /// # Errors
    /// Any `SystemFaultKind`: old system, missing pack, unsupported pair, framework failure.
    fn open(
        &self,
        source: &str,
        target: &str,
        strategy: Strategy,
    ) -> Result<Arc<dyn SystemSession>, SystemFault>;
}

/// One session of a language pair. Calls on it are serialised by the adapter; `cancel` may be
/// called from any thread while a call is blocked.
pub trait SystemSession: Send + Sync {
    /// Load the pair's resources. Blocks.
    ///
    /// # Errors
    /// Timeout, cancellation or a framework failure.
    fn prepare(&self, timeout: Duration) -> Result<(), SystemFault>;

    /// Translate one text as a whole. Blocks until the text, the timeout or `cancel`.
    ///
    /// # Errors
    /// Timeout, cancellation or a framework failure.
    fn translate(&self, text: &str, timeout: Duration) -> Result<String, SystemFault>;

    /// Make the blocked call return `Cancelled`.
    fn cancel(&self);
}
