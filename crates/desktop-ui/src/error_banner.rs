//! Error banner: the danger tip bar shown while the UI state carries an error message.

use crate::state::DesktopUiState;
use crate::theme::layout;
use crate::widgets::{self, ButtonKind, Tone};

/// Render an error banner at the top of the screen if there is an active error.
///
/// Shown as a danger tip bar. Recoverable errors show a retry button. Both
/// include a close button.
pub fn render_error_banner(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    if let Some(ref error) = state.error_message {
        let error_text = error.clone();
        let tk = state.theme.tokens();

        widgets::tip_bar(ui, &tk, Tone::Danger, &error_text, |ui| {
            if state.error_recoverable
                && widgets::button_with(
                    ui,
                    &tk,
                    ButtonKind::Secondary,
                    &prxvc_i18n::t("common.retry"),
                    layout::CONTROL_HEIGHT_SMALL,
                    true,
                )
                .clicked()
            {
                state.pending_commands.push("RetryConnection".into());
                state.error_message = None;
            }
            if widgets::button_with(
                ui,
                &tk,
                ButtonKind::Text,
                &prxvc_i18n::t("common.close"),
                layout::CONTROL_HEIGHT_SMALL,
                true,
            )
            .clicked()
            {
                state.error_message = None;
            }
        });
    }
}
