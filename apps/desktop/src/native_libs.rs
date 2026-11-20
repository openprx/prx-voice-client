//! Which native runtime libraries this build links and whether they load.
//!
//! Only the `--check` report uses this (see `cli`): it proves, from inside an application bundle,
//! that each library is found through `@rpath` and answers a call. Nothing is loaded for synthesis
//! here.

/// One report line per native runtime this build was compiled with, or one line saying there is none.
pub(crate) fn report_lines() -> Vec<String> {
    #[allow(unused_mut)]
    let mut lines: Vec<String> = Vec::new();
    #[cfg(feature = "qwentts")]
    lines.push(format!(
        "native:      qwentts {}",
        prxvc_adapters::tts::qwentts::engine::runtime_version()
    ));
    #[cfg(feature = "transcribe")]
    lines.push(format!(
        "native:      transcribe {}",
        prxvc_adapters::transcribe::runtime_version()
    ));
    if lines.is_empty() {
        lines.push("native:      none (built without native features)".into());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_build_reports_at_least_one_line() {
        let lines = report_lines();
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|l| l.starts_with("native:")));
    }
}
