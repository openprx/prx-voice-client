//! Running a report that asks the operating system's translation service from a command-line
//! tool: the framework needs the main thread to run its run loop, so the work goes to a second
//! thread. Without the `apple-translation` feature the work simply runs here.

/// Run `work` where the translation framework can answer.
pub(crate) fn with_main_loop<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    #[cfg(feature = "apple-translation")]
    {
        prxvc_adapters::mt::apple::ffi::run_with_main_loop(work)
    }
    #[cfg(not(feature = "apple-translation"))]
    {
        work()
    }
}
