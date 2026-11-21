//! Settings page Call translation: languages, call application, what the user hears (design 2.2).

use egui::Ui;

use crate::settings_rows::{
    PageCx, notice, radio_row, saved_only, segmented_row, select_row, switch_row, t,
    unavailable_row,
};
use crate::settings_view::{index_of, labels};
use crate::widgets::Tone;

/// Values of the language selects: empty follows the interface language.
const LANGUAGES: [&str; 3] = ["", "zh", "en"];
/// Values of `call.local_output_mode` in list order.
const OUTPUT_MODES: [&str; 3] = ["translation", "both", "original"];
/// Values of `call.outgoing_capture` in segment order.
const CAPTURE: [&str; 2] = ["hands_free", "push_to_talk"];

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let language_options = labels(&[
        "settings.call.lang.follow",
        "settings.call.lang.zh",
        "settings.call.lang.en",
    ]);

    let own = index_of(&LANGUAGES, &cx.settings.call_self_language);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_call_self",
        &t("settings.call.self_language"),
        &t("settings.call.self_language.desc"),
        &language_options,
        own,
    ) {
        // The two languages must differ; a clash is refused and explained below.
        let clash = !LANGUAGES[i].is_empty() && LANGUAGES[i] == cx.settings.call_peer_language;
        if !clash {
            cx.settings.call_self_language = LANGUAGES[i].to_owned();
        }
    }
    let peer = index_of(&LANGUAGES, &cx.settings.call_peer_language);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_call_peer",
        &t("settings.call.peer_language"),
        &t("settings.call.peer_language.desc"),
        &language_options,
        peer,
    ) {
        let clash = !LANGUAGES[i].is_empty() && LANGUAGES[i] == cx.settings.call_self_language;
        if !clash {
            cx.settings.call_peer_language = LANGUAGES[i].to_owned();
        }
    }
    let same = !cx.settings.call_self_language.is_empty()
        && cx.settings.call_self_language == cx.settings.call_peer_language;
    if same {
        notice(ui, tk, Tone::Danger, &t("settings.call.languages_differ"));
    }

    let app = if cx.settings.call_target_app.is_empty() {
        t("settings.call.target_app.choose")
    } else {
        cx.settings.call_target_app.clone()
    };
    unavailable_row(
        ui,
        tk,
        &t("settings.call.target_app"),
        &t("settings.call.target_app.desc"),
        "settings.reason.running_apps",
        Some(&app),
    );

    ui.add_space(crate::theme::space::S12);
    crate::settings_rows::body_medium(ui, tk, &t("settings.call.local_output"));
    crate::settings_rows::note(ui, tk, &t("settings.call.local_output.desc"));
    ui.add_space(crate::theme::space::S8);
    let options: Vec<(String, String)> = [
        (
            "settings.call.output.translation",
            "settings.call.output.translation.desc",
        ),
        (
            "settings.call.output.both",
            "settings.call.output.both.desc",
        ),
        (
            "settings.call.output.original",
            "settings.call.output.original.desc",
        ),
    ]
    .iter()
    .map(|(l, d)| (t(l), t(d)))
    .collect();
    let selected = index_of(&OUTPUT_MODES, &cx.settings.call_local_output_mode);
    if let Some(i) = radio_row(ui, tk, "settings_call_output", &options, selected) {
        cx.settings.call_local_output_mode = OUTPUT_MODES[i].to_owned();
    }
    ui.add_space(crate::theme::space::S8);

    if cx.settings.hotkey_push_to_talk.is_empty() {
        unavailable_row(
            ui,
            tk,
            &t("settings.call.capture"),
            &t("settings.call.capture.desc"),
            "settings.reason.ptt_needs_shortcut",
            Some(&t("settings.call.capture.hands_free")),
        );
    } else {
        let options = labels(&[
            "settings.call.capture.hands_free",
            "settings.call.capture.ptt",
        ]);
        let selected = index_of(&CAPTURE, &cx.settings.call_outgoing_capture);
        if let Some(i) = segmented_row(
            ui,
            tk,
            &t("settings.call.capture"),
            &saved_only(&t("settings.call.capture.desc")),
            &options,
            selected,
        ) {
            cx.settings.call_outgoing_capture = CAPTURE[i].to_owned();
        }
    }

    unavailable_row(
        ui,
        tk,
        &t("settings.call.half_duplex"),
        &t("settings.call.half_duplex.desc"),
        "settings.reason.half_duplex",
        None,
    );
    switch_row(
        ui,
        tk,
        &t("settings.call.captions_source"),
        &saved_only(&t("settings.call.captions_source.desc")),
        &mut cx.settings.call_captions_show_source,
        true,
    );
    switch_row(
        ui,
        tk,
        &t("settings.call.disclosure"),
        &saved_only(&t("settings.call.disclosure.desc")),
        &mut cx.settings.call_disclosure,
        true,
    );
}
