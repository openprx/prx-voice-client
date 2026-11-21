//! The head of the Call translation window: the state as a chip in plain words with the call's duration, the language pair with its swap, and the one prominent action with Pause beside it and the Translate switch.

use crate::call_view::{CallPanel, CallPhase, command};
use crate::call_window_state::CallWindowState;
use crate::call_window_view::CallWindowAction;
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, size, space};
use crate::widgets::{self, ButtonKind, Tone};

/// The state chip's words.
fn chip_text(phase: CallPhase) -> String {
    prxvc_i18n::t(match phase {
        CallPhase::Idle => "callwin.chip.idle",
        CallPhase::Initializing => "callwin.chip.loading",
        CallPhase::AudioReady => "callwin.chip.audio",
        CallPhase::Translating => "callwin.chip.translating",
        CallPhase::Paused => "callwin.chip.paused",
        CallPhase::Degraded => "callwin.chip.reduced",
        CallPhase::Recovering => "callwin.chip.recovering",
        CallPhase::Stopping => "callwin.chip.stopping",
    })
}

/// The colour family of the chip: tinted only for facts that need attention or a running translation.
fn chip_tone(phase: CallPhase) -> Tone {
    match phase {
        CallPhase::Translating => Tone::Success,
        CallPhase::Degraded | CallPhase::Recovering => Tone::Warning,
        _ => Tone::Neutral,
    }
}

/// The language of the interface as a code.
fn interface_code() -> &'static str {
    match prxvc_i18n::current_locale() {
        prxvc_i18n::Locale::ZhCn => "zh",
        _ => "en",
    }
}

/// The other one of the two languages.
fn other(code: &str) -> &'static str {
    if code == "zh" { "en" } else { "zh" }
}

/// The language codes the user speaks and the other party speaks: what the running call
/// translates, otherwise the settings (empty: the interface language, and the other one).
pub(crate) fn language_codes(panel: &CallPanel, settings: &UiSettings) -> (String, String) {
    if let Some((from, to)) = &panel.outgoing.languages {
        return (from.clone(), to.clone());
    }
    let own = if settings.call_self_language.is_empty() {
        interface_code().to_owned()
    } else {
        settings.call_self_language.clone()
    };
    let peer = if settings.call_peer_language.is_empty() {
        other(&own).to_owned()
    } else {
        settings.call_peer_language.clone()
    };
    (own, peer)
}

/// The name of a language in its own words.
pub(crate) fn language_name(code: &str) -> String {
    match code {
        "zh" => prxvc_i18n::t("callwin.lang.zh"),
        "en" => prxvc_i18n::t("callwin.lang.en"),
        other => other.to_owned(),
    }
}

/// Row one: the state chip (its words are the state, said once; the longer sentence is its hover
/// text), the call's duration while one runs, and the language pair with Swap and Edit while
/// call mode is off. Row two: the controls.
pub(crate) fn header(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    translate: &mut bool,
    settings: &mut UiSettings,
    w: &mut CallWindowState,
    actions: &mut Vec<CallWindowAction>,
) {
    status_row(ui, tk, panel, settings, w);
    ui.add_space(space::S8);
    controls(ui, tk, panel, translate, actions);
}

/// "[chip] 02:31 中文 ⇄ English  Swap Edit".
fn status_row(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    settings: &mut UiSettings,
    w: &mut CallWindowState,
) {
    let (own, peer) = language_codes(panel, settings);
    let pair = prxvc_i18n::t_with(
        "callwin.lang_pair",
        &[&language_name(&own), &language_name(&peer)],
    );
    ui.horizontal_wrapped(|ui| {
        widgets::chip(ui, tk, chip_tone(panel.phase), &chip_text(panel.phase))
            .on_hover_text(crate::call_window_view::state_text(panel));
        if panel.phase != CallPhase::Idle {
            ui.label(widgets::text(
                crate::call_transcript::mmss(w.elapsed_s()),
                size::SECONDARY,
                tk.secondary,
            ));
        }
        ui.label(widgets::text(pair, size::SECONDARY, tk.text));
        if !panel.translation.engine_key.is_empty() {
            let name = widgets::text(
                prxvc_i18n::t(panel.translation.engine_key),
                size::META,
                tk.secondary,
            );
            // Quiet and optional: only where it fits on the row (the details always name it).
            let width = egui::WidgetText::from(name.clone())
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x;
            let left = ui.max_rect().right() - ui.next_widget_position().x;
            if width + ui.spacing().item_spacing.x <= left {
                ui.label(name);
            }
        }
        if panel.can_start() {
            if widgets::button_with(
                ui,
                tk,
                ButtonKind::Text,
                &prxvc_i18n::t("callwin.swap"),
                layout::CONTROL_HEIGHT_SMALL,
                true,
            )
            .clicked()
            {
                settings.call_self_language = peer.clone();
                settings.call_peer_language = own.clone();
            }
            if widgets::button_with(
                ui,
                tk,
                ButtonKind::Text,
                &prxvc_i18n::t("callwin.edit"),
                layout::CONTROL_HEIGHT_SMALL,
                true,
            )
            .clicked()
            {
                w.details_open = true;
            }
        }
    });
}

