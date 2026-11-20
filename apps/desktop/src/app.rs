//! The egui application: [`PrxVoiceApp`] state and its `eframe::App` implementation.

use std::thread::JoinHandle;

use eframe::egui;
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{SharedAppState, Theme};

use crate::persistence::save_settings_to_config;
use crate::{hotkey, settings, tray};

/// Top-level application wiring egui to the async runtime.
pub(crate) struct PrxVoiceApp {
    /// Mutable UI state consumed by the render functions.
    pub(crate) ui_state: prxvc_desktop_ui::DesktopUiState,
    /// Shared state bridging the UI thread and the tokio runtime.
    pub(crate) shared: SharedAppState,
    /// Handle to the background runtime thread (kept alive for the app lifetime).
    pub(crate) _runtime_handle: Option<JoinHandle<()>>,
    /// Global hotkey manager (optional — degrades gracefully).
    pub(crate) hotkey_manager: Option<hotkey::HotkeyManager>,
    /// System tray manager (optional — degrades gracefully).
    pub(crate) tray_manager: Option<tray::TrayManager>,
    /// Last observed i18n locale, used to detect language changes reliably.
    pub(crate) last_locale: String,
    /// The configuration in effect (loaded at startup, the one the engine is
    /// built from). Settings saves start from it and change only what the
    /// popup edits.
    pub(crate) config: prxvc_config::AppConfig,
    /// Persisted subset of the settings last written (or loaded).
    pub(crate) last_saved: settings::SettingsSnapshot,
}

impl eframe::App for PrxVoiceApp {
    /// Tell the glow renderer to clear with fully transparent color.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        for kind in ToolKind::ALL {
            if let Some(geometry) = self.ui_state.windows.get(kind).geometry() {
                eframe::set_value(storage, kind.storage_key(), &geometry.to_tuple());
            }
        }
    }

    /// Non-drawing work. eframe calls this before every `ui` pass and also
    /// while the window is hidden when a repaint was requested (the hotkey
    /// and tray handlers request one).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Sync state from async runtime
        self.ui_state.sync_from_shared(&self.shared);

        // Check hotkey
        if let Some(ref mut hk) = self.hotkey_manager {
            if hk.was_triggered() {
                if !self.ui_state.session_active {
                    self.shared.push_command("CreateSession");
                    self.shared.push_command("StartSession");
                } else {
                    self.shared.push_command("StopSession");
                }
            }
        }

        // Check tray actions
        if let Some(ref tray) = self.tray_manager {
            if let Some(action) = tray.poll_action() {
                match action {
                    tray::TrayAction::StartSession => {
                        self.shared.push_command("CreateSession");
                        self.shared.push_command("StartSession");
                        // Optimistic update so orb responds immediately
                        self.shared.update_session_state("Listening", true);
                    }
                    tray::TrayAction::StopSession => {
                        self.shared.push_command("StopSession");
                        // Optimistic update so orb responds immediately
                        self.shared.update_session_state("Idle", false);
                        self.shared.clear_transcript();
                    }
                    tray::TrayAction::Settings => {
                        self.ui_state
                            .windows
                            .request_open(ToolKind::Settings);
                    }
                    tray::TrayAction::Quit => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    tray::TrayAction::Show => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            }
        }

    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Render the entire UI
        prxvc_desktop_ui::render_app(ui, &mut self.ui_state);

        // Apply pending theme change (if settings changed the theme)
        if let Some(theme_name) = self.ui_state.pending_theme.take() {
            if let Some(new_theme) = Theme::all()
                .into_iter()
                .find(|t| t.name == theme_name)
            {
                // The new theme's style is applied by the UI on its next frame.
                self.ui_state.theme = new_theme;
            }
        }

        // Refresh tray menu labels when locale changes
        let current_locale = prxvc_i18n::current_locale().as_str().to_owned();
        if current_locale != self.last_locale {
            self.last_locale = current_locale;
            if let Some(ref tray) = self.tray_manager {
                tray.refresh_labels();
            }
        }

        // Persist settings to config.toml when they change
        save_settings_to_config(&mut self.config, &mut self.last_saved, &self.ui_state.settings);

        // Flush commands to async runtime
        self.ui_state.flush_commands_to_shared(&self.shared);
    }
}
