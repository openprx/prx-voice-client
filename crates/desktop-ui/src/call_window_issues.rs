//! The issues of the Call translation window: shown only when there is something to say or do, each as a card with an icon, one plain sentence and at most one action button.

use egui::{Sense, Stroke, vec2};

use crate::call_view::{CallPanel, IssueAction, IssueSeverity, IssueView, command};
use crate::call_window_view::CallWindowAction;
use crate::settings_section::SettingsSection;
use crate::theme::{Tokens, layout, radius, size, space};
use crate::widgets::{self, ButtonKind, Tone};

/// The label key of the button of an action, and what a click does.
fn action_of(action: IssueAction) -> Option<(&'static str, CallWindowAction)> {
    match action {
        IssueAction::None => None,
        IssueAction::EndAssistantAndStart => Some((
            "callwin.action.end_assistant_and_start",
            CallWindowAction::Command(command::END_ASSISTANT_AND_START),
        )),
        IssueAction::OpenPermissions => Some((
            "callwin.action.open_permissions",
            CallWindowAction::OpenSettings(SettingsSection::Permissions),
        )),
        IssueAction::OpenSpeechSettings => Some((
            "callwin.action.open_speech",
            CallWindowAction::OpenSettings(SettingsSection::SpeechPipeline),
        )),
        IssueAction::OpenAudioSettings => Some((
            "callwin.action.open_audio",
            CallWindowAction::OpenSettings(SettingsSection::Audio),
        )),
        IssueAction::OpenAssistantSettings => Some((
            "assistant.action.open_assistant",
            CallWindowAction::OpenSettings(SettingsSection::Assistant),
        )),
    }
}

/// The issue keys that concern the translation engine; the engine's actions go under them.
const ENGINE_ISSUES: [&str; 7] = [
    "call.issue.provider.language_pack_missing",
    "call.issue.provider.os_too_old",
    "call.issue.provider.server_failed",
    "call.issue.provider.server_unreachable",
    "call.issue.provider.runtime_not_built_in",
    "call.issue.provider.start_failed",
    "call.issue.provider.unsupported",
];

fn tone_of(severity: IssueSeverity) -> Tone {
    match severity {
        IssueSeverity::Blocking => Tone::Danger,
        IssueSeverity::Degraded => Tone::Warning,
        IssueSeverity::Info => Tone::Neutral,
    }
}

/// Whether there is anything to show.
pub(crate) fn any(panel: &CallPanel) -> bool {
    !panel.issues.is_empty() || panel.last_error.is_some()
}

/// The cards, one per issue, then the last failure when it is not among them.
pub(crate) fn issues(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    actions: &mut Vec<CallWindowAction>,
) {
    if !any(panel) {
        return;
    }
    ui.add_space(space::S12);
    ui.spacing_mut().item_spacing.y = space::S4;
    for (i, issue) in panel.issues.iter().enumerate() {
        ui.push_id(("issue", i), |ui| card(ui, tk, panel, issue, actions));
    }
    if let Some(error) = &panel.last_error
        && !panel.issues.iter().any(|i| i.key == error.key)
    {
        ui.push_id("last_error", |ui| card(ui, tk, panel, error, actions));
    }
}

fn card(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    issue: &IssueView,
    actions: &mut Vec<CallWindowAction>,
) {
    let tone = tone_of(issue.severity);
    let (fill, glyph) = match tone {
        Tone::Danger => (tk.danger_bg, tk.danger),
        Tone::Warning => (tk.warning_bg, tk.warning),
        Tone::Success => (tk.success_bg, tk.success),
        Tone::Neutral => (tk.surface, tk.secondary),
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                let c = rect.center();
                ui.painter().circle_stroke(c, 7.0, Stroke::new(1.5, glyph));
                ui.painter().text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    if tone == Tone::Neutral { "i" } else { "!" },
                    egui::FontId::proportional(size::CAPTION),
                    glyph,
                );
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(widgets::text(
                            prxvc_i18n::t(&issue.key),
                            size::SECONDARY,
                            tk.text,
                        ))
                        .wrap(),
                    );
                    if let Some((key, action)) = action_of(issue.action) {
                        ui.add_space(space::S4);
                        if widgets::button_with(
                            ui,
                            tk,
                            ButtonKind::Secondary,
                            &prxvc_i18n::t(key),
                            layout::CONTROL_HEIGHT_SMALL,
                            true,
                        )
                        .clicked()
                        {
                            actions.push(action);
                        }
                    }
                    if ENGINE_ISSUES.contains(&issue.key.as_str()) {
                        for engine_action in &panel.translation.actions {
                            ui.add_space(space::S4);
                            if widgets::button_with(
                                ui,
                                tk,
                                ButtonKind::Secondary,
                                &prxvc_i18n::t(engine_action.label_key()),
                                layout::CONTROL_HEIGHT_SMALL,
                                true,
                            )
                            .clicked()
                            {
                                actions.push(CallWindowAction::Command(engine_action.command()));
                            }
                        }
                    }
                });
            });
        });
}
