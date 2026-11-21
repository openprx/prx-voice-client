//! Settings page Assistant: reply playback, the language, the dialogue backend and the assistant's voice (design 2.2).
//!
//! Recognition and the voice are the models of call translation (see Speech pipeline); this page
//! chooses what is specific to the assistant. A backend that cannot serve (the OpenPRX agent is
//! not available yet, a server that is not installed, a missing credential) is chosen like any
//! other and its problem is shown under the row, never replaced by another backend.

use egui::Ui;

use crate::settings_data::{DialogueKindView, PipelineData};
use crate::settings_pipeline::problem_text;
use crate::settings_rows::{PageCx, note, notice, select_row, status_row, switch_row, t};
use crate::settings_view::index_of;
use crate::settings_voice::preset_options;
use crate::theme::space;
use crate::widgets::Tone;

/// Values of `assistant.language` in the order of the select: the interface language, Chinese, English.
const LANGUAGES: [&str; 3] = ["", "zh-CN", "en"];

/// Values of `assistant.voice` in the order of the select.
const VOICES: [&str; 2] = ["preset", "clone"];

/// A description with the note that the setting is read when a session starts.
fn next_session(description: &str) -> String {
    format!("{description} {}", t("settings.note.next_session"))
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;

    switch_row(
        ui,
        tk,
        &t("settings.assistant.speak"),
        &next_session(&t("settings.auto_play.desc")),
        &mut cx.settings.auto_play,
        true,
    );

    let options = vec![
        t("settings.assistant.language.follow"),
        t("settings.call.lang.zh"),
        t("settings.call.lang.en"),
    ];
    let selected = index_of(&LANGUAGES, &cx.settings.assistant_language);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_assistant_language",
        &t("settings.assistant.language"),
        &next_session(&t("settings.assistant.language.desc")),
        &options,
        selected,
    ) {
        cx.settings.assistant_language = LANGUAGES[i].to_owned();
    }

    backend_rows(ui, cx);
    voice_rows(ui, cx);

    ui.add_space(space::S8);
    note(ui, tk, &t("settings.assistant.models_note"));
}

fn backend_rows(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let backends = cx.data.assistant.backends.clone();
    if backends.is_empty() {
        note(ui, tk, &t("settings.assistant.backend.loading"));
        return;
    }
    let ids: Vec<&str> = backends.iter().map(|b| b.id.as_str()).collect();
    let options: Vec<String> = backends
        .iter()
        .map(|b| match b.kind {
            DialogueKindView::LocalServer => t("settings.assistant.backend.local"),
            DialogueKindView::OpenPrx => t("settings.assistant.backend.openprx"),
            DialogueKindView::Entry if b.name.is_empty() => b.id.clone(),
            DialogueKindView::Entry => b.name.clone(),
        })
        .collect();
    // An empty setting is the built-in local server, the first of the list.
    let configured = cx.settings.dialogue_provider.as_str();
    let selected = if configured.is_empty() {
        0
    } else {
        index_of(&ids, configured)
    };
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_assistant_backend",
        &t("settings.assistant.backend"),
        &next_session(&t("settings.assistant.backend.desc")),
        &options,
        selected,
    ) {
        cx.settings.dialogue_provider = if i == 0 {
            String::new()
        } else {
            ids[i].to_owned()
        };
    }
    let shown = backends.get(selected);
    match shown.and_then(|b| b.problem.as_ref()) {
        Some(problem) => notice(ui, tk, Tone::Warning, &problem_text(problem)),
        None => {
            let ready = matches!(cx.data.assistant.chain, PipelineData::NotLoaded);
            status_row(
                ui,
                tk,
                &t("settings.assistant.backend.status"),
                &t("settings.assistant.backend.status.desc"),
                if ready { Tone::Neutral } else { Tone::Success },
                &t(if ready {
                    "settings.pipeline.loading"
                } else {
                    "settings.pipeline.ready"
                }),
                None,
            );
        }
    }
}

fn voice_rows(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let has_profile = cx.data.voice_profile.is_some();
    let options = vec![
        t("settings.assistant.voice.preset"),
        t("settings.assistant.voice.clone"),
    ];
    let selected = index_of(&VOICES, &cx.settings.assistant_voice);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_assistant_voice",
        &t("settings.assistant.voice"),
        &next_session(&t("settings.assistant.voice.desc")),
        &options,
        selected,
    ) {
        cx.settings.assistant_voice = VOICES[i].to_owned();
    }
    if cx.settings.assistant_voice == "clone" {
        if !has_profile {
            notice(
                ui,
                tk,
                Tone::Warning,
                &t("settings.assistant.voice.needs_profile"),
            );
        }
        return;
    }
    let (options, ids, selected) =
        preset_options(&cx.data.voice_presets, &cx.settings.assistant_voice_preset);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_assistant_voice_preset",
        &t("settings.assistant.voice.preset_name"),
        &next_session(&t("settings.assistant.voice.preset_name.desc")),
        &options,
        selected,
    ) {
        cx.settings.assistant_voice_preset = ids[i].clone();
    }
}
