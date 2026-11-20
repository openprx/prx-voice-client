//! Command-line flags that run without a window.
//!
//! `--version` prints the version. `--check` resolves the configuration and
//! data directories, reports where the process runs from and exits. Neither
//! opens a window, creates a directory, reads credentials or touches an audio
//! device, so both are safe to run from inside an application bundle in a
//! script. `--check-call` adds what a translated call needs from this machine
//! (consents as this process sees them, the driver, the audio devices, the
//! models, the voices, the translation server); it reads the settings and the
//! device list but changes nothing and raises no permission prompt.

use std::path::Path;

use crate::bundle::{self, BundleStatus};

/// What the command line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Gui,
    Version,
    Check,
    CheckCall,
    /// A `--flag` this version does not know: report it and exit instead of opening the window,
    /// so that a script written for a newer version cannot start the application by accident.
    Unknown,
}

/// Choose the mode from the arguments after the program name. The first
/// recognised flag wins; anything else starts the GUI.
pub(crate) fn parse(args: &[String]) -> Mode {
    for arg in args {
        match arg.as_str() {
            "--version" | "-V" => return Mode::Version,
            "--check" => return Mode::Check,
            "--check-call" => return Mode::CheckCall,
            other if other.starts_with("--") => return Mode::Unknown,
            _ => {}
        }
    }
    Mode::Gui
}

/// The one-line version string.
pub(crate) fn version_line() -> String {
    format!("prxvc-desktop {}", env!("CARGO_PKG_VERSION"))
}

fn exists(p: &Path) -> &'static str {
    if p.exists() { "exists" } else { "missing" }
}

/// The `--check` report. Settings are not read, so `models.dir` overrides do
/// not show; the paths are those of a default configuration.
pub(crate) fn check_report(status: &BundleStatus) -> String {
    let mut out = vec![version_line()];
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("unknown ({e})"));
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("unknown ({e})"));
    out.push(format!("executable:  {exe}"));
    out.push(format!("working dir: {cwd}"));
    out.extend(crate::native_libs::report_lines());
    match status {
        BundleStatus::Bundled {
            identifier,
            app_dir,
        } => {
            out.push(format!("bundle:      {} ({identifier})", app_dir.display()));
            if let Some(res) = bundle::resources_dir(status) {
                out.push(format!("resources:   {} ({})", res.display(), exists(&res)));
            }
        }
        BundleStatus::Unbundled => {
            out.push("bundle:      none (running outside an application bundle)".into())
        }
    }
    out.push(format!(
        "expected bundle identifier: {}",
        prxvc_config::paths::bundle_identifier()
    ));
    match prxvc_config::paths::AppPaths::resolve(&prxvc_config::AppConfig::default()) {
        Ok(p) => {
            let config_file = p.config_dir.join(prxvc_config::CONFIG_FILE_NAME);
            out.push(format!(
                "config file: {} ({})",
                config_file.display(),
                exists(&config_file)
            ));
            out.push(format!(
                "data dir:    {} ({})",
                p.data_dir.display(),
                exists(&p.data_dir)
            ));
            out.push(format!(
                "models dir:  {} ({})",
                p.models_dir.display(),
                exists(&p.models_dir)
            ));
            out.push(format!(
                "voices dir:  {} ({})",
                p.voices_dir.display(),
                exists(&p.voices_dir)
            ));
            let log = p.logs_dir.join(crate::logging::LOG_FILE_NAME);
            out.push(format!("log file:    {} ({})", log.display(), exists(&log)));
        }
        Err(e) => out.push(format!("application directories: error: {e}")),
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags_select_the_mode() {
        assert_eq!(parse(&args(&[])), Mode::Gui);
        assert_eq!(parse(&args(&["--version"])), Mode::Version);
        assert_eq!(parse(&args(&["-V"])), Mode::Version);
        assert_eq!(parse(&args(&["--check"])), Mode::Check);
        assert_eq!(parse(&args(&["--check-call"])), Mode::CheckCall);
        assert_eq!(parse(&args(&["-psn_0_12345"])), Mode::Gui);
        assert_eq!(parse(&args(&["--no-such-flag"])), Mode::Unknown);
    }

    #[test]
    fn check_report_names_the_version_and_the_directories() {
        let report = check_report(&BundleStatus::Unbundled);
        assert!(report.starts_with(&version_line()));
        assert!(report.contains("none (running outside an application bundle)"));
        assert!(report.contains("org.openprx.prx-voice-client"));
        assert!(report.contains("data dir:"));
        assert!(report.contains("log file:"));
    }
}
