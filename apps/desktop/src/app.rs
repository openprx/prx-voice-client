//! The egui application: [`PrxVoiceApp`] state and its `eframe::App` implementation.

use std::thread::JoinHandle;

use eframe::egui;
use prxvc_desktop_ui::tool_window::ToolKind;
use prxvc_desktop_ui::{SharedAppState, Theme};

use crate::{hotkey, settings, settings_host, tray};

/// Open the Call translation window from the tray. The tool windows are drawn inside the orb
/// window's pass, which does not run while that window is hidden, so the orb window is shown first.
pub(crate) fn open_call_window(ctx: &egui::Context, ui_state: &mut prxvc_desktop_ui::DesktopUiState) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ui_state.windows.request_open(ToolKind::Call);
}

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
    /// The configuration in effect at startup (a session reads the file again when it starts). The settings window writes through the configuration's
    /// atomic `update`, never into this copy.
    pub(crate) config: prxvc_config::AppConfig,
    /// The state of the settings window last known to be stored (or loaded);
    /// edits are found by comparing against it.
    pub(crate) last_saved: settings::SettingsSnapshot,
    /// Call mode: stopped (and the default input restored) when the app closes.
    pub(crate) call: crate::call_compose::CallControl,
}

impl eframe::App for PrxVoiceApp {
    /// Tell the glow renderer to clear with fully transparent color.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// Closing the app ends call mode first, so the system default input is
    /// restored instead of being left to the next start's crash recovery.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.call.shutdown();
        settings_host::on_exit(&self.ui_state);
        // Stops the model server we started, if any (an adopted one is left alone).
        crate::llm_server::shutdown();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        for kind in ToolKind::ALL {
            // The settings window's place lives in `ui.settings_window` of the configuration.
            if kind == ToolKind::Settings {
                continue;
            }
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
        settings_host::before_frame(
            ctx,
            &mut self.ui_state,
            &self.shared,
            &self.config,
            &mut self.last_saved,
        );

        // Check hotkey
        if let Some(ref mut hk) = self.hotkey_manager {
            if hk.was_triggered() {
                if self.ui_state.session_state == "Starting" {
                    // Loading: ignored, never queued behind the start.
                } else if !self.ui_state.session_active {
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
                    tray::TrayAction::CallTranslation => {
                        open_call_window(ctx, &mut self.ui_state);
                    }
                    tray::TrayAction::Settings => {
                        self.ui_state.open_settings(None);
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
        // The tray's first line says what the app is doing (text is set only when it changes;
        // a language change changes the text too).
        let issue_text = self
            .ui_state
            .assistant_issue
            .as_ref()
            .map(|i| prxvc_i18n::t(&i.key));
        if let Some(ref tray) = self.tray_manager {
            tray.set_state_line(
                &tray::tray_state(
                    &self.ui_state.session_state,
                    self.ui_state.call.phase,
                    issue_text.as_deref(),
                )
                .text(),
            );
        }

        // The engine commands of the Call translation window become settings edits (or open the
        // System Settings pane); then the settings below are saved as usual.
        crate::call_engine::intercept(&mut self.ui_state);

        // Store settings edits through the configuration's atomic update, run the
        // window's commands, remember the window when it closed.
        settings_host::after_frame(
            ui.ctx(),
            &mut self.ui_state,
            &self.shared,
            &mut self.last_saved,
        );

        // Flush commands to async runtime
        self.ui_state.flush_commands_to_shared(&self.shared);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_entry_opens_the_call_translation_window_and_shows_the_orb_window() {
        let ctx = egui::Context::default();
        let mut state = prxvc_desktop_ui::DesktopUiState::new();
        assert!(!state.windows.call.is_open());
        open_call_window(&ctx, &mut state);
        assert!(state.windows.call.is_open());
        // Opening it again only asks for focus.
        open_call_window(&ctx, &mut state);
        assert!(state.windows.call.is_open());
        assert!(!state.windows.settings.is_open());
    }
}
