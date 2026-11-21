//! Development helpers for this machine: adopt models into the model store, install the owner's voice and a stand-in preset voice, and report what a translated call still needs. Not part of the product.
//!
//! ```text
//! prxvc-dev adopt-models  [--source DIR]... [--dry-run]
//! prxvc-dev install-voices [--refs DIR] [--no-prepare]
//! prxvc-dev preflight
//! prxvc-dev translate-compare [--out FILE]
//! ```
//!
//! Everything is written inside the application's own data directory (`PRXVC_DATA_DIR`, else
//! `~/Library/Application Support/org.openprx.prx-voice-client/`). The scripts in `scripts/dev/`
//! build and run this program with the native features.

#[path = "../../call_kit/mod.rs"]
mod call_kit;

mod adopt;
mod compare;
mod preflight;
mod sentences;
mod voices;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: prxvc-dev adopt-models [--source DIR]... [--dry-run]\n       prxvc-dev install-voices [--refs DIR] [--no-prepare]\n       prxvc-dev preflight\n       prxvc-dev translate-compare [--out FILE]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        return usage();
    };
    let result = match command.as_str() {
        "adopt-models" => adopt::run(rest),
        "install-voices" => voices::run(rest),
        "preflight" => call_kit::main_loop::with_main_loop(preflight::run),
        "translate-compare" => call_kit::main_loop::with_main_loop(|| compare::run(rest)),
        _ => return usage(),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}
