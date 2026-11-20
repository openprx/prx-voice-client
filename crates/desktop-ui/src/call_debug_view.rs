//! The temporary call-mode control inside the debug window: buttons, the state, what each side hears, devices, levels, counters and issues.
//!
//! Layout is minimal on purpose (the real call UI is a later package); every
//! string goes through i18n and every button has its label as accessible name.

use crate::call_view::{ActivityView, CallPanel, Heard, IssueSeverity, IssueView, LaneView, command};
use crate::theme::{Tokens, size, space};
use crate::widgets::{self, ButtonKind, Tone};

/// Draw the control; clicks push command strings into `commands`.
pub(crate) fn call_section(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, commands: &mut Vec<String>) {
    widgets::section_header(ui, tk, &prxvc_i18n::t("call.title"), false);

    ui.horizontal_wrapped(|ui| {
        let buttons = [
            (ButtonKind::Primary, "call.start", command::START, panel.can_start()),
            (ButtonKind::Secondary, "call.stop", command::STOP, panel.can_stop()),
            (ButtonKind::Secondary, "call.pause", command::PAUSE, panel.can_pause()),
            (ButtonKind::Secondary, "call.resume", command::RESUME, panel.can_resume()),
            (ButtonKind::Secondary, "call.reapply", command::REAPPLY_MICROPHONE, panel.can_reapply()),
        ];
        for (kind, key, cmd, enabled) in buttons {
            let label = prxvc_i18n::t(key);
            if widgets::button_with(ui, tk, kind, &label, crate::theme::layout::CONTROL_HEIGHT, enabled).clicked() {
                commands.push(cmd.to_owned());
            }
        }
    });
    ui.add_space(space::S8);

    row(ui, tk, "call.state", &prxvc_i18n::t(panel.phase.key()));
    row(ui, tk, "call.microphone", dash(&panel.microphone));
    row(ui, tk, "call.virtual_sink", dash(&panel.virtual_sink));
    row(ui, tk, "call.app", dash(&panel.call_app));
    row(ui, tk, "call.default_input", &default_input_text(panel));
    row(ui, tk, "call.activity", &activity_text(panel));

    ui.add_space(space::S8);
    lane(ui, tk, "call.path.outgoing", "call.they_hear", &panel.outgoing);
    lane(ui, tk, "call.path.incoming", "call.you_hear", &panel.incoming);

    ui.add_space(space::S8);
    ui.label(widgets::text(prxvc_i18n::t("call.issues"), size::SECONDARY, tk.text));
    if panel.issues.is_empty() {
        ui.label(widgets::text(prxvc_i18n::t("call.no_issues"), size::META, tk.secondary));
    }
    for issue in &panel.issues {
        issue_row(ui, tk, issue);
    }
    if let Some(error) = &panel.last_error {
        ui.add_space(space::S4);
        ui.label(widgets::text(prxvc_i18n::t("call.last_error"), size::SECONDARY, tk.text));
        issue_row(ui, tk, error);
    }
}

fn dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

fn row(ui: &mut egui::Ui, tk: &Tokens, label_key: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(widgets::text(prxvc_i18n::t(label_key), size::META, tk.secondary));
        ui.label(widgets::text(value, size::META, tk.text));
    });
}

fn default_input_text(panel: &CallPanel) -> String {
    if !panel.in_call_mode {
        prxvc_i18n::t("call.default_input.off")
    } else if panel.default_input_ours {
        prxvc_i18n::t("call.default_input.ours")
    } else {
        prxvc_i18n::t_with("call.default_input.displaced", &[dash(&panel.displaced_to)])
    }
}

fn activity_text(panel: &CallPanel) -> String {
    match panel.activity {
        ActivityView::Unknown => prxvc_i18n::t("call.activity.unknown"),
        ActivityView::Unavailable => prxvc_i18n::t("call.activity.unavailable"),
        ActivityView::NotRunning => prxvc_i18n::t("call.activity.not_running"),
        ActivityView::Idle => prxvc_i18n::t("call.activity.idle"),
        ActivityView::OnOurMicrophone => prxvc_i18n::t("call.activity.on_ours"),
        ActivityView::OnOtherMicrophone => {
            prxvc_i18n::t_with("call.activity.on_other", &[dash(&panel.activity_devices)])
        }
    }
}

fn lane(ui: &mut egui::Ui, tk: &Tokens, title_key: &str, hear_key: &str, lane: &LaneView) {
    ui.label(widgets::text(prxvc_i18n::t(title_key), size::SECONDARY, tk.text));
    if !lane.running {
        ui.label(widgets::text(prxvc_i18n::t("call.path.off"), size::META, tk.secondary));
        return;
    }
    row(ui, tk, hear_key, &heard_text(hear_key, lane.hears));
    row(ui, tk, "call.path.device", dash(&lane.device));
    let level = prxvc_i18n::t_with("call.path.level", &[&db(lane.rms), &db(lane.peak)]);
    ui.label(widgets::text(level, size::META, tk.text));
    let counters = prxvc_i18n::t_with(
        "call.path.counters",
        &[
            &lane.frames.to_string(),
            &lane.overruns.to_string(),
            &lane.drops.to_string(),
            &lane.underruns.to_string(),
        ],
    );
    ui.label(widgets::text(counters, size::META, tk.text));
}

fn heard_text(hear_key: &str, hears: Heard) -> String {
    match hears {
        Heard::Translation => prxvc_i18n::t("call.hears.translation"),
        // The local user hears the original from the call application itself in this build.
        Heard::Original if hear_key == "call.you_hear" => prxvc_i18n::t("call.hears.original_by_app"),
        Heard::Original => prxvc_i18n::t("call.hears.original"),
        Heard::Nothing => prxvc_i18n::t("call.hears.nothing"),
    }
}

/// A linear level in dBFS as text.
pub(crate) fn db(linear: f32) -> String {
    if linear <= 0.0 {
        "-inf".to_owned()
    } else {
        format!("{:.1}", 20.0 * linear.log10())
    }
}

fn issue_row(ui: &mut egui::Ui, tk: &Tokens, issue: &IssueView) {
    let tone = match issue.severity {
        IssueSeverity::Blocking => Tone::Danger,
        IssueSeverity::Degraded => Tone::Warning,
        IssueSeverity::Info => Tone::Neutral,
    };
    widgets::tip_bar(ui, tk, tone, &prxvc_i18n::t(&issue.key), |_| {});
    if !issue.detail.is_empty() {
        ui.label(widgets::text(&issue.detail, size::CAPTION, tk.secondary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_shown_in_dbfs() {
        assert_eq!(db(1.0), "0.0");
        assert_eq!(db(0.5), "-6.0");
        assert_eq!(db(0.0), "-inf");
    }
}
