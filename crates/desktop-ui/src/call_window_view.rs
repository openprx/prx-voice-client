//! The Call translation window: everything needed to run a translated call in one narrow column (about 420 px, growing): the state in plain words, Start and Stop, Pause and Resume, the Translate switch, the languages, what each side hears, the issues with their actions, and the live captions of both directions.
//!
//! The view model is the one the debug control used ([`CallPanel`]) and the commands are the same
//! strings; the window adds no logic of its own beyond which button to show. Clicks are collected
//! as [`CallWindowAction`]s and applied by the caller after the frame.

use crate::call_view::{
    CallPanel, CallPhase, CountersView, Heard, IssueAction, IssueSeverity, IssueView, LaneView,
    command,
};
use crate::settings_section::SettingsSection;
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, radius, size, space};
use crate::widgets::{self, ButtonKind, Tone};

/// What a click in the window asks for; the caller applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CallWindowAction {
    /// A call command string for the engine's queue.
    Command(&'static str),
    /// Open the settings window at a page.
    OpenSettings(SettingsSection),
}

/// Values of the language selects: empty follows the interface language.
const LANGUAGES: [&str; 3] = ["", "zh", "en"];

/// Draw the window's content; clicks are appended to `actions`.
pub(crate) fn body(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    translate: &mut bool,
    settings: &mut UiSettings,
    actions: &mut Vec<CallWindowAction>,
) {
    state_line(ui, tk, panel);
    ui.add_space(space::S8);
    controls(ui, tk, panel, translate, actions);
    ui.add_space(space::S12);
    languages(ui, tk, panel, settings);
    ui.add_space(space::S12);
    issues(ui, tk, panel, actions);
    ui.add_space(space::S12);
    lane(
        ui,
        tk,
        "callwin.lane.out",
        "call.they_hear",
        &panel.outgoing,
    );
    ui.add_space(space::S12);
    lane(ui, tk, "callwin.lane.in", "call.you_hear", &panel.incoming);
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

fn state_line(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel) {
    let tone = match panel.phase {
        CallPhase::Degraded | CallPhase::Recovering => tk.warning,
        CallPhase::Translating => tk.success,
        _ => tk.text,
    };
    ui.add(egui::Label::new(widgets::text(state_text(panel), size::SUBHEADING, tone)).wrap());
}

/// The one primary button (Start, or Stop while a call runs), Pause or Resume, and the Translate switch.
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
    });
    ui.add_space(space::S8);
    // The switch decides what the next start does; it can only change while call mode is off.
    let label = prxvc_i18n::t("call.translate");
    ui.horizontal(|ui| {
        ui.name_inputs(&label, |ui| {
            if panel.can_start() {
                widgets::switch(ui, tk, translate);
            } else {
                widgets::switch_with(ui, tk, &mut panel.translate_enabled.clone(), false);
            }
        });
        ui.label(widgets::text(&label, size::SECONDARY, tk.text));
    });
}

/// The two languages of the call (`call.self_language`, `call.peer_language`). They are the
/// settings rows, so a change here is a change there; they can change while call mode is off.
fn languages(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, settings: &mut UiSettings) {
    ui.label(widgets::text(
        prxvc_i18n::t("callwin.languages"),
        size::SECONDARY,
        tk.secondary,
    ));
    let options: Vec<String> = [
        "settings.call.lang.follow",
        "settings.call.lang.zh",
        "settings.call.lang.en",
    ]
    .iter()
    .map(|k| prxvc_i18n::t(k))
    .collect();
    let editable = panel.can_start();
    let (own, peer) = (
        settings.call_self_language.clone(),
        settings.call_peer_language.clone(),
    );
    let pick = |ui: &mut egui::Ui, salt: &str, label_key: &str, current: &str, other: &str| {
        let selected = LANGUAGES.iter().position(|l| *l == current).unwrap_or(0);
        let label = prxvc_i18n::t(label_key);
        let mut chosen = None;
        ui.label(widgets::text(&label, size::SECONDARY, tk.text));
        if editable {
            // The two languages must differ; a clash is refused.
            let width = ui.available_width().min(layout::FIELD_WIDTH);
            let picked = widgets::select_named(
                ui,
                tk,
                salt,
                width,
                &label,
                &options[selected],
                &options,
                selected,
            );
            chosen = picked
                .map(|i| LANGUAGES[i])
                .filter(|l| l.is_empty() || *l != other);
        } else {
            ui.label(widgets::text(
                &options[selected],
                size::SECONDARY,
                tk.secondary,
            ));
        }
        chosen.map(str::to_owned)
    };
    if let Some(v) = pick(ui, "callwin_own", "callwin.own_language", &own, &peer) {
        settings.call_self_language = v;
    }
    if let Some(v) = pick(ui, "callwin_peer", "callwin.peer_language", &peer, &own) {
        settings.call_peer_language = v;
    }
    if !editable {
        ui.label(widgets::text(
            prxvc_i18n::t("callwin.language_locked"),
            size::META,
            tk.secondary,
        ));
    }
}

