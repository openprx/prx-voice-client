//! The translation part of a call lane for the debug control: the languages, the level of the translated playback, the counters and the last latency figures. The captions themselves are the Call translation window's timeline (`call_timeline_view`).

use crate::call_debug_view::db;
use crate::call_view::{LaneView, LatencyView};
use crate::theme::{Tokens, size, space};
use crate::widgets;

/// Draw what a translating lane adds to the path numbers; nothing for a lane that does not translate.
pub(crate) fn translation_detail(ui: &mut egui::Ui, tk: &Tokens, lane: &LaneView) {
    if !lane.translating {
        return;
    }
    if let Some((from, to)) = &lane.languages {
        ui.label(widgets::text(
            prxvc_i18n::t_with("call.languages", &[from, to]),
            size::META,
            tk.text,
        ));
    }
    ui.label(widgets::text(
        prxvc_i18n::t_with(
            "call.translated_level",
            &[&db(lane.translated_rms), &db(lane.translated_peak)],
        ),
        size::META,
        tk.text,
    ));
    if let Some(c) = &lane.counters {
        let first = prxvc_i18n::t_with(
            "call.counters.line1",
            &[
                &c.late_dropped.to_string(),
                &c.backlog_skips.to_string(),
                &c.stale_skips.to_string(),
                &c.forced_cuts.to_string(),
                &c.final_rewrites.to_string(),
            ],
        );
        let second = prxvc_i18n::t_with(
            "call.counters.line2",
            &[
                &c.mt_suspect.to_string(),
                &c.runaway_synth.to_string(),
                &c.capture_overruns.to_string(),
                &c.underruns.to_string(),
                &c.refine_fallbacks.to_string(),
            ],
        );
        ui.label(widgets::text(first, size::META, tk.text));
        let third = prxvc_i18n::t_with(
            "call.counters.line3",
            &[
                &c.filler_dropped.to_string(),
                &c.unspeakable_dropped.to_string(),
            ],
        );
        ui.label(widgets::text(second, size::META, tk.text));
        ui.label(widgets::text(third, size::META, tk.text));
    }
    latency(ui, tk, lane.latency.as_ref());
}

fn latency(ui: &mut egui::Ui, tk: &Tokens, latency: Option<&LatencyView>) {
    ui.add_space(space::S4);
    let Some(l) = latency else {
        ui.label(widgets::text(
            prxvc_i18n::t("call.latency.none"),
            size::META,
            tk.secondary,
        ));
        return;
    };
    let ms = |v: Option<f64>| v.map_or_else(|| "-".to_owned(), |v| format!("{v:.0}"));
    ui.label(widgets::text(
        prxvc_i18n::t_with(
            "call.latency.start_to_audio",
            &[&ms(l.speech_start_to_first_audio_ms)],
        ),
        size::META,
        tk.text,
    ));
    ui.label(widgets::text(
        prxvc_i18n::t_with(
            "call.latency.pause_to_audio",
            &[&ms(l.pause_to_first_audio_ms)],
        ),
        size::META,
        tk.text,
    ));
    let stages = l
        .stages
        .iter()
        .map(|(name, v)| format!("{name} {v:.0}"))
        .collect::<Vec<_>>()
        .join(", ");
    ui.label(widgets::text(
        prxvc_i18n::t_with("call.latency.stages", &[&stages]),
        size::CAPTION,
        tk.secondary,
    ));
}
