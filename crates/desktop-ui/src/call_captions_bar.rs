//! The row above the captions: the "show original" switch, the text size, copy the conversation and clear it.

use crate::call_transcript::Transcript;
use crate::call_window_state::{CallWindowState, CaptionSize};
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, size, space};
use crate::widgets::{self, ButtonKind};

/// Draw the bar; `names` are the words for the two speakers in the copied text.
pub(crate) fn bar(
    ui: &mut egui::Ui,
    tk: &Tokens,
    w: &mut CallWindowState,
    settings: &mut UiSettings,
    names: [&str; 2],
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(widgets::text(
            prxvc_i18n::t("callwin.captions"),
            size::SECONDARY,
            tk.text,
        ));
        ui.add_space(space::S8);
        let label = prxvc_i18n::t("callwin.show_original");
        ui.name_inputs(&label, |ui| {
            widgets::switch(ui, tk, &mut settings.call_captions_show_source);
        });
        ui.label(widgets::text(&label, size::META, tk.secondary));
        ui.add_space(space::S8);
        let current = CaptionSize::from_setting(&settings.call_captions_size);
        let labels: Vec<String> = CaptionSize::ALL.iter().map(|s| prxvc_i18n::t(s.key())).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let selected = CaptionSize::ALL.iter().position(|s| *s == current).unwrap_or(1);
        if let Some(i) = widgets::segmented(ui, tk, selected, &refs) {
            settings.call_captions_size = CaptionSize::ALL[i].setting().to_owned();
        }
        let empty = w.transcript.is_empty();
        if widgets::button_with(
            ui,
            tk,
            ButtonKind::Text,
            &prxvc_i18n::t("callwin.copy_all"),
            layout::CONTROL_HEIGHT_SMALL,
            !empty,
        )
        .clicked()
        {
            ui.ctx().copy_text(copy_all(
                &w.transcript,
                settings.call_captions_show_source,
                names,
            ));
        }
        if widgets::button_with(
            ui,
            tk,
            ButtonKind::Text,
            &prxvc_i18n::t("callwin.clear"),
            layout::CONTROL_HEIGHT_SMALL,
            !empty,
        )
        .clicked()
        {
            w.clear_captions();
        }
    });
}

/// The text "Copy conversation" puts on the clipboard.
pub(crate) fn copy_all(t: &Transcript, with_source: bool, names: [&str; 2]) -> String {
    t.conversation_text(with_source, names[0], names[1])
}
