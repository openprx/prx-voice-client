//! PRX Voice Client desktop application: entry point and module table of contents.
//!
//! Runs the egui UI on the main thread and a tokio async runtime on a
//! background thread. The two communicate through [`SharedAppState`] which
//! uses `parking_lot::Mutex` for minimal-contention synchronization.
//!
//! Modules:
//! - `compose`: composition root, the only place concrete types are wired together
//! - `config_load`: configuration load and legacy-migration reporting
//! - `app`: the egui application and its `eframe::App` implementation
//! - `engine_loop`: the polling loop on the background runtime
//! - `commands`: UI command dispatch
//! - `event_map`: engine events to shared UI state
//! - `asr_session`: ASR and audio capture loop start-up
//! - `persistence`: history, session save and settings write
//! - `diagnostics`: diagnostics export
//! - `hotkey`, `tray`, `playback`, `settings`: platform and device helpers

mod app;
mod asr_session;
mod commands;
mod compose;
mod config_load;
mod diagnostics;
mod engine_loop;
mod event_map;
mod hotkey;
mod persistence;
mod playback;
mod settings;
mod tray;

use eframe::egui;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

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
