//! The details part of the Call translation window: the languages, what each side hears, the counters, the flagged translations, the last delay and the notes the coordinator gave. Behind a disclosure; nothing here is needed to run a call.

use crate::call_view::{CallPanel, CaptionFlag, CountersView, Heard, LaneView};
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, size, space};
use crate::widgets;

/// Values of the language selects: empty follows the interface language.
const LANGUAGES: [&str; 3] = ["", "zh", "en"];

fn heard_text(you: bool, hears: Heard) -> String {
    match hears {
        Heard::Translation => prxvc_i18n::t("call.hears.translation"),
        Heard::Original if you => prxvc_i18n::t("call.hears.original_by_app"),
        Heard::Original => prxvc_i18n::t("call.hears.original"),
        Heard::Nothing => prxvc_i18n::t("call.hears.nothing"),
    }
}

/// The counters of one direction as one plain line.
pub(crate) fn counters_line(c: &CountersView) -> String {
    let skipped = c.backlog_skips + c.stale_skips;
    prxvc_i18n::t_with(
        "callwin.detail.counts",
        &[
            &c.filler_dropped.to_string(),
            &skipped.to_string(),
            &c.in_target_passed.to_string(),
            &c.mt_retries.to_string(),
            &c.mt_suspect.to_string(),
        ],
    )
}

/// The flagged translations by kind, or `None` when there are none.
pub(crate) fn flags_line(c: &CountersView) -> Option<String> {
    let parts: Vec<String> = CaptionFlag::ALL
        .iter()
        .filter(|f| c.flag(**f) > 0)
        .map(|f| format!("{} {}", prxvc_i18n::t(f.key()), c.flag(*f)))
        .collect();
    (!parts.is_empty()).then(|| prxvc_i18n::t_with("callwin.detail.flags", &[&parts.join(", ")]))
}

fn small(ui: &mut egui::Ui, tk: &Tokens, text: impl Into<String>) {
    ui.add(egui::Label::new(widgets::text(text, size::META, tk.secondary)).wrap());
}

fn lane(ui: &mut egui::Ui, tk: &Tokens, title_key: &str, hear_key: &str, lane: &LaneView) {
    ui.label(widgets::text(prxvc_i18n::t(title_key), size::META, tk.text));
    if !lane.running && !lane.translating {
        small(ui, tk, prxvc_i18n::t("callwin.lane.off"));
        return;
    }
    small(
        ui,
        tk,
        format!(
            "{}: {}  ({})",
            prxvc_i18n::t(hear_key),
            heard_text(hear_key == "call.you_hear", lane.hears),
            prxvc_i18n::t_with("callwin.level", &[&crate::call_debug_view::db(lane.rms)]),
        ),
    );
    if let Some(c) = &lane.counters {
        small(ui, tk, counters_line(c));
        if let Some(f) = flags_line(c) {
            small(ui, tk, f);
        }
    }
    if let Some(ms) = lane
        .latency
        .as_ref()
        .and_then(|l| l.speech_start_to_first_audio_ms)
    {
        small(
            ui,
            tk,
            prxvc_i18n::t_with("callwin.latency", &[&format!("{ms:.0}")]),
        );
    }
}

/// The two language selects: the settings rows, so a change here is a change there. They can
/// change while call mode is off.
fn languages(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, settings: &mut UiSettings) {
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
        ui.label(widgets::text(&label, size::META, tk.text));
        if editable {
            let width = ui.available_width().min(layout::FIELD_WIDTH);
            widgets::select_named(ui, tk, salt, width, &label, &options[selected], &options, selected)
                .map(|i| LANGUAGES[i])
                // The two languages must differ; a clash is refused.
                .filter(|l| l.is_empty() || *l != other)
                .map(str::to_owned)
        } else {
            small(ui, tk, options[selected].clone());
            None
        }
    };
    if let Some(v) = pick(ui, "callwin_own", "callwin.own_language", &own, &peer) {
        settings.call_self_language = v;
    }
    if let Some(v) = pick(ui, "callwin_peer", "callwin.peer_language", &peer, &own) {
        settings.call_peer_language = v;
    }
    if !editable {
        small(ui, tk, prxvc_i18n::t("callwin.language_locked"));
    }
}

/// The details content.
pub(crate) fn details(ui: &mut egui::Ui, tk: &Tokens, panel: &CallPanel, settings: &mut UiSettings) {
    ui.spacing_mut().item_spacing.y = space::S4;
    languages(ui, tk, panel, settings);
    crate::call_window_notice::switch_row(ui, tk, settings);
    ui.add_space(space::S8);
    lane(ui, tk, "callwin.lane.out", "call.they_hear", &panel.outgoing);
    ui.add_space(space::S4);
    lane(ui, tk, "callwin.lane.in", "call.you_hear", &panel.incoming);
    if !panel.issues.is_empty() {
        ui.add_space(space::S8);
        ui.label(widgets::text(
            prxvc_i18n::t("callwin.issues.title"),
            size::META,
            tk.text,
        ));
        for issue in panel.issues.iter().filter(|i| !i.detail.is_empty()) {
            small(ui, tk, issue.detail.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counters_line_has_every_figure_and_the_flags_line_only_the_flagged() {
        let mut c = CountersView {
            filler_dropped: 3,
            backlog_skips: 1,
            stale_skips: 1,
            in_target_passed: 2,
            mt_retries: 1,
            mt_suspect: 4,
            ..CountersView::default()
        };
        let line = counters_line(&c);
        for n in ["3", "2", "1", "4"] {
            assert!(line.contains(n), "{n} in {line}");
        }
        assert_eq!(flags_line(&c), None);
        c.flags[CaptionFlag::ALL.iter().position(|f| *f == CaptionFlag::Refusal).unwrap()] = 5;
        assert!(flags_line(&c).unwrap().contains('5'));
    }
}