fn issues(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, actions: &mut Vec<CallWindowAction>) {
    ui.label(widgets::text(
        prxvc_i18n::t("callwin.issues.title"),
        size::SECONDARY,
        tk.secondary,
    ));
    if panel.issues.is_empty() && panel.last_error.is_none() {
        ui.label(widgets::text(
            prxvc_i18n::t("callwin.issues.none"),
            size::META,
            tk.secondary,
        ));
    }
    ui.spacing_mut().item_spacing.y = space::S4;
    for (i, issue) in panel.issues.iter().enumerate() {
        ui.push_id(("issue", i), |ui| issue_row(ui, tk, issue, actions));
    }
    if let Some(error) = &panel.last_error {
        ui.push_id("last_error", |ui| issue_row(ui, tk, error, actions));
    }
}

/// The message key of the button label of an action, and what a click does.
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

fn issue_row(
    ui: &mut egui::Ui,
    tk: &Tokens,
    issue: &IssueView,
    actions: &mut Vec<CallWindowAction>,
) {
    let tone = match issue.severity {
        IssueSeverity::Blocking => Tone::Danger,
        IssueSeverity::Degraded => Tone::Warning,
        IssueSeverity::Info => Tone::Neutral,
    };
    ui.vertical(|ui| {
        tip_wrapped(ui, tk, tone, &prxvc_i18n::t(&issue.key));
        if !issue.detail.is_empty() {
            ui.add(
                egui::Label::new(widgets::text(&issue.detail, size::CAPTION, tk.secondary)).wrap(),
            );
        }
        if let Some((key, action)) = action_of(issue.action)
            && widgets::button_with(
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
    });
}

/// A tip bar whose message wraps (the window is narrow): soft fill, radius `card`, padding 8 / 12.
fn tip_wrapped(ui: &mut egui::Ui, tk: &Tokens, tone: Tone, message: &str) {
    let fill = match tone {
        Tone::Neutral => tk.surface,
        Tone::Success => tk.success_bg,
        Tone::Warning => tk.warning_bg,
        Tone::Danger => tk.danger_bg,
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(widgets::text(message, size::SECONDARY, tk.text)).wrap());
        });
}

fn heard_text(you: bool, hears: Heard) -> String {
    match hears {
        Heard::Translation => prxvc_i18n::t("call.hears.translation"),
        Heard::Original if you => prxvc_i18n::t("call.hears.original_by_app"),
        Heard::Original => prxvc_i18n::t("call.hears.original"),
        Heard::Nothing => prxvc_i18n::t("call.hears.nothing"),
    }
}

/// One direction: what its listener hears, the level, the captions, the notes and the last latency.
fn lane(ui: &mut egui::Ui, tk: &Tokens, title_key: &str, hear_key: &str, lane: &LaneView) {
    ui.label(widgets::text(
        prxvc_i18n::t(title_key),
        size::SECONDARY,
        tk.text,
    ));
    if !lane.running && !lane.translating {
        ui.label(widgets::text(
            prxvc_i18n::t("callwin.lane.off"),
            size::META,
            tk.secondary,
        ));
        return;
    }
    ui.add(
        egui::Label::new(widgets::text(
            format!(
                "{}: {}",
                prxvc_i18n::t(hear_key),
                heard_text(hear_key == "call.you_hear", lane.hears)
            ),
            size::META,
            tk.text,
        ))
        .wrap(),
    );
    ui.label(widgets::text(
        prxvc_i18n::t_with("callwin.level", &[&crate::call_debug_view::db(lane.rms)]),
        size::META,
        tk.secondary,
    ));
    if !lane.translating {
        return;
    }
    crate::call_captions_view::captions(ui, tk, lane);
    if let Some(c) = &lane.counters {
        notes(ui, tk, c);
    }
    crate::call_captions_view::latency_line(ui, tk, lane.latency.as_ref());
}

/// What the pipeline left out on purpose: filler it dropped and clauses it skipped to keep up.
fn notes(ui: &mut egui::Ui, tk: &Tokens, c: &CountersView) {
    let skipped = c.backlog_skips + c.stale_skips;
    if c.filler_dropped == 0 && skipped == 0 {
        return;
    }
    ui.add(
        egui::Label::new(widgets::text(
            prxvc_i18n::t_with(
                "callwin.notes",
                &[&c.filler_dropped.to_string(), &skipped.to_string()],
            ),
            size::META,
            tk.secondary,
        ))
        .wrap(),
    );
}
