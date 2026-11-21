//! Settings page Voice: the user's voice profile and the preset voices (design 2.2; enrolment is a later package).

use egui::Ui;

use crate::settings_rows::{PageCx, note, select_row, status_row, switch_row, t};
use crate::theme::space;
use crate::widgets::Tone;

/// Options of a preset select: the catalogue default (empty id) first, then the presets;
/// a configured id that is not among them stays visible. Returns the options, the id of
/// each and the current index.
pub(crate) fn preset_options(presets: &[String], configured: &str) -> (Vec<String>, Vec<String>, usize) {
    let mut options = vec![t("settings.voice.preset.default")];
    let mut ids = vec![String::new()];
    for p in presets {
        options.push(p.clone());
        ids.push(p.clone());
    }
    let selected = if configured.is_empty() {
        0
    } else if let Some(i) = ids.iter().position(|id| id == configured) {
        i
    } else {
        options.push(configured.to_owned());
        ids.push(configured.to_owned());
        options.len() - 1
    };
    (options, ids, selected)
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let profile = cx.data.voice_profile.clone();

    // My voice (read-only; enrolment is a later package).
    let (tone, status, details) = match &profile {
        Some(p) => {
            let language = p
                .language
                .clone()
                .unwrap_or_else(|| t("settings.voice.language_unknown"));
            let prepared = if p.prepared_for.is_empty() {
                t("settings.voice.not_prepared")
            } else {
                prxvc_i18n::t_with("settings.voice.prepared_for", &[&p.prepared_for.join(", ")])
            };
            (
                Tone::Success,
                t("settings.voice.recorded"),
                format!("{} · {} · {}", p.id, language, prepared),
            )
        }
        None => (
            Tone::Neutral,
            t("settings.voice.not_set_up"),
            t("settings.voice.my_voice.desc"),
        ),
    };
    let action = t("settings.voice.set_up");
    status_row(
        ui,
        tk,
        &t("settings.voice.my_voice"),
        &details,
        tone,
        &status,
        Some((&action, false)),
    );
    note(ui, tk, &t("settings.reason.enrolment"));
    ui.add_space(space::S4);

    // Speak my translations in my voice: only with a profile.
    let ready = profile.is_some();
    let description = if ready {
        t("settings.voice.use_clone.desc")
    } else {
        format!(
            "{} {}",
            t("settings.voice.use_clone.desc"),
            t("settings.voice.use_clone.needs_profile")
        )
    };
    switch_row(
        ui,
        tk,
        &t("settings.voice.use_clone"),
        &description,
        &mut cx.settings.voice_use_clone,
        ready,
    );

    let (options, ids, selected) =
        preset_options(&cx.data.voice_presets, &cx.settings.voice_incoming_preset);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_voice_incoming",
        &t("settings.voice.incoming"),
        &t("settings.voice.incoming.desc"),
        &options,
        selected,
    ) {
        cx.settings.voice_incoming_preset = ids[i].clone();
    }
    let (options, ids, selected) =
        preset_options(&cx.data.voice_presets, &cx.settings.voice_fallback_preset);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_voice_fallback",
        &t("settings.voice.fallback"),
        &t("settings.voice.fallback.desc"),
        &options,
        selected,
    ) {
        cx.settings.voice_fallback_preset = ids[i].clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_preset_is_selected_for_an_empty_setting() {
        let (_, ids, selected) = preset_options(&["a".into(), "b".into()], "");
        assert_eq!(ids, ["", "a", "b"]);
        assert_eq!(selected, 0);
    }

    #[test]
    fn an_unknown_configured_preset_stays_visible() {
        let (options, ids, selected) = preset_options(&["a".into()], "zz");
        assert_eq!(ids[selected], "zz");
        assert_eq!(options[selected], "zz");
    }
}
