//! Composition root: builds the configuration, storage, engine, transport, audio collaborators and shared state, and wires the concrete types together.

use prxvc_audio::echo_gate::{DEFAULT_HOLD_OFF, EchoGate, EchoGateHandle};
use prxvc_audio::playback::AssistantPlayer;
use prxvc_audio::turn_policy::{TurnPolicy, TurnPolicyConfig};
use prxvc_core::engine::Engine;
use prxvc_desktop_ui::tool_window::{Geometry, ToolKind};
use prxvc_desktop_ui::SharedAppState;

use crate::app::PrxVoiceApp;
use crate::config_load::{app_paths, initialize_config};
use crate::engine_loop::run_engine_loop;
use crate::{hotkey, playback, settings, tray};

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
    shared.set_outside_app_bundle(cfg!(target_os = "macos") && crate::bundle::current().is_outside_bundle());
    // First thing that touches audio: put the default input back if the last run
    // ended in call mode without restoring it (pipeline-design PD-4.2.3).
    crate::routing_recovery::recover_at_start(&shared);
    let call = crate::call_compose::CallControl::start(&shared, &config);
    let handle = spawn_engine_thread(shared.clone(), config.clone(), call.clone());

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
        _runtime_handle: Some(handle),
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

/// Start the background thread that owns the tokio runtime and runs the engine loop.
fn spawn_engine_thread(
    shared: SharedAppState,
    config: prxvc_config::AppConfig,
    call: crate::call_compose::CallControl,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("Failed to create tokio runtime: {}", e);
                return;
            }
        };
        rt.block_on(run_engine_loop(shared, config, call));
    })
}

/// Attempt to open a SQLite-backed [`StorageManager`].
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

/// Create the engine with the transport the configuration selects. Returns
/// the engine and a label naming the transport for the start-up event.
pub(crate) fn build_engine(config: &prxvc_config::AppConfig) -> (Engine, &'static str) {
    let (event_tx, _event_rx) = prxvc_core::bus::event_channel(256);

    let transport: Box<dyn prxvc_protocol::transport::Transport> = if config.network.use_mock {
        tracing::info!("Using MockGateway (offline mode)");
        Box::new(prxvc_protocol::mock::MockGateway::new())
    } else {
        tracing::info!(
            "Connecting to prx-voice server: {} (asr={}, agent={}, tts={}, language={})",
            config.network.server_url,
            config.network.asr_provider,
            config.network.agent_provider,
            config.network.tts_provider,
            config.network.language,
        );
        let prx_config = prxvc_protocol::prx_voice_transport::PrxVoiceConfig {
            base_url: config.network.server_url.clone(),
            language: config.network.language.clone(),
            asr_provider: config.network.asr_provider.clone(),
            agent_provider: config.network.agent_provider.clone(),
            tts_provider: config.network.tts_provider.clone(),
            channel: "app_sdk".into(),
            client_id: uuid::Uuid::new_v4().to_string(),
        };
        Box::new(prxvc_protocol::prx_voice_transport::PrxVoiceTransport::new(prx_config))
    };

    let transport_label = if config.network.use_mock { "MockGateway" } else { "PrxVoiceTransport" };
    let mut engine = Engine::new(transport, event_tx);
    engine.set_auto_play_reply(config.audio.auto_play_reply);
    // The engine loop plays the reply audio: the end of a reply returns the
    // session to listening only once the output has drained.
    engine.set_external_playback(true);
    (engine, transport_label)
}

/// Create the playback driver: assistant speech goes to the default output
/// device, opened on first use. The echo gate keeps the microphone away from
/// the ASR while it plays (half duplex), unless `echo_control` is off
/// (headphones).
pub(crate) fn build_playback(config: &prxvc_config::AppConfig) -> playback::PlaybackDriver {
    let player = AssistantPlayer::new(Box::new(|| {
        Ok(Box::new(prxvc_audio::cpal_output::open_default()?)
            as Box<dyn prxvc_audio::stream_sink::StreamSink>)
    }));
    let gate = EchoGateHandle::new(EchoGate::new(config.audio.echo_control, DEFAULT_HOLD_OFF));
    playback::PlaybackDriver::new(player, gate)
}

/// Create the turn policy from the audio settings.
pub(crate) fn build_turn_policy(config: &prxvc_config::AppConfig) -> TurnPolicy {
    TurnPolicy::new(TurnPolicyConfig::from_ms(
        config.audio.merge_window_ms,
        config.audio.min_speech_ms,
    ))
}
