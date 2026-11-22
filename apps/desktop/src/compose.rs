//! Composition root: builds the configuration, storage, call control and shared state, starts the assistant's host and wires the concrete types together.

use prxvc_desktop_ui::SharedAppState;
use prxvc_desktop_ui::tool_window::{Geometry, ToolKind};

use crate::app::PrxVoiceApp;
use crate::config_load::{app_paths, initialize_config};
use crate::{hotkey, settings, tray};

/// Start the manager of the local model server, and keep the server running when the language
/// model is the selected translation engine.
fn start_llm_server(config: &prxvc_config::AppConfig) {
    let Some(paths) = crate::config_load::app_paths() else {
        tracing::warn!("no application directories: the local model server is not managed");
        return;
    };
    crate::llm_server::install(config, paths);
    crate::call_kit::model_server::install(|provider, base_url| {
        crate::llm_server::hold_for(provider, base_url)
            .map(|lease| lease.map(|l| Box::new(l) as crate::call_kit::model_server::Hold))
            .map_err(|problem| problem.to_string())
    });
    crate::llm_server::on_engine_changed(&config.call.translation_engine);
}

/// Bootstrap the application: install fonts and theme, load the configuration,
/// create shared state and spawn the engine runtime.
pub(crate) fn build_app(cc: &eframe::CreationContext<'_>) -> PrxVoiceApp {
    install_fonts(&cc.egui_ctx);

    // The RF theme is the default: install its light and dark styles and
    // follow the system preference. The central panel draws with an
    // explicit transparent frame, so no panel fill override is needed.
    prxvc_desktop_ui::theme::install(&cc.egui_ctx);

    // Initialize i18n with default locale
    prxvc_i18n::set_locale(prxvc_i18n::Locale::En);

    // One configuration load serves both the engine and the settings popup.
    let config = initialize_config();
    let mut ui_state = prxvc_desktop_ui::DesktopUiState::new();
    ui_state.settings = settings::ui_settings_from_config(&config);
    ui_state.call_translate = config.call.translate;
    let last_saved = settings::snapshot(&ui_state.settings);

    restore_window_geometry(cc, &mut ui_state);
    let initial_locale = if config.ui.language == "zh-CN" {
        prxvc_i18n::set_locale(prxvc_i18n::Locale::ZhCn);
        "zh-CN"
    } else {
        "en"
    };

    let shared = SharedAppState::new();
    shared.set_repaint_context(cc.egui_ctx.clone());
    shared.set_outside_app_bundle(
        cfg!(target_os = "macos") && crate::bundle::current().is_outside_bundle(),
    );
    // First thing that touches audio: put the default input back if the last run
    // ended in call mode without restoring it (pipeline-design PD-4.2.3).
    crate::routing_recovery::recover_at_start(&shared);
    crate::call_engine::set(&config.call.translation_engine);
    start_llm_server(&config);
    let call = crate::call_compose::CallControl::start(&shared, &config);
    let handle = spawn_assistant_thread(shared.clone(), call.clone());

    let hotkey_manager = match hotkey::HotkeyManager::new(cc.egui_ctx.clone()) {
        Ok(m) => {
            tracing::info!("Global hotkey registered: Ctrl+Shift+Space");
            Some(m)
        }
        Err(e) => {
            tracing::warn!("Failed to register hotkey: {}", e);
            None
        }
    };

    let tray_manager = match tray::TrayManager::new(cc.egui_ctx.clone()) {
        Ok(t) => {
            tracing::info!("System tray created");
            Some(t)
        }
        Err(e) => {
            tracing::warn!("Failed to create tray: {}", e);
            None
        }
    };

    PrxVoiceApp {
        ui_state,
        shared,
        _runtime_handle: handle,
        hotkey_manager,
        tray_manager,
        last_locale: initial_locale.to_owned(),
        config,
        last_saved,
        call,
    }
}

/// Install the embedded UI fonts plus a system CJK fallback when one exists.
fn install_fonts(ctx: &eframe::egui::Context) {
    // Embedded UI fonts (Hanken Grotesk, JetBrains Mono, Noto Sans SC
    // subset). A system CJK font is the fallback after the embedded
    // Chinese face, for characters outside the embedded subset.
    let cjk_paths = [
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
    ];
    let system_cjk = cjk_paths.iter().find_map(|path| {
        let data = std::fs::read(path).ok()?;
        tracing::info!("System CJK fallback font loaded from {}", path);
        Some(data)
    });
    if system_cjk.is_none() {
        tracing::warn!("No system CJK font found; only the embedded subset covers Chinese");
    }
    prxvc_desktop_ui::fonts::install(ctx, system_cjk);
}

/// Remembered position and size of the settings/history/debug windows
/// (eframe persists only the main window).
fn restore_window_geometry(
    cc: &eframe::CreationContext<'_>,
    ui_state: &mut prxvc_desktop_ui::DesktopUiState,
) {
    if let Some(storage) = cc.storage {
        for kind in ToolKind::ALL {
            let saved = eframe::get_value::<(f32, f32, f32, f32)>(storage, kind.storage_key())
                .and_then(Geometry::from_tuple);
            if let Some(geometry) = saved {
                ui_state.windows.get_mut(kind).restore(geometry);
            }
        }
    }
}

/// Start the background thread that runs the assistant's host.
fn spawn_assistant_thread(
    shared: SharedAppState,
    call: crate::call_compose::CallControl,
) -> Option<std::thread::JoinHandle<()>> {
    match crate::assistant::run::spawn(shared, call) {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::error!("Failed to start the assistant thread: {e}");
            None
        }
    }
}

/// Attempt to open a SQLite-backed `StorageManager`.
///
/// Returns `None` (with a warning log) if the platform data directory cannot
/// be resolved or the database cannot be opened, so the app can continue
/// without persistence.
pub(crate) fn initialize_storage() -> Option<prxvc_storage::StorageManager> {
    let db_path = app_paths()?.data_dir.join("sessions.db");
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match prxvc_storage::StorageManager::new(db_path) {
        Ok(s) => {
            tracing::info!("Storage initialized");
            Some(s)
        }
        Err(e) => {
            tracing::warn!("Storage init failed: {e}");
            None
        }
    }
}
