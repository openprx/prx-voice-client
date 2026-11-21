//! PRX Voice Client desktop application: entry point and module table of contents.
//!
//! Runs the egui UI on the main thread and the assistant's host on a background thread. The two
//! communicate through [`SharedAppState`] which uses `parking_lot::Mutex` for minimal-contention
//! synchronization.
//!
//! Modules:
//! - `compose`: composition root, the only place concrete types are wired together
//! - `config_load`: configuration load and legacy-migration reporting
//! - `app`: the egui application and its `eframe::App` implementation
//! - `assistant`: the voice assistant's host: the orb's commands, the loop's events, issues, the session record
//! - `event_map`: the debug log
//! - `persistence`: history load
//! - `call_compose`: call mode: the coordinator thread, its commands and snapshots, exclusion with the assistant
//! - `call_panel`: the coordinator's snapshot as the debug control's view model
//! - `call_host` (macOS): the real host and audio paths behind the coordinator's seams
//! - `routing_recovery`: restores the system default input after a crash in call mode, at start
//! - `diagnostics`: diagnostics export
//! - `driver_install`: virtual microphone driver status, install, update and uninstall
//! - `bundle`: whether the process runs from its macOS application bundle
//! - `cli`: `--version` and `--check`, which run without a window
//! - `logging`: stderr or size-capped log file
//! - `native_libs`: which native runtime libraries are linked, for `--check`
//! - `settings`: mapping between the configuration and the settings window (pure)
//! - `settings_data`: what the settings window shows that only the app can probe
//! - `settings_host`: applies the window's edits, commands and memory; runs the probes
//! - `hotkey`, `tray`: platform helpers

mod app;
mod assistant;
mod bundle;
mod call_engine;
mod call_kit;
#[cfg(target_os = "macos")]
mod call_host;
mod call_compose;
mod call_panel;
mod cli;
mod compose;
mod config_load;
mod diagnostics;
mod driver_install;
mod event_map;
mod hotkey;
mod llm_server;
mod logging;
mod native_libs;
mod persistence;
mod routing_recovery;
mod settings;
mod settings_data;
mod settings_host;
mod tray;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&args) {
        cli::Mode::Version => {
            println!("{}", cli::version_line());
            return Ok(());
        }
        cli::Mode::Check => {
            println!("{}", cli::check_report(bundle::current()));
            return Ok(());
        }
        cli::Mode::CheckCall => {
            println!("{}", cli::check_report(bundle::current()));
            println!("{}", check_call_report());
            return Ok(());
        }
        cli::Mode::Unknown => {
            eprintln!(
                "prxvc-desktop: unknown option (this version knows --version, --check, --check-call)"
            );
            std::process::exit(2);
        }
        cli::Mode::Gui => {}
    }

    init_logging();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_inner_size([220.0, 320.0])
            .with_min_inner_size([180.0, 260.0])
            .with_title("PRX Voice"),
        ..Default::default()
    };

    eframe::run_native(
        "PRX Voice",
        options,
        Box::new(|cc| Ok(Box::new(compose::build_app(cc)))),
    )
}

/// What a translated call needs from this machine, as the report lines of `--check-call`.
#[cfg(target_os = "macos")]
fn check_call_report() -> String {
    use call_kit::readiness::{machine_lines, provider_lines, render};
    let config = call_kit::config::load_current();
    let mut lines = machine_lines(&config);
    lines.extend(provider_lines(&config));
    render(&lines)
}

#[cfg(not(target_os = "macos"))]
fn check_call_report() -> String {
    "call mode needs macOS".to_owned()
}

/// Install logging, then record where the process runs from. A process
/// outside an application bundle gets one clear warning, because macOS then
/// attributes microphone and audio-capture permission to the launching
/// application (and a tap started from a terminal delivers silence).
fn init_logging() {
    let logs_dir = prxvc_config::paths::AppPaths::resolve(&prxvc_config::AppConfig::default())
        .ok()
        .map(|p| p.logs_dir)
        .filter(|dir| prxvc_config::paths::create_private_dir_all(dir).is_ok());
    let log_file = logging::init(logs_dir.as_deref());
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        log_file = ?log_file,
        "PRX Voice starting"
    );
    match bundle::current() {
        bundle::BundleStatus::Bundled { identifier, app_dir } => {
            tracing::info!(%identifier, app = %app_dir.display(), "running from the application bundle");
        }
        bundle::BundleStatus::Unbundled if cfg!(target_os = "macos") => tracing::warn!(
            "running outside an application bundle: microphone and call-audio permissions go to the \
             launching application, and call-audio capture will deliver silence; start PRX Voice from \
             its .app (scripts/run-app.sh)"
        ),
        bundle::BundleStatus::Unbundled => {}
    }
}
