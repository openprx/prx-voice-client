//! A process-wide panic hook that reports every thread panic through tracing.
//!
//! A panic on a worker thread (a pump, a pipeline stage) is otherwise visible only as "the thread
//! panicked" when it is joined, with the message printed to a standard error nobody reads in a
//! bundled application. The hook logs the thread name, the source location and the message, then
//! calls the hook that was installed before it (the default one prints to standard error).

use std::panic::PanicHookInfo;

/// What a panic carries, as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicReport {
    /// Name of the panicking thread (`<unnamed>` when it has none).
    pub thread: String,
    /// `file:line:column`, or `<unknown>`.
    pub location: String,
    /// The panic message, or `<non-text payload>`.
    pub message: String,
}

/// The report of a panic.
pub fn describe(info: &PanicHookInfo<'_>) -> PanicReport {
    let thread = std::thread::current()
        .name()
        .map_or_else(|| "<unnamed>".to_owned(), str::to_owned);
    let location = info.location().map_or_else(
        || "<unknown>".to_owned(),
        |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
    );
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-text payload>".to_owned());
    PanicReport {
        thread,
        location,
        message,
    }
}

/// Install the hook. Call once, after logging is set up. Calling it again chains one more layer,
/// which only repeats the log line.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = describe(info);
        tracing::error!(
            thread = %report.thread,
            location = %report.location,
            message = %report.message,
            "thread panicked"
        );
        previous(info);
    }));
}
