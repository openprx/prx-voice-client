//! The assistant's issue card on the orb stage: what went wrong in plain words and what to do about it.
//!
//! The application maps every typed provider problem and fault of the assistant to an
//! [`AssistantIssue`] (a message key, the English detail and an action). The card shows the message,
//! the detail in small type, a button that opens the settings section the fix lives in, and a close
//! button. Nothing here knows a provider or an error type.

use egui::Ui;

use crate::call_view::IssueAction;
use crate::settings_section::SettingsSection;
use crate::state::DesktopUiState;
use crate::theme::{Tokens, layout, radius, size, space};
use crate::widgets::{self, ButtonKind};

/// One thing that keeps the assistant from working, or went wrong in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssistantIssue {
    /// The i18n key of the message (`assistant.issue.<code>`).
    pub key: String,
    /// English detail from the provider or the loop, shown in small type; never spoken content.
    pub detail: String,
    /// What the user can do about it, right next to it.
    pub action: IssueAction,
}

/// The settings section an action opens and the key of its button label; `None` for no action.
pub(crate) fn action_target(action: IssueAction) -> Option<(SettingsSection, &'static str)> {
    match action {
        IssueAction::None | IssueAction::EndAssistantAndStart => None,
        IssueAction::OpenPermissions => Some((
            SettingsSection::Permissions,
            "assistant.action.open_permissions",
        )),
        IssueAction::OpenSpeechSettings => Some((
            SettingsSection::SpeechPipeline,
            "assistant.action.open_speech",
        )),
        IssueAction::OpenAudioSettings => {
            Some((SettingsSection::Audio, "assistant.action.open_audio"))
        }
        IssueAction::OpenAssistantSettings => Some((
            SettingsSection::Assistant,
            "assistant.action.open_assistant",
        )),
    }
}

/// Draw the card of the current issue between `top` and the bottom of `rect`; returns the height
/// it took (zero without an issue) so that the transcript card can start below it.
pub(crate) fn issue_card(
    ui: &mut Ui,
    state: &mut DesktopUiState,
    tk: &Tokens,
    rect: egui::Rect,
    top: f32,
) -> f32 {
    let Some(issue) = state.assistant_issue.clone() else {
        return 0.0;
    };
    let card = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 20.0, top),
        egui::pos2(rect.right() - 20.0, rect.bottom() - 10.0),
    );
    if card.width() < 120.0 || card.height() < 40.0 {
        return 0.0;
    }
    let mut open: Option<SettingsSection> = None;
    let mut close = false;
    let used = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(card)
                .layout(egui::Layout::top_down(egui::Align::LEFT)),
            |ui| {
                ui.push_id("assistant_issue", |ui| {
                    // The message wraps: the window is narrow and the sentences are long.
                    egui::Frame::NONE
                        .fill(tk.danger_bg)
                        .corner_radius(egui::CornerRadius::same(radius::CARD))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(widgets::text(
                                    prxvc_i18n::t(&issue.key),
                                    size::SECONDARY,
                                    tk.text,
                                ))
                                .wrap(),
                            );
                            if !issue.detail.is_empty() {
                                ui.add(
                                    egui::Label::new(widgets::text(
                                        &issue.detail,
                                        size::CAPTION,
                                        tk.secondary,
                                    ))
                                    .wrap(),
                                );
                            }
                            ui.add_space(space::S4);
                            ui.horizontal(|ui| {
                                if let Some((section, label)) = action_target(issue.action)
                                    && widgets::button_with(
                                        ui,
                                        tk,
                                        ButtonKind::Secondary,
                                        &prxvc_i18n::t(label),
                                        layout::CONTROL_HEIGHT_SMALL,
                                        true,
                                    )
                                    .clicked()
                                {
                                    open = Some(section);
                                }
                                if widgets::button_with(
                                    ui,
                                    tk,
                                    ButtonKind::Text,
                                    &prxvc_i18n::t("common.close"),
                                    layout::CONTROL_HEIGHT_SMALL,
                                    true,
                                )
                                .clicked()
                                {
                                    close = true;
                                }
                            });
                        });
                });
            },
        )
        .response
        .rect
        .height();
    if let Some(section) = open {
        state.open_settings(Some(section));
    }
    if close {
        state.assistant_issue = None;
        state.pending_commands.push(DISMISS_ISSUE.into());
    }
    used + space::S8
}

/// The command that asks the app to forget the issue.
pub const DISMISS_ISSUE: &str = "DismissAssistantIssue";