/// The one primary button (Start, or Stop while a call runs), Pause or Resume, and the Translate
/// switch. The switch decides what the next start does, so it can only change while call mode is
/// off; while a call runs it shows what this call does, dimmed, with the reason as its hover text.
fn controls(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    translate: &mut bool,
    actions: &mut Vec<CallWindowAction>,
) {
    let running = panel.phase != CallPhase::Idle;
    let start = if *translate {
        command::START_TRANSLATED
    } else {
        command::START_AUDIO_ONLY
    };
    ui.horizontal_wrapped(|ui| {
        let (label, cmd, enabled) = if running {
            ("callwin.stop", command::STOP, panel.can_stop())
        } else {
            ("callwin.start", start, panel.can_start())
        };
        if widgets::button_with(
            ui,
            tk,
            ButtonKind::Primary,
            &prxvc_i18n::t(label),
            layout::CONTROL_HEIGHT,
            enabled,
        )
        .clicked()
        {
            actions.push(CallWindowAction::Command(cmd));
        }
        let (label, cmd, enabled) = if panel.can_resume() {
            ("call.resume", command::RESUME, true)
        } else {
            ("call.pause", command::PAUSE, panel.can_pause())
        };
        if widgets::button_with(
            ui,
            tk,
            ButtonKind::Secondary,
            &prxvc_i18n::t(label),
            layout::CONTROL_HEIGHT,
            enabled,
        )
        .clicked()
        {
            actions.push(CallWindowAction::Command(cmd));
        }
        if panel.can_reapply()
            && widgets::button_with(
                ui,
                tk,
                ButtonKind::Secondary,
                &prxvc_i18n::t("call.reapply"),
                layout::CONTROL_HEIGHT,
                true,
            )
            .clicked()
        {
            actions.push(CallWindowAction::Command(command::REAPPLY_MICROPHONE));
        }
        let label = prxvc_i18n::t("call.translate");
        ui.name_inputs(&label, |ui| {
            if panel.can_start() {
                widgets::switch(ui, tk, translate);
            } else {
                widgets::switch_locked(
                    ui,
                    tk,
                    panel.translate_enabled,
                    &label,
                    &prxvc_i18n::t("callwin.translate_locked"),
                );
            }
        });
        ui.label(widgets::text(&label, size::SECONDARY, tk.text));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_view::LaneView;

    #[test]
    fn the_pair_follows_a_running_call_and_otherwise_the_settings() {
        let settings = UiSettings {
            call_self_language: "zh".into(),
            call_peer_language: String::new(),
            ..UiSettings::default()
        };
        assert_eq!(
            language_codes(&CallPanel::default(), &settings),
            ("zh".into(), "en".into())
        );
        let running = CallPanel {
            outgoing: LaneView {
                languages: Some(("en".into(), "zh".into())),
                ..LaneView::default()
            },
            ..CallPanel::default()
        };
        assert_eq!(language_codes(&running, &settings), ("en".into(), "zh".into()));
    }

    #[test]
    fn the_chip_tone_is_tinted_only_when_it_means_something() {
        assert_eq!(chip_tone(CallPhase::Idle), Tone::Neutral);
        assert_eq!(chip_tone(CallPhase::Paused), Tone::Neutral);
        assert_eq!(chip_tone(CallPhase::Translating), Tone::Success);
        assert_eq!(chip_tone(CallPhase::Degraded), Tone::Warning);
    }
}
