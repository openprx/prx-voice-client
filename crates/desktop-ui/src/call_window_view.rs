//! The Call translation window: a compact head (state chip, duration, language pair, Start or Stop, Pause, the Translate switch), issues only when there is something to do, the captions as a conversation taking the remaining height, and a slim footer with the levels and the delay and the details behind a disclosure.
//!
//! The view model is [`CallPanel`] and the commands are the strings of the engine's queue; the
//! window adds no logic of its own beyond which button to show and how the captions scroll.
//! Clicks are collected as [`CallWindowAction`]s and applied by the caller after the frame.

use egui::{Align, Layout};

use crate::call_captions_bar;
use crate::call_timeline_view::{self, EmptyState, TimelineArgs};
use crate::call_view::{CallPanel, CallPhase, IssueSeverity, command};
use crate::call_window_details;
use crate::call_window_footer;
use crate::call_window_header::{self, language_codes, language_name};
use crate::call_window_issues;
use crate::call_window_state::{CallWindowState, CaptionSize};
use crate::settings_section::SettingsSection;
use crate::state::UiSettings;
use crate::theme::{Tokens, border, hairline, space};

/// What a click in the window asks for; the caller applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CallWindowAction {
    /// A call command string for the engine's queue.
    Command(&'static str),
    /// Open the settings window at a page.
    OpenSettings(SettingsSection),
}

/// The state of the call in plain words, with the reason when it is reduced.
pub(crate) fn state_text(panel: &CallPanel) -> String {
    match panel.phase {
        CallPhase::Idle => prxvc_i18n::t("callwin.state.idle"),
        CallPhase::Initializing => prxvc_i18n::t("callwin.state.loading"),
        CallPhase::AudioReady => prxvc_i18n::t("callwin.state.audio_ready"),
        CallPhase::Translating => prxvc_i18n::t("callwin.state.translating"),
        CallPhase::Paused => prxvc_i18n::t("callwin.state.paused"),
        CallPhase::Degraded => match reason(panel) {
            Some(key) => prxvc_i18n::t_with("callwin.state.degraded", &[&prxvc_i18n::t(key)]),
            None => prxvc_i18n::t("callwin.state.degraded_plain"),
        },
        CallPhase::Recovering => prxvc_i18n::t("callwin.state.recovering"),
        CallPhase::Stopping => prxvc_i18n::t("callwin.state.stopping"),
    }
}

/// The message key of the first reduced-operation issue, the reason of a degraded call.
fn reason(panel: &CallPanel) -> Option<&str> {
    panel
        .issues
        .iter()
        .find(|i| i.severity == IssueSeverity::Degraded)
        .map(|i| i.key.as_str())
}

/// What the empty captions say for the state of the call.
fn empty_state(panel: &CallPanel) -> EmptyState {
    let t = prxvc_i18n::t;
    let (title, body) = match panel.phase {
        CallPhase::Idle | CallPhase::Stopping => ("callwin.empty.idle.title", "callwin.empty.idle.body"),
        CallPhase::Initializing => ("callwin.empty.loading.title", "callwin.empty.loading.body"),
        CallPhase::AudioReady if !panel.translate_enabled => {
            ("callwin.empty.audio.title", "callwin.empty.audio.body")
        }
        CallPhase::AudioReady => ("callwin.empty.loading.title", "callwin.empty.loading.body"),
        CallPhase::Paused => ("callwin.empty.paused.title", "callwin.empty.paused.body"),
        CallPhase::Translating | CallPhase::Degraded | CallPhase::Recovering => {
            ("callwin.empty.listening.title", "callwin.empty.listening.body")
        }
    };
    EmptyState {
        title: t(title),
        body: t(body),
    }
}

/// Space toggles Pause and Resume while this window has the keyboard and nothing in it has focus
/// (a focused button would take the key itself).
fn handle_keys(ctx: &egui::Context, panel: &CallPanel, actions: &mut Vec<CallWindowAction>) {
    let focused = ctx.input(|i| i.viewport().focused != Some(false));
    if !focused || ctx.egui_wants_keyboard_input() || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    if !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
        return;
    }
    if panel.can_resume() {
        actions.push(CallWindowAction::Command(command::RESUME));
    } else if panel.can_pause() {
        actions.push(CallWindowAction::Command(command::PAUSE));
    }
}

/// Draw the window's content; clicks are appended to `actions`.
pub(crate) fn body(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    translate: &mut bool,
    settings: &mut UiSettings,
    w: &mut CallWindowState,
    actions: &mut Vec<CallWindowAction>,
) {
    w.sync(panel);
    handle_keys(ui.ctx(), panel, actions);
    // The footer is laid out first, from the bottom; the rest takes the height that is left.
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        footer(ui, tk, panel, settings, w);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            call_window_header::header(ui, tk, panel, translate, settings, w, actions);
            call_window_issues::issues(ui, tk, panel, actions);
            ui.add_space(space::S12);
            let (own, peer) = language_codes(panel, settings);
            let names = [prxvc_i18n::t("callwin.caption.me"), prxvc_i18n::t("callwin.caption.peer")];
            call_captions_bar::bar(ui, tk, w, settings, [&names[0], &names[1]]);
            ui.add_space(space::S8);
            // The remote party hears the user's translation (into the peer's language), the user
            // hears the peer's (into their own).
            let targets = [language_name(&peer), language_name(&own)];
            call_timeline_view::timeline(
                ui,
                tk,
                w,
                &TimelineArgs {
                    size: CaptionSize::from_setting(&settings.call_captions_size),
                    show_source: settings.call_captions_show_source,
                    targets: [&targets[0], &targets[1]],
                    empty: empty_state(panel),
                },
            );
        });
    });
}

/// The slim row with the details above it when they are open.
fn footer(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, settings: &mut UiSettings, w: &mut CallWindowState) {
    call_window_footer::footer_row(ui, tk, panel, w);
    if w.details_open {
        let max = (ui.ctx().content_rect().height() * 0.45).clamp(120.0, 360.0);
        egui::ScrollArea::vertical()
            .id_salt("call_details")
            .max_height(max)
            .auto_shrink([false, true])
            .show(ui, |ui| call_window_details::details(ui, tk, panel, settings));
    }
    // A hairline between the captions and the footer.
    let used = ui.min_rect();
    ui.painter().hline(
        used.x_range(),
        used.top() - space::S4,
        hairline(tk.line),
    );
    ui.add_space(space::S8 + border::DEFAULT);
}
