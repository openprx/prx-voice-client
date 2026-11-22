//! The Call translation window: a compact head (state chip, duration, language pair, Start or Stop, Pause, the Translate switch), issues only when there is something to do, the captions as a conversation taking the remaining height, and a slim footer with the levels and the delay and the details behind a disclosure.
//!
//! The view model is [`CallPanel`] and the commands are the strings of the engine's queue; the
//! window adds no logic of its own beyond which button to show and how the captions scroll.
//! Clicks are collected as [`CallWindowAction`]s and applied by the caller after the frame.

use crate::call_captions_bar;
use crate::call_timeline_view::{self, EmptyState, TimelineArgs};
use crate::call_view::{CallPanel, CallPhase, IssueSeverity, command};
use crate::call_window_details;
use crate::call_window_footer;
use crate::call_window_header::{self, language_codes, language_name};
use crate::call_window_issues;
use crate::call_window_notice;
use crate::call_window_send;
use crate::call_window_state::{CallWindowState, CaptionSize};
use crate::settings_section::SettingsSection;
use crate::state::UiSettings;
use crate::theme::{Tokens, hairline, layout, space};

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
        CallPhase::Idle | CallPhase::Stopping => {
            ("callwin.empty.idle.title", "callwin.empty.idle.body")
        }
        CallPhase::Initializing => ("callwin.empty.loading.title", "callwin.empty.loading.body"),
        CallPhase::AudioReady if !panel.translate_enabled => {
            ("callwin.empty.audio.title", "callwin.empty.audio.body")
        }
        CallPhase::AudioReady => ("callwin.empty.loading.title", "callwin.empty.loading.body"),
        CallPhase::Paused => ("callwin.empty.paused.title", "callwin.empty.paused.body"),
        CallPhase::Translating | CallPhase::Degraded | CallPhase::Recovering => (
            "callwin.empty.listening.title",
            "callwin.empty.listening.body",
        ),
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

/// The footer's height: the hairline's gap and one small control row.
const FOOTER_HEIGHT: f32 = space::S8 + layout::CONTROL_HEIGHT_SMALL;
/// The least height the captions or the details get; when less is left the whole body scrolls.
const MIN_FLEX_HEIGHT: f32 = 120.0;

/// Draw the window's content; clicks are appended to `actions`.
///
/// One vertical flow. The footer has a fixed height and sits at the bottom of the rect; above it a
/// scrolling body holds the fixed regions (header, notice line, issues, captions bar) at their
/// natural heights and then one flexible region, the captions or, while the details are open, the
/// details in place of them. The flexible region gets exactly what is left, never less than
/// [`MIN_FLEX_HEIGHT`]; when the window is too small for that, the body scrolls instead of
/// anything overlapping.
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
    let full = ui.available_rect_before_wrap();
    let top_h = (full.height() - FOOTER_HEIGHT).max(0.0);
    let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), top_h));
    let bottom = egui::Rect::from_min_max(egui::pos2(full.min.x, top.max.y), full.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("call_body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                flow(ui, tk, panel, translate, settings, w, actions, top_h);
            });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(bottom), |ui| {
        ui.painter()
            .hline(bottom.x_range(), bottom.top(), hairline(tk.line));
        ui.add_space(space::S8);
        call_window_footer::footer_row(ui, tk, panel, w);
    });
    ui.allocate_rect(full, egui::Sense::hover());
}

/// The scrolling body: the fixed regions, then the flexible one in what is left of `height`.
#[allow(clippy::too_many_arguments)]
fn flow(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    translate: &mut bool,
    settings: &mut UiSettings,
    w: &mut CallWindowState,
    actions: &mut Vec<CallWindowAction>,
    height: f32,
) {
    let origin = ui.cursor().top();
    call_window_header::header(ui, tk, panel, translate, settings, w, actions);
    call_window_notice::notice_line(ui, tk, panel, actions);
    call_window_send::indicator(ui, tk, panel, w.drained_for());
    call_window_issues::issues(ui, tk, panel, actions);
    ui.add_space(space::S12);
    if w.details_open {
        let left = flexible_height(ui, origin, height);
        flexible(ui, left, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("call_details")
                .auto_shrink([false, false])
                .max_height(left)
                .show(ui, |ui| {
                    call_window_details::details(ui, tk, panel, settings)
                });
        });
        return;
    }
    let (own, peer) = language_codes(panel, settings);
    let names = [
        prxvc_i18n::t("callwin.caption.me"),
        prxvc_i18n::t("callwin.caption.peer"),
    ];
    call_captions_bar::bar(ui, tk, w, settings, [&names[0], &names[1]]);
    ui.add_space(space::S8);
    // The remote party hears the user's translation (into the peer's language), the user
    // hears the peer's (into their own).
    let targets = [language_name(&peer), language_name(&own)];
    let left = flexible_height(ui, origin, height);
    let args = TimelineArgs {
        size: CaptionSize::from_setting(&settings.call_captions_size),
        show_source: settings.call_captions_show_source,
        targets: [&targets[0], &targets[1]],
        empty: empty_state(panel),
    };
    flexible(ui, left, |ui| {
        call_timeline_view::timeline(ui, tk, w, &args)
    });
}

/// What is left of `height` below the cursor (`origin` is where the flow began), at least
/// [`MIN_FLEX_HEIGHT`]; a hair less than the rest, so that rounding never makes the body scroll.
fn flexible_height(ui: &egui::Ui, origin: f32, height: f32) -> f32 {
    (height - (ui.cursor().top() - origin) - 0.5).max(MIN_FLEX_HEIGHT)
}

/// A region of exactly `height` and the full width.
fn flexible(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(ui.available_width(), height);
    ui.allocate_ui(size, |ui| {
        ui.set_min_size(size);
        ui.set_max_size(size);
        add(ui);
    });
}
